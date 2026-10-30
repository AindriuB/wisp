//! Draft vs saved comparison and the disk-change decision table.

use wisp_core::request::Request;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DirtySections {
    pub name: bool,
    pub method: bool,
    pub url: bool,
    pub params: bool,
    pub path: bool,
    pub headers: bool,
    pub auth: bool,
    pub body: bool,
    pub settings: bool,
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
        Some(s) => s != draft,
        None => *draft != blank_request(),
    }
}

/// Which sections differ between `saved` and `draft`.
///
/// `Request.vars` has no flag in `DirtySections` (frozen in task 21). v1 has
/// no editor for per-request vars, so they change only via external file
/// edits; such a draft is dirty (`is_dirty` true) with no section highlighted.
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
        headers: base.headers != draft.headers,
        auth: base.auth != draft.auth,
        body: base.body != draft.body,
        settings: base.settings != draft.settings,
    }
}

pub fn on_disk_change(
    dirty: bool,
    saved: Option<&Request>,
    disk: Option<Result<Request, String>>,
) -> ReloadAction {
    match disk {
        None => ReloadAction::MarkDeleted,
        Some(Err(_)) => {
            if dirty {
                ReloadAction::ShowBanner
            } else {
                ReloadAction::Ignore
            }
        }
        Some(Ok(d)) => {
            if saved == Some(&d) {
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
    fn each_section_flags_alone() {
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
        let ok = |r: &Request| Some(Ok(r.clone()));
        assert_eq!(
            on_disk_change(false, Some(&s), ok(&s)),
            ReloadAction::Ignore
        );
        assert_eq!(on_disk_change(true, Some(&s), ok(&s)), ReloadAction::Ignore);
        assert_eq!(
            on_disk_change(false, Some(&s), ok(&other)),
            ReloadAction::Reload
        );
        assert_eq!(
            on_disk_change(true, Some(&s), ok(&other)),
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
        let err = || Some(Err("bad".to_string()));
        assert_eq!(
            on_disk_change(true, Some(&s), err()),
            ReloadAction::ShowBanner
        );
        assert_eq!(on_disk_change(false, Some(&s), err()), ReloadAction::Ignore);
    }
}
