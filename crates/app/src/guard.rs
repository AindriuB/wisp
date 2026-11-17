//! The environment guard: pure decisions behind "Send to <environment>?".

use std::path::PathBuf;

use crate::settings::{GuardKey, Settings};

/// True when the (collection id, environment) pair is guarded.
pub fn needs_confirm(
    settings: &Settings,
    collection_id: Option<&str>,
    env_name: Option<&str>,
) -> bool {
    let (Some(id), Some(env)) = (collection_id, env_name) else {
        return false;
    };
    settings.confirm_send.contains(&GuardKey {
        collection_id: id.to_string(),
        environment: env.to_string(),
    })
}

/// What a send is aimed at: the tab, its collection and the environment name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub tab: u64,
    pub root: Option<PathBuf>,
    pub env: Option<String>,
}

/// The button the user pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Confirm,
    Cancel,
}

/// What to do once the confirm prompt is answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    Send,
    Abort,
    /// The environment changed to a guarded one: ask again for it.
    Reprompt,
    /// The environment changed to an unguarded one: send nothing and say so.
    AbortChanged,
}

pub fn after_prompt(
    confirmed: &Target,
    current: Option<&Target>,
    answer: Answer,
    current_guarded: bool,
) -> Next {
    if answer == Answer::Cancel {
        return Next::Abort;
    }
    let Some(current) = current else {
        return Next::Abort;
    };
    if current == confirmed {
        Next::Send
    } else if current.tab != confirmed.tab || current.root != confirmed.root {
        Next::Abort
    } else if current_guarded {
        Next::Reprompt
    } else {
        Next::AbortChanged
    }
}

/// The host of `url` without scheme, userinfo, path, query or fragment.
pub fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    authority
        .rsplit_once('@')
        .map_or(authority, |(_, h)| h)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(keys: &[(&str, &str)]) -> Settings {
        let mut s = Settings::default();
        for (id, env) in keys {
            s.confirm_send.insert(GuardKey {
                collection_id: id.to_string(),
                environment: env.to_string(),
            });
        }
        s
    }

    #[test]
    fn needs_confirm_table() {
        let s = settings(&[("c1", "prod"), ("c1", "a:b")]);
        let cases = [
            (Some("c1"), Some("prod"), true),
            (Some("c1"), Some("dev"), false),
            (Some("c2"), Some("prod"), false),
            (None, Some("prod"), false),
            (Some("c1"), None, false),
            (Some("c1"), Some("a:b"), true),
        ];
        for (id, env, want) in cases {
            assert_eq!(needs_confirm(&s, id, env), want, "{id:?} {env:?}");
        }
    }

    fn t(tab: u64, root: &str, env: Option<&str>) -> Target {
        Target {
            tab,
            root: Some(root.into()),
            env: env.map(String::from),
        }
    }

    #[test]
    fn after_prompt_table() {
        let c = t(1, "/a", Some("prod"));
        let cases = [
            (Some(c.clone()), Answer::Cancel, false, Next::Abort),
            (Some(c.clone()), Answer::Confirm, true, Next::Send),
            (
                Some(t(1, "/a", Some("dev"))),
                Answer::Confirm,
                true,
                Next::Reprompt,
            ),
            (
                Some(t(1, "/a", Some("dev"))),
                Answer::Confirm,
                false,
                Next::AbortChanged,
            ),
            (
                Some(t(1, "/a", None)),
                Answer::Confirm,
                false,
                Next::AbortChanged,
            ),
            (
                Some(t(1, "/b", Some("prod"))),
                Answer::Confirm,
                true,
                Next::Abort,
            ),
            (
                Some(t(2, "/a", Some("prod"))),
                Answer::Confirm,
                true,
                Next::Abort,
            ),
            (None, Answer::Confirm, false, Next::Abort),
        ];
        for (cur, ans, guarded, want) in cases {
            assert_eq!(
                after_prompt(&c, cur.as_ref(), ans, guarded),
                want,
                "{cur:?}"
            );
        }
    }

    #[test]
    fn host_drops_userinfo_and_path() {
        assert_eq!(host_of("https://u:p@api.test:8080/x?y=1"), "api.test:8080");
        assert_eq!(host_of("{{base}}/x"), "{{base}}");
        assert_eq!(host_of("http://h#f"), "h");
        assert_eq!(host_of("http://a/b@c"), "a");
    }
}
