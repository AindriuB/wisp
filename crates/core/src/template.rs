//! Scanning of `{{var}}` references and `{name}` path parameters in URLs.
//!
//! `resolve()` uses these same functions, so positions reported here agree
//! exactly with what resolution does. All ranges are byte ranges on char
//! boundaries into the input.

use std::ops::Range;

/// A `{{name}}` reference. `range` covers the braces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarRef {
    pub range: Range<usize>,
    /// Trimmed name between the braces.
    pub name: String,
    /// False when there is no closing `}}`; the range then runs to the end.
    pub terminated: bool,
}

/// A `{name}`, `{name:pattern}` or `{*name}` path parameter. `range` covers the braces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathParamRef {
    pub range: Range<usize>,
    pub name: String,
    pub pattern: Option<String>,
    pub catch_all: bool,
}

/// Every `{{name}}` in `s`, in order. An unterminated `{{` yields a final
/// entry with `terminated == false` and nothing after it is scanned.
pub fn var_refs(s: &str) -> Vec<VarRef> {
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(i) = s[pos..].find("{{") {
        let start = pos + i;
        let after = start + 2;
        match s[after..].find("}}") {
            Some(e) => {
                out.push(VarRef {
                    range: start..after + e + 2,
                    name: s[after..after + e].trim().to_string(),
                    terminated: true,
                });
                pos = after + e + 2;
            }
            None => {
                out.push(VarRef {
                    range: start..s.len(),
                    name: s[after..].trim().to_string(),
                    terminated: false,
                });
                break;
            }
        }
    }
    out
}

/// Byte offset of the first char matching `pred`, optionally ignoring `{{...}}` regions.
fn find_char(s: &str, from: usize, skip_vars: bool, pred: impl Fn(char) -> bool) -> Option<usize> {
    let vars = if skip_vars { var_refs(s) } else { Vec::new() };
    let mut k = 0;
    for (i, c) in s[from..].char_indices() {
        let i = from + i;
        while k < vars.len() && vars[k].range.end <= i {
            k += 1;
        }
        if k < vars.len() && vars[k].range.start <= i {
            continue;
        }
        if pred(c) {
            return Some(i);
        }
    }
    None
}

/// Offset of the first `?` that is not inside `{{...}}`.
pub fn query_start(url: &str) -> Option<usize> {
    find_char(url, 0, true, |c| c == '?')
}

fn split_path(url: &str, skip_vars: bool) -> Range<usize> {
    let delim = |c: char| matches!(c, '/' | '?' | '#');
    let from = url.find("://").map_or(0, |i| i + 3);
    let start = find_char(url, from, skip_vars, delim).unwrap_or(url.len());
    let end = find_char(url, start, skip_vars, |c| matches!(c, '?' | '#')).unwrap_or(url.len());
    start..end
}

/// The path portion: after `scheme://authority`, before `?` or `#`.
/// `{{...}}` regions are opaque.
pub fn path_range(url: &str) -> Range<usize> {
    split_path(url, true)
}

/// Like `path_range` but treats `{{` as ordinary text; used by `resolve()` on
/// the already-expanded URL.
pub(crate) fn path_range_plain(url: &str) -> Range<usize> {
    split_path(url, false)
}

/// One lexical item found in a path.
pub(crate) enum PathToken<'a> {
    /// A balanced `{...}`; `range` covers the braces, `inner` is between them.
    Param { range: Range<usize>, inner: &'a str },
    /// A `}` with no opener.
    StrayClose,
    /// A `{` with no matching `}`; scanning stops.
    Unclosed,
}

/// Scan `path` for brace groups. With `skip_vars`, `{{...}}` regions are opaque.
pub(crate) fn scan_path(path: &str, skip_vars: bool) -> Vec<PathToken<'_>> {
    let vars = if skip_vars {
        var_refs(path)
    } else {
        Vec::new()
    };
    let chars: Vec<(usize, char)> = path.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut k = 0;
    while i < chars.len() {
        let (pos, c) = chars[i];
        while k < vars.len() && vars[k].range.end <= pos {
            k += 1;
        }
        if k < vars.len() && vars[k].range.start <= pos {
            let end = vars[k].range.end;
            while i < chars.len() && chars[i].0 < end {
                i += 1;
            }
            continue;
        }
        match c {
            '}' => {
                out.push(PathToken::StrayClose);
                i += 1;
            }
            '{' => {
                let mut depth = 0usize;
                let mut close = None;
                for (j, &(_, ch)) in chars.iter().enumerate().skip(i) {
                    match ch {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                close = Some(j);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                let Some(j) = close else {
                    out.push(PathToken::Unclosed);
                    break;
                };
                let end = chars[j].0;
                out.push(PathToken::Param {
                    range: pos..end + 1,
                    inner: &path[pos + 1..end],
                });
                i = j + 1;
            }
            _ => i += 1,
        }
    }
    out
}

/// Parse the text between a path parameter's braces. `None` when invalid.
pub(crate) fn parse_param(inner: &str) -> Option<(String, Option<String>, bool)> {
    let (spec, pattern) = match inner.split_once(':') {
        Some((a, b)) => (a, Some(b.to_string())),
        None => (inner, None),
    };
    let (catch_all, name) = match spec.strip_prefix('*') {
        Some(n) => (true, n),
        None => (false, spec),
    };
    if name.is_empty() || name.contains(['{', '}']) {
        return None;
    }
    Some((name.to_string(), pattern, catch_all))
}

/// Valid path parameters in the path portion of `url` (not the query).
/// `{{var}}` references are skipped; malformed groups are not reported.
pub fn path_params(url: &str) -> Vec<PathParamRef> {
    let pr = path_range(url);
    let base = pr.start;
    scan_path(&url[pr], true)
        .into_iter()
        .filter_map(|t| match t {
            PathToken::Param { range, inner } => {
                let (name, pattern, catch_all) = parse_param(inner)?;
                Some(PathParamRef {
                    range: base + range.start..base + range.end,
                    name,
                    pattern,
                    catch_all,
                })
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_start_is_linear() {
        let url = "{{a}}".repeat(20_000);
        let t = std::time::Instant::now();
        assert_eq!(query_start(&url), None);
        assert!(t.elapsed() < std::time::Duration::from_millis(100));
    }

    #[test]
    fn plain_split_ignores_unclosed_var() {
        let url = "http://{{h/a/{id}";
        assert_eq!(path_range(url).len(), 0);
        assert_eq!(&url[path_range_plain(url)], "/a/{id}");
    }

    #[test]
    fn adjacent_vars() {
        let v = var_refs("{{a}}{{b}}");
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].range.clone(), v[0].name.as_str()), (0..5, "a"));
        assert_eq!((v[1].range.clone(), v[1].name.as_str()), (5..10, "b"));
        assert!(v.iter().all(|v| v.terminated));
    }

    #[test]
    fn unterminated() {
        let v = var_refs("x{{a");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].range, 1..4);
        assert_eq!(v[0].name, "a");
        assert!(!v[0].terminated);
    }

    #[test]
    fn name_is_trimmed() {
        let v = var_refs("{{ a }}");
        assert_eq!(v[0].name, "a");
        assert_eq!(v[0].range, 0..7);
    }

    #[test]
    fn question_mark_inside_var_is_not_query() {
        assert_eq!(query_start("{{a?b}}/x?y=1"), Some(9));
        assert_eq!(query_start("{{a?b}}/x"), None);
        assert_eq!(query_start("http://h/x"), None);
    }

    #[test]
    fn several_params_in_one_segment() {
        let p = path_params("http://h/{a}-{b}");
        assert_eq!(p.len(), 2);
        assert_eq!((p[0].name.as_str(), p[0].range.clone()), ("a", 9..12));
        assert_eq!((p[1].name.as_str(), p[1].range.clone()), ("b", 13..16));
        let p = path_params("/{name}.{ext}");
        assert_eq!(p[0].name, "name");
        assert_eq!(p[1].name, "ext");
    }

    #[test]
    fn pattern_capture() {
        let p = path_params(r"/u/{id:\d+}");
        assert_eq!(p[0].name, "id");
        assert_eq!(p[0].pattern.as_deref(), Some(r"\d+"));
        assert!(!p[0].catch_all);
    }

    #[test]
    fn catch_all() {
        let p = path_params("/f/{*path}");
        assert_eq!(p[0].name, "path");
        assert!(p[0].catch_all);
    }

    #[test]
    fn query_braces_not_reported() {
        assert!(path_params("http://h/p?x={x}").is_empty());
        assert!(path_params("http://h/p#{x}").is_empty());
    }

    #[test]
    fn vars_in_path_are_skipped() {
        let p = path_params("{{base}}/a/{id}");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].name, "id");
        assert_eq!(p[0].range, 11..15);
    }
}
