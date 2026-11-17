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

    fn confirm_settings() -> Settings {
        settings(&[("c1", "prod"), ("c1", "a:b")])
    }

    #[test]
    fn a_guarded_collection_and_environment_needs_confirmation() {
        let s = confirm_settings();
        assert!(needs_confirm(&s, Some("c1"), Some("prod")));
    }

    #[test]
    fn an_unguarded_environment_of_a_guarded_collection_needs_none() {
        let s = confirm_settings();
        assert!(!needs_confirm(&s, Some("c1"), Some("dev")));
    }

    #[test]
    fn the_same_environment_in_another_collection_needs_none() {
        let s = confirm_settings();
        assert!(!needs_confirm(&s, Some("c2"), Some("prod")));
    }

    #[test]
    fn a_request_without_a_collection_needs_none() {
        let s = confirm_settings();
        assert!(!needs_confirm(&s, None, Some("prod")));
    }

    #[test]
    fn a_request_without_an_environment_needs_none() {
        let s = confirm_settings();
        assert!(!needs_confirm(&s, Some("c1"), None));
    }

    #[test]
    fn an_environment_name_containing_a_colon_is_guarded() {
        let s = confirm_settings();
        assert!(needs_confirm(&s, Some("c1"), Some("a:b")));
    }

    fn t(tab: u64, root: &str, env: Option<&str>) -> Target {
        Target {
            tab,
            root: Some(root.into()),
            env: env.map(String::from),
        }
    }

    fn confirmed() -> Target {
        t(1, "/a", Some("prod"))
    }

    #[test]
    fn cancelling_the_prompt_aborts() {
        let c = confirmed();
        assert_eq!(
            after_prompt(&c, Some(&c), Answer::Cancel, false),
            Next::Abort
        );
    }

    #[test]
    fn confirming_an_unchanged_target_sends() {
        let c = confirmed();
        assert_eq!(
            after_prompt(&c, Some(&c), Answer::Confirm, true),
            Next::Send
        );
    }

    #[test]
    fn confirming_after_a_switch_to_another_guarded_environment_prompts_again() {
        let cur = t(1, "/a", Some("dev"));
        assert_eq!(
            after_prompt(&confirmed(), Some(&cur), Answer::Confirm, true),
            Next::Reprompt
        );
    }

    #[test]
    fn confirming_after_a_switch_to_an_unguarded_environment_aborts_as_changed() {
        let cur = t(1, "/a", Some("dev"));
        assert_eq!(
            after_prompt(&confirmed(), Some(&cur), Answer::Confirm, false),
            Next::AbortChanged
        );
    }

    #[test]
    fn confirming_after_the_environment_was_cleared_aborts_as_changed() {
        let cur = t(1, "/a", None);
        assert_eq!(
            after_prompt(&confirmed(), Some(&cur), Answer::Confirm, false),
            Next::AbortChanged
        );
    }

    #[test]
    fn confirming_after_the_collection_root_changed_aborts() {
        let cur = t(1, "/b", Some("prod"));
        assert_eq!(
            after_prompt(&confirmed(), Some(&cur), Answer::Confirm, true),
            Next::Abort
        );
    }

    #[test]
    fn confirming_after_the_tab_changed_aborts() {
        let cur = t(2, "/a", Some("prod"));
        assert_eq!(
            after_prompt(&confirmed(), Some(&cur), Answer::Confirm, true),
            Next::Abort
        );
    }

    #[test]
    fn confirming_after_the_tab_closed_aborts() {
        assert_eq!(
            after_prompt(&confirmed(), None, Answer::Confirm, false),
            Next::Abort
        );
    }

    #[test]
    fn host_drops_userinfo_and_path() {
        assert_eq!(host_of("https://u:p@api.test:8080/x?y=1"), "api.test:8080");
        assert_eq!(host_of("{{base}}/x"), "{{base}}");
        assert_eq!(host_of("http://h#f"), "h");
        assert_eq!(host_of("http://a/b@c"), "a");
    }
}
