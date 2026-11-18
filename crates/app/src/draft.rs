//! Draft vs saved comparison and the disk-change decision table.

use wisp_core::request::Request;
use wisp_storage::request::{assertions_equal, requests_equal};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DirtySections {
    pub name: bool,
    pub method: bool,
    pub url: bool,
    pub params: bool,
    pub path: bool,
    pub vars: bool,
    pub headers: bool,
    pub auth: bool,
    pub body: bool,
    pub settings: bool,
    pub assertions: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadAction {
    Reload,
    ShowBanner,
    MarkDeleted,
    Ignore,
}

pub fn blank_request() -> Request {
    Request {
        name: "Untitled".to_string(),
        ..Request::default()
    }
}

pub fn is_dirty(saved: Option<&Request>, draft: &Request) -> bool {
    match saved {
        Some(s) => !requests_equal(s, draft),
        None => !requests_equal(draft, &blank_request()),
    }
}

/// Which sections differ between `saved` and `draft`.
pub fn dirty_sections(saved: Option<&Request>, draft: &Request) -> DirtySections {
    let blank;
    let base = match saved {
        Some(s) => s,
        None => {
            blank = blank_request();
            &blank
        }
    };
    DirtySections {
        name: base.name != draft.name,
        method: base.method != draft.method,
        url: base.url != draft.url,
        params: base.params != draft.params,
        path: base.path != draft.path,
        vars: base.vars != draft.vars,
        headers: base.headers != draft.headers,
        auth: base.auth != draft.auth,
        body: base.body != draft.body,
        settings: base.settings != draft.settings,
        assertions: !assertions_equal(&base.assertions, &draft.assertions),
    }
}

pub fn on_disk_change(
    dirty: bool,
    saved: Option<&Request>,
    disk: Option<Result<&Request, ()>>,
) -> ReloadAction {
    match disk {
        None => ReloadAction::MarkDeleted,
        Some(Err(())) => ReloadAction::ShowBanner,
        Some(Ok(d)) => {
            if saved.is_some_and(|saved| requests_equal(saved, d)) {
                ReloadAction::Ignore
            } else if dirty {
                ReloadAction::ShowBanner
            } else {
                ReloadAction::Reload
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_core::Method;
    use wisp_core::assert::{Assertion, Expected, Op};
    use wisp_core::request::{Auth, Body, KeyValue, RequestSettings, Var};

    fn base() -> Request {
        Request {
            name: "r".into(),
            ..Request::default()
        }
    }

    #[test]
    fn dirty_basics() {
        let s = base();
        assert!(!is_dirty(Some(&s), &s.clone()));
        let mut d = s.clone();
        d.url = "x".into();
        assert!(is_dirty(Some(&s), &d));
        assert_eq!(blank_request().name, "Untitled");
        assert!(!is_dirty(None, &blank_request()));
        assert!(is_dirty(None, &base()));
    }

    #[test]
    fn nan_assertions_are_clean_until_the_definition_changes() {
        let mut saved = base();
        saved.assertions = vec![Assertion::Json {
            path: "$.n".into(),
            op: Op::Equals,
            expected: Expected::Float(f64::NAN),
        }];
        let mut draft = saved.clone();
        assert!(!is_dirty(Some(&saved), &draft));
        assert_eq!(
            dirty_sections(Some(&saved), &draft),
            DirtySections::default()
        );
        assert_eq!(
            on_disk_change(false, Some(&saved), Some(Ok(&draft))),
            ReloadAction::Ignore
        );
        draft.assertions = vec![Assertion::Json {
            path: "$.n".into(),
            op: Op::Equals,
            expected: Expected::Float(0.0),
        }];
        assert!(is_dirty(Some(&saved), &draft));
        assert!(dirty_sections(Some(&saved), &draft).assertions);
    }

    #[test]
    fn each_section_flags_alone_vars_included() {
        let s = base();
        let kv = vec![KeyValue {
            name: "a".into(),
            value: "1".into(),
            enabled: true,
        }];
        let mut cases: Vec<(Request, DirtySections)> = Vec::new();
        let mut d = s.clone();
        d.name = "n".into();
        cases.push((
            d,
            DirtySections {
                name: true,
                ..Default::default()
            },
        ));
        let mut d = s.clone();
        d.method = Method::Post;
        cases.push((
            d,
            DirtySections {
                method: true,
                ..Default::default()
            },
        ));
        let mut d = s.clone();
        d.url = "u".into();
        cases.push((
            d,
            DirtySections {
                url: true,
                ..Default::default()
            },
        ));
        let mut d = s.clone();
        d.params = kv.clone();
        cases.push((
            d,
            DirtySections {
                params: true,
                ..Default::default()
            },
        ));
        let mut d = s.clone();
        d.path = vec![Var {
            name: "a".into(),
            value: "".into(),
        }];
        cases.push((
            d,
            DirtySections {
                path: true,
                ..Default::default()
            },
        ));
        let mut d = s.clone();
        d.vars = vec![Var {
            name: "a".into(),
            value: "1".into(),
        }];
        cases.push((
            d,
            DirtySections {
                vars: true,
                ..Default::default()
            },
        ));
        let mut d = s.clone();
        d.headers = kv.clone();
        cases.push((
            d,
            DirtySections {
                headers: true,
                ..Default::default()
            },
        ));
        let mut d = s.clone();
        d.auth = Auth::None;
        cases.push((
            d,
            DirtySections {
                auth: true,
                ..Default::default()
            },
        ));
        let mut d = s.clone();
        d.body = Body::Form(kv);
        cases.push((
            d,
            DirtySections {
                body: true,
                ..Default::default()
            },
        ));
        let mut d = s.clone();
        d.settings = RequestSettings {
            follow_redirects: false,
            ..RequestSettings::default()
        };
        cases.push((
            d,
            DirtySections {
                settings: true,
                ..Default::default()
            },
        ));
        let mut d = s.clone();
        d.assertions = vec![Assertion::Status {
            op: Op::Equals,
            expected: Expected::Int(200),
        }];
        cases.push((
            d,
            DirtySections {
                assertions: true,
                ..Default::default()
            },
        ));
        for (d, want) in cases {
            assert_eq!(dirty_sections(Some(&s), &d), want);
        }
        assert_eq!(dirty_sections(Some(&s), &s), DirtySections::default());
    }

    #[test]
    fn unsaved_compares_to_blank() {
        let mut d = blank_request();
        assert_eq!(dirty_sections(None, &d), DirtySections::default());
        d.name = "x".into();
        d.url = "u".into();
        assert_eq!(
            dirty_sections(None, &d),
            DirtySections {
                name: true,
                url: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn disk_change_table() {
        let s = base();
        let mut other = s.clone();
        other.url = "z".into();

        assert_eq!(
            on_disk_change(false, Some(&s), Some(Ok(&s))),
            ReloadAction::Ignore
        );
        assert_eq!(
            on_disk_change(true, Some(&s), Some(Ok(&s))),
            ReloadAction::Ignore
        );
        assert_eq!(
            on_disk_change(false, Some(&s), Some(Ok(&other))),
            ReloadAction::Reload
        );
        assert_eq!(
            on_disk_change(true, Some(&s), Some(Ok(&other))),
            ReloadAction::ShowBanner
        );
        assert_eq!(
            on_disk_change(false, Some(&s), None),
            ReloadAction::MarkDeleted
        );
        assert_eq!(
            on_disk_change(true, Some(&s), None),
            ReloadAction::MarkDeleted
        );
        let err = || Some(Err(()));
        assert_eq!(
            on_disk_change(true, Some(&s), err()),
            ReloadAction::ShowBanner
        );
        assert_eq!(
            on_disk_change(false, Some(&s), err()),
            ReloadAction::ShowBanner
        );
    }
}
