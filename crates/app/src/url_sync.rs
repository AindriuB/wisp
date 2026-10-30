//! Keeps the URL text, the params table and path variables consistent.

use wisp_core::request::{KeyValue, Var};
use wisp_core::template::{path_params, query_start, var_refs};

/// Byte offsets of `c` in `s` that are not inside `{{...}}`.
fn positions_outside_vars(s: &str, c: char) -> Vec<usize> {
    let refs = var_refs(s);
    s.char_indices()
        .filter(|(i, ch)| *ch == c && !refs.iter().any(|r| r.range.contains(i)))
        .map(|(i, _)| i)
        .collect()
}

/// Splits `url` into (base, query-with-no-`?`, fragment-with-`#`).
fn split_url(url: &str) -> (&str, Option<&str>, &str) {
    let hash = positions_outside_vars(url, '#').first().copied();
    let (head, frag) = match hash {
        Some(h) => (&url[..h], &url[h..]),
        None => (url, ""),
    };
    match query_start(head) {
        Some(q) => (&head[..q], Some(&head[q + 1..]), frag),
        None => (head, None, frag),
    }
}

fn split_outside_vars(s: &str, c: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for p in positions_outside_vars(s, c) {
        out.push(&s[start..p]);
        start = p + c.len_utf8();
    }
    out.push(&s[start..]);
    out
}

/// The URL as shown in the URL bar: `url` plus the enabled params, raw.
///
/// Names are not escaped, so an enabled param with an empty name, or a name
/// containing `=` or `&`, does not round-trip through `apply_url_text`.
pub fn display_url(url: &str, params: &[KeyValue]) -> String {
    let query: Vec<String> = params
        .iter()
        .filter(|p| p.enabled)
        .map(|p| {
            if p.value.is_empty() {
                p.name.clone()
            } else {
                format!("{}={}", p.name, p.value)
            }
        })
        .collect();
    if query.is_empty() {
        return url.to_string();
    }
    let (base, existing, frag) = split_url(url);
    let mut s = base.to_string();
    if let Some(q) = existing {
        s.push('?');
        s.push_str(q);
        s.push('&');
    } else {
        s.push('?');
    }
    s.push_str(&query.join("&"));
    s.push_str(frag);
    s
}

/// Reads URL bar text back into (url without query, params).
pub fn apply_url_text(text: &str, params: &[KeyValue]) -> (String, Vec<KeyValue>) {
    let (base, query, frag) = split_url(text);
    let url = format!("{base}{frag}");
    let Some(query) = query else {
        return (url, merge_parsed_params(params, Vec::new()));
    };
    let mut parsed: Vec<KeyValue> = Vec::new();
    for seg in split_outside_vars(query, '&') {
        if seg.is_empty() {
            continue;
        }
        let (name, value) = match positions_outside_vars(seg, '=').first() {
            Some(&e) => (&seg[..e], &seg[e + 1..]),
            None => (seg, ""),
        };
        parsed.push(KeyValue {
            name: name.to_string(),
            value: value.to_string(),
            enabled: true,
        });
    }
    (url, merge_parsed_params(params, parsed))
}

/// Order-preserving alignment of `parsed` against the enabled entries `en`
/// of `old`. Pairs are found by the longest common subsequence on name and
/// value, then, within each gap between those pairs, on name only, then, within
/// the remaining gaps, by slot (the i-th leftover with the i-th leftover).
/// Records `matched[k] = Some(old index)` for each paired parsed entry.
fn align(
    old: &[KeyValue],
    en: &[usize],
    parsed: &[KeyValue],
    (p_lo, p_hi): (usize, usize),
    (o_lo, o_hi): (usize, usize),
    level: u8,
    matched: &mut [Option<usize>],
) {
    let (np, no) = (p_hi - p_lo, o_hi - o_lo);
    if np == 0 || no == 0 {
        return;
    }
    let eq = |k: usize, e: usize| {
        let (n, o) = (&parsed[k], &old[en[e]]);
        match level {
            0 => n.name == o.name && n.value == o.value,
            _ => n.name == o.name,
        }
    };
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    if level >= 2 {
        pairs.extend((0..np.min(no)).map(|i| (p_lo + i, o_lo + i)));
    } else {
        let mut dp = vec![vec![0usize; no + 1]; np + 1];
        for i in 1..=np {
            for j in 1..=no {
                dp[i][j] = if eq(p_lo + i - 1, o_lo + j - 1) {
                    dp[i - 1][j - 1] + 1
                } else {
                    dp[i - 1][j].max(dp[i][j - 1])
                };
            }
        }
        let (mut i, mut j) = (np, no);
        while i > 0 && j > 0 {
            if eq(p_lo + i - 1, o_lo + j - 1) {
                pairs.push((p_lo + i - 1, o_lo + j - 1));
                i -= 1;
                j -= 1;
            } else if dp[i - 1][j] > dp[i][j - 1] {
                i -= 1;
            } else {
                j -= 1;
            }
        }
        pairs.reverse();
    }
    for &(k, e) in &pairs {
        matched[k] = Some(en[e]);
    }
    if level >= 2 {
        return;
    }
    let (mut pl, mut ol) = (p_lo, o_lo);
    for (k, e) in pairs.into_iter().chain([(p_hi, o_hi)]) {
        align(old, en, parsed, (pl, k), (ol, e), level + 1, matched);
        pl = k + 1;
        ol = e + 1;
    }
}

/// Merges the params parsed from the URL bar into the old list. Enabled
/// params follow the URL order and keep the identity of the old enabled entry
/// they align with (see `align`). Each disabled entry stays right after the
/// nearest preceding old enabled entry that survives, or at the front if none
/// does.
fn merge_parsed_params(old: &[KeyValue], parsed: Vec<KeyValue>) -> Vec<KeyValue> {
    let en: Vec<usize> = (0..old.len()).filter(|&i| old[i].enabled).collect();
    let mut used = vec![false; old.len()];
    let mut matched: Vec<Option<usize>> = vec![None; parsed.len()];
    align(
        old,
        &en,
        &parsed,
        (0, parsed.len()),
        (0, en.len()),
        0,
        &mut matched,
    );
    for i in matched.iter().flatten() {
        used[*i] = true;
    }
    // Anchor of each disabled old entry: index of the surviving enabled entry
    // before it, `None` for the front.
    let mut anchor: Option<usize> = None;
    let mut disabled: Vec<(Option<usize>, &KeyValue)> = Vec::new();
    for (i, o) in old.iter().enumerate() {
        if !o.enabled {
            disabled.push((anchor, o));
        } else if used[i] {
            anchor = Some(i);
        }
    }
    let mut out: Vec<KeyValue> = disabled
        .iter()
        .filter(|(a, _)| a.is_none())
        .map(|(_, o)| (*o).clone())
        .collect();
    for (n, m) in parsed.into_iter().zip(matched) {
        out.push(n);
        if let Some(i) = m {
            out.extend(
                disabled
                    .iter()
                    .filter(|(a, _)| *a == Some(i))
                    .map(|(_, o)| (*o).clone()),
            );
        }
    }
    out
}

/// One `Var` per distinct path param name in URL order, keeping known values.
pub fn sync_path_vars(url: &str, path: &[Var]) -> Vec<Var> {
    let mut out: Vec<Var> = Vec::new();
    for p in path_params(url) {
        if out.iter().any(|v| v.name == p.name) {
            continue;
        }
        let value = path
            .iter()
            .find(|v| v.name == p.name)
            .map(|v| v.value.clone())
            .unwrap_or_default();
        out.push(Var {
            name: p.name,
            value,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(v: &[KeyValue]) -> Vec<String> {
        v.iter()
            .map(|k| format!("{}={}", k.name, k.value))
            .collect()
    }

    #[test]
    fn rename_keeps_disabled_in_slot() {
        let old = [kv("a", "1", true), kv("off", "z", false)];
        let (_, p) = apply_url_text("h?ab=1", &old);
        assert_eq!(pairs(&p), ["ab=1", "off=z"]);
        let old = [
            kv("a", "1", true),
            kv("off", "z", false),
            kv("b", "2", true),
        ];
        let (_, p) = apply_url_text("h?x=1&b=2", &old);
        assert_eq!(pairs(&p), ["x=1", "off=z", "b=2"]);
        let old = [
            kv("a", "1", true),
            kv("b", "2", true),
            kv("off", "z", false),
        ];
        let (_, p) = apply_url_text("h?a=1&bx=2", &old);
        assert_eq!(pairs(&p), ["a=1", "bx=2", "off=z"]);
    }

    #[test]
    fn deleting_first_of_repeated_names() {
        let old = [
            kv("a", "1", true),
            kv("off", "z", false),
            kv("a", "2", true),
            kv("off2", "y", false),
        ];
        let (_, p) = apply_url_text("h?a=2", &old);
        assert_eq!(pairs(&p), ["off=z", "a=2", "off2=y"]);
    }

    fn kv(n: &str, v: &str, e: bool) -> KeyValue {
        KeyValue {
            name: n.into(),
            value: v.into(),
            enabled: e,
        }
    }

    #[test]
    fn display_enabled_only_raw() {
        let p = [
            kv("a", "1", true),
            kv("x", "9", false),
            kv("b", "", true),
            kv("c", "{{v}}", true),
        ];
        assert_eq!(display_url("http://h/p", &p), "http://h/p?a=1&b&c={{v}}");
        assert_eq!(
            display_url("http://h/p", &[kv("x", "9", false)]),
            "http://h/p"
        );
        assert_eq!(display_url("u", &[]), "u");
    }

    #[test]
    fn display_keeps_fragment_last() {
        assert_eq!(display_url("u#f", &[kv("a", "1", true)]), "u?a=1#f");
    }

    #[test]
    fn apply_splits_and_reads_empty_values() {
        let (u, p) = apply_url_text("http://h/p?a=1&b&c=&a=2", &[]);
        assert_eq!(u, "http://h/p");
        assert_eq!(
            p,
            vec![
                kv("a", "1", true),
                kv("b", "", true),
                kv("c", "", true),
                kv("a", "2", true)
            ]
        );
    }

    #[test]
    fn apply_question_mark_in_var_not_split() {
        let (u, p) = apply_url_text("http://h/{{a?b}}", &[]);
        assert_eq!(u, "http://h/{{a?b}}");
        assert!(p.is_empty());
        let (u, p) = apply_url_text("h?x={{a&b}}", &[]);
        assert_eq!(u, "h");
        assert_eq!(p, vec![kv("x", "{{a&b}}", true)]);
    }

    #[test]
    fn apply_fragment_stays_in_url() {
        let (u, p) = apply_url_text("h?a=1#frag", &[]);
        assert_eq!(u, "h#frag");
        assert_eq!(p, vec![kv("a", "1", true)]);
    }

    #[test]
    fn apply_empty_query_gives_no_params() {
        let (u, p) = apply_url_text("x?", &[kv("a", "1", true)]);
        assert_eq!(u, "x");
        assert!(p.is_empty());
    }

    #[test]
    fn apply_keeps_disabled_positions() {
        let old = [
            kv("a", "1", true),
            kv("off", "z", false),
            kv("b", "2", true),
        ];
        let (_, p) = apply_url_text("h?a=5&b=6&c=7", &old);
        assert_eq!(
            p,
            vec![
                kv("a", "5", true),
                kv("off", "z", false),
                kv("b", "6", true),
                kv("c", "7", true)
            ]
        );
        let (_, p) = apply_url_text("h", &old);
        assert_eq!(p, vec![kv("off", "z", false)]);
    }

    #[test]
    fn apply_removal_keeps_disabled_anchor_by_name() {
        let old = [
            kv("a", "1", true),
            kv("off", "z", false),
            kv("b", "2", true),
        ];
        let (_, p) = apply_url_text("h?b=2", &old);
        assert_eq!(p, vec![kv("off", "z", false), kv("b", "2", true)]);
    }

    #[test]
    fn apply_reorder_matches_by_name() {
        let old = [
            kv("a", "1", true),
            kv("off", "z", false),
            kv("b", "2", true),
        ];
        let (_, p) = apply_url_text("h?b=2&a=1", &old);
        assert_eq!(
            p,
            vec![
                kv("b", "2", true),
                kv("a", "1", true),
                kv("off", "z", false)
            ]
        );
    }

    #[test]
    fn round_trip() {
        for t in [
            "h",
            "h?a=1",
            "h?a=1&a=2&b",
            "h/{{x?y}}?q={{z}}#f",
            "h?a=1#f",
        ] {
            let (u, p) = apply_url_text(t, &[]);
            assert_eq!(display_url(&u, &p), t);
        }
    }

    #[test]
    fn path_vars_sync() {
        let old = [
            Var {
                name: "id".into(),
                value: "7".into(),
            },
            Var {
                name: "gone".into(),
                value: "x".into(),
            },
        ];
        let out = sync_path_vars("http://h/{id}/{new}/{id}", &old);
        assert_eq!(
            out,
            vec![
                Var {
                    name: "id".into(),
                    value: "7".into()
                },
                Var {
                    name: "new".into(),
                    value: String::new()
                }
            ]
        );
    }

    #[test]
    fn delete_one_add_another_keeps_disabled_anchor() {
        let old = [
            kv("a", "1", true),
            kv("off", "z", false),
            kv("b", "2", true),
        ];
        let (_, p) = apply_url_text("h?b=2&c=3", &old);
        assert_eq!(pairs(&p), ["off=z", "b=2", "c=3"]);
    }

    #[test]
    fn repeated_names_stay_order_preserving() {
        let old = [kv("a", "", true), kv("off", "z", false), kv("a", "2", true)];
        let (_, p) = apply_url_text("h?a=2&a=2", &old);
        assert_eq!(pairs(&p), ["a=2", "off=z", "a=2"]);
    }
}
