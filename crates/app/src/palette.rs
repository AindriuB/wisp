//! Command palette items and ranking.

use std::path::PathBuf;

use wisp_storage::collection::{CollectionTree, Node};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandId {
    Send,
    Save,
    NewRequest,
    CloseTab,
    OpenCollection,
    SwitchEnvironment,
    ToggleTheme,
    ToggleLayout,
    ReloadFromDisk,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteKind {
    Request(PathBuf),
    Environment(Option<String>),
    Command(CommandId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteMode {
    All,
    Environments,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteItem {
    pub label: String,
    pub detail: String,
    pub kind: PaletteKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub index: usize,
    pub score: i64,
    pub positions: Vec<usize>,
}

impl CommandId {
    pub const ALL: [CommandId; 9] = [
        CommandId::Send,
        CommandId::Save,
        CommandId::NewRequest,
        CommandId::CloseTab,
        CommandId::OpenCollection,
        CommandId::SwitchEnvironment,
        CommandId::ToggleTheme,
        CommandId::ToggleLayout,
        CommandId::ReloadFromDisk,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CommandId::Send => "Send request",
            CommandId::Save => "Save request",
            CommandId::NewRequest => "New request",
            CommandId::CloseTab => "Close tab",
            CommandId::OpenCollection => "Open collection",
            CommandId::SwitchEnvironment => "Switch environment",
            CommandId::ToggleTheme => "Toggle theme",
            CommandId::ToggleLayout => "Toggle layout",
            CommandId::ReloadFromDisk => "Reload from disk",
        }
    }
}

fn collect_requests(nodes: &[Node], out: &mut Vec<PaletteItem>) {
    for n in nodes {
        match n {
            Node::Folder { children, .. } => collect_requests(children, out),
            Node::Request(m) => out.push(PaletteItem {
                label: m.name.clone(),
                detail: m.path.display().to_string(),
                kind: PaletteKind::Request(m.path.clone()),
            }),
            Node::Invalid { .. } => {}
        }
    }
}

pub fn items(
    tree: Option<&CollectionTree>,
    environments: &[String],
    active_env: Option<&str>,
    mode: PaletteMode,
) -> Vec<PaletteItem> {
    let mut out = Vec::new();
    match mode {
        PaletteMode::All => {
            if let Some(t) = tree {
                collect_requests(&t.nodes, &mut out);
            }
            out.extend(CommandId::ALL.iter().map(|c| PaletteItem {
                label: c.label().to_string(),
                detail: String::new(),
                kind: PaletteKind::Command(*c),
            }));
        }
        PaletteMode::Environments => {
            let mark = |on: bool| {
                if on {
                    "active".to_string()
                } else {
                    String::new()
                }
            };
            out.push(PaletteItem {
                label: "No environment".to_string(),
                detail: mark(active_env.is_none()),
                kind: PaletteKind::Environment(None),
            });
            out.extend(environments.iter().map(|e| PaletteItem {
                label: e.clone(),
                detail: mark(active_env == Some(e.as_str())),
                kind: PaletteKind::Environment(Some(e.clone())),
            }));
        }
    }
    out
}

/// Score `query` (lowercased chars) as a subsequence of `text`; returns the
/// best score and matched char indices.
fn score_text(text: &str, query: &[char]) -> Option<(i64, Vec<usize>)> {
    let chars: Vec<char> = text.chars().collect();
    let lower: Vec<char> = chars
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    let mut best: Option<(i64, Vec<usize>)> = None;
    for start in 0..lower.len() {
        if lower[start] != query[0] {
            continue;
        }
        let mut pos = vec![start];
        let mut at = start + 1;
        for q in &query[1..] {
            while at < lower.len() && lower[at] != *q {
                at += 1;
            }
            if at >= lower.len() {
                break;
            }
            pos.push(at);
            at += 1;
        }
        if pos.len() != query.len() {
            break; // later starts can only fail too
        }
        let mut score = 0i64;
        for (k, &p) in pos.iter().enumerate() {
            score += 10;
            let word_start = p == 0
                || !chars[p - 1].is_alphanumeric()
                || (chars[p - 1].is_lowercase() && chars[p].is_uppercase());
            if word_start {
                score += 10;
            }
            if k > 0 {
                let gap = (p - pos[k - 1] - 1) as i64;
                if gap == 0 {
                    score += 15;
                } else {
                    score -= gap.min(10);
                }
            }
        }
        score -= (start as i64).min(10);
        if start == 0 {
            score += 20;
            if pos.windows(2).all(|w| w[1] == w[0] + 1) {
                score += 30;
            }
        }
        if best.as_ref().is_none_or(|(b, _)| score > *b) {
            best = Some((score, pos));
        }
    }
    best
}

pub fn filter(items: &[PaletteItem], query: &str, limit: usize) -> Vec<Match> {
    let q: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(|c| c.to_lowercase())
        .collect();
    if q.is_empty() {
        return (0..items.len().min(limit))
            .map(|index| Match {
                index,
                score: 0,
                positions: Vec::new(),
            })
            .collect();
    }
    let mut out: Vec<Match> = items
        .iter()
        .enumerate()
        .filter_map(|(index, it)| {
            if let Some((score, positions)) = score_text(&it.label, &q) {
                Some(Match {
                    index,
                    score: score + 1000,
                    positions,
                })
            } else {
                score_text(&it.detail, &q).map(|(score, _)| Match {
                    index,
                    score,
                    positions: Vec::new(),
                })
            }
        })
        .collect();
    out.sort_by(|a, b| b.score.cmp(&a.score).then(a.index.cmp(&b.index)));
    out.truncate(limit);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_core::Method;
    use wisp_core::request::CollectionDefaults;
    use wisp_storage::collection::RequestMeta;

    fn item(label: &str, detail: &str) -> PaletteItem {
        PaletteItem {
            label: label.into(),
            detail: detail.into(),
            kind: PaletteKind::Command(CommandId::Send),
        }
    }

    fn order(items: &[PaletteItem], q: &str) -> Vec<usize> {
        filter(items, q, 100).into_iter().map(|m| m.index).collect()
    }

    #[test]
    fn items_all_and_envs() {
        let meta = |n: &str, p: &str| {
            Node::Request(RequestMeta {
                path: p.into(),
                name: n.into(),
                method: Method::Get,
            })
        };
        let tree = CollectionTree {
            root: "/r".into(),
            defaults: CollectionDefaults::default(),
            folders: Default::default(),
            nodes: vec![
                Node::Folder {
                    path: "users".into(),
                    name: "users".into(),
                    children: vec![
                        meta("Get user", "users/get.toml"),
                        Node::Invalid {
                            path: "users/bad.toml".into(),
                            error: "x".into(),
                        },
                    ],
                },
                meta("Ping", "ping.toml"),
            ],
        };
        let all = items(Some(&tree), &[], None, PaletteMode::All);
        assert_eq!(all.len(), 2 + 9);
        assert_eq!(all[0].label, "Get user");
        assert_eq!(all[0].detail, "users/get.toml");
        assert_eq!(all[0].kind, PaletteKind::Request("users/get.toml".into()));
        assert_eq!(all[2].label, "Send request");
        assert_eq!(items(None, &[], None, PaletteMode::All).len(), 9);

        let envs = vec!["dev".to_string(), "prod".to_string()];
        let e = items(None, &envs, Some("prod"), PaletteMode::Environments);
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].label, "No environment");
        assert_eq!(e[0].kind, PaletteKind::Environment(None));
        assert_eq!(e[0].detail, "");
        assert_eq!(e[2].detail, "active");
        assert_eq!(e[2].kind, PaletteKind::Environment(Some("prod".into())));
        let e = items(None, &envs, None, PaletteMode::Environments);
        assert_eq!(e[0].detail, "active");
    }

    #[test]
    fn empty_query_keeps_order_and_limit() {
        let v = vec![item("b", ""), item("a", ""), item("c", "")];
        assert_eq!(
            filter(&v, "", 2)
                .iter()
                .map(|m| m.index)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
    }

    #[test]
    fn ranking() {
        let v = vec![
            item("a long scattered thing", ""), // s..c..t scattered
            item("Send request", ""),
            item("Resend", ""),
            item("Save token", ""),
        ];
        // prefix first, then contiguous mid-word, then scattered
        assert_eq!(order(&v, "sen")[..2], [1, 2]);
        // word-start beats mid-word
        let w = vec![item("xrequest", ""), item("Send request", "")];
        assert_eq!(order(&w, "req"), vec![1, 0]);
        // contiguous beats scattered
        let c = vec![item("s_e_n_d", ""), item("send", "")];
        assert_eq!(order(&c, "send"), vec![1, 0]);
    }

    #[test]
    fn ties_and_exclusion_and_positions() {
        let v = vec![item("abc", ""), item("abc", ""), item("xyz", "")];
        assert_eq!(order(&v, "ABC"), vec![0, 1]);
        let m = filter(&v, "ac", 10);
        assert_eq!(m[0].positions, vec![0, 2]);
        let d = vec![item("zzz", "users/get.toml")];
        let m = filter(&d, "ugt", 10);
        assert_eq!(m.len(), 1);
        assert!(m[0].positions.is_empty());
        let u = vec![item("Ünï", "")];
        assert_eq!(filter(&u, "nï", 5)[0].positions, vec![1, 2]);
    }
}
