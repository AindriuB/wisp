//! The Cookies manager: the open collection's jar, one row per cookie, with the
//! "Keep cookies between sessions" toggle.
//!
//! Values are never read into a row or shown. The pure helpers at the top (record mapping,
//! the save debounce, the partition, labels) are what the unit tests cover.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use gpui::{
    Context, DismissEvent, Entity, EventEmitter, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement, PromptLevel, Render, SharedString, StatefulInteractiveElement as _, Styled,
    Subscription, Task, Window, div, px,
};
use gpui_component::button::Button;
use gpui_component::{ActiveTheme as _, Disableable as _, v_flex};
use wisp_core::cookies::CookieRecord;
use wisp_storage::cookies::StoredCookie;

use crate::actions::CancelRequest;
use crate::workspace::{Persistence, Workspace, WorkspaceEvent};

/// How long the jar must stay unchanged before a save runs.
pub const SAVE_QUIET: Duration = Duration::from_secs(2);

/// Persistent records as stored cookies. Session cookies (`expires: None`) are never persisted.
pub fn to_stored(records: Vec<CookieRecord>) -> Vec<StoredCookie> {
    records
        .into_iter()
        .filter_map(|r| {
            Some(StoredCookie {
                expires: r.expires?,
                name: r.name,
                value: r.value,
                domain: r.domain,
                host_only: r.host_only,
                path: r.path,
                secure: r.secure,
                http_only: r.http_only,
            })
        })
        .collect()
}

/// Stored cookies as jar records.
pub fn to_records(stored: Vec<StoredCookie>) -> Vec<CookieRecord> {
    stored
        .into_iter()
        .map(|c| CookieRecord {
            name: c.name,
            value: c.value,
            domain: c.domain,
            host_only: c.host_only,
            path: c.path,
            secure: c.secure,
            http_only: c.http_only,
            expires: Some(c.expires),
        })
        .collect()
}

/// The cookie-jar and OAuth2-token partition of a collection: its id, or the canonical root
/// path only when the id could be neither read nor created.
///
/// Copies or clones of a collection carry the same id, so they share this partition: the
/// in-memory jar, the OAuth2 tokens, the history and `cookies/<id>.json`. That is accepted.
pub fn cookie_jar_partition(id: Option<&str>, root: &Path) -> String {
    match id {
        Some(id) => id.to_owned(),
        None => std::fs::canonicalize(root)
            .unwrap_or_else(|_| PathBuf::from(root))
            .to_string_lossy()
            .into_owned(),
    }
}

/// Decides when a save is due. Times are passed in, so tests need no sleeps.
#[derive(Debug, Clone)]
pub struct Debounce {
    saved: u64,
    seen: u64,
    changed: Option<Instant>,
}

impl Debounce {
    pub fn new(generation: u64) -> Self {
        Debounce {
            saved: generation,
            seen: generation,
            changed: None,
        }
    }

    /// Note the jar's current generation.
    pub fn observe(&mut self, generation: u64, now: Instant) {
        if generation != self.seen {
            self.seen = generation;
            self.changed = Some(now);
        }
    }

    /// The jar changed since the last save.
    pub fn dirty(&self) -> bool {
        self.seen != self.saved
    }

    /// A save is due: changed, and quiet for [`SAVE_QUIET`].
    pub fn due(&self, now: Instant) -> bool {
        self.dirty()
            && self
                .changed
                .is_some_and(|c| now.saturating_duration_since(c) >= SAVE_QUIET)
    }

    /// Record that `generation` was handed to a save.
    pub fn saved(&mut self, generation: u64) {
        self.saved = generation;
    }
}

/// One listed cookie. No value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub domain: String,
    pub path: String,
    pub expiry: String,
    pub secure: bool,
    pub http_only: bool,
}

/// "Session", or how long until the cookie expires.
pub fn expiry_label(expires: Option<SystemTime>, now: SystemTime) -> String {
    let Some(t) = expires else {
        return "Session".into();
    };
    let secs = t.duration_since(now).map_or(0, |d| d.as_secs());
    match secs {
        0 => "expired".into(),
        1..=59 => format!("in {secs}s"),
        60..=3599 => format!("in {}m", secs / 60),
        3600..=86399 => format!("in {}h", secs / 3600),
        _ => format!("in {}d", secs / 86400),
    }
}

/// Rows for `records`, sorted by domain, path, name.
pub fn rows(records: &[CookieRecord], now: SystemTime) -> Vec<Row> {
    let mut out: Vec<Row> = records
        .iter()
        .map(|r| Row {
            name: r.name.clone(),
            domain: r.domain.clone(),
            path: r.path.clone(),
            expiry: expiry_label(r.expires, now),
            secure: r.secure,
            http_only: r.http_only,
        })
        .collect();
    out.sort_by(|a, b| (&a.domain, &a.path, &a.name).cmp(&(&b.domain, &b.path, &b.name)));
    out
}

pub struct CookieJarView {
    workspace: Entity<Workspace>,
    rows: Vec<Row>,
    /// The jar generation the rows were built at.
    seen: Option<Option<u64>>,
    focus: FocusHandle,
    prev_focus: Option<FocusHandle>,
    _sub: Subscription,
    _poll: Task<()>,
}

impl EventEmitter<DismissEvent> for CookieJarView {}

impl CookieJarView {
    pub fn new(workspace: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let prev_focus = window.focused(cx);
        window.focus(&focus, cx);
        let _sub = cx.subscribe(&workspace, |this, _, event, cx| {
            if matches!(
                event,
                WorkspaceEvent::CookiesChanged | WorkspaceEvent::CollectionChanged
            ) {
                this.seen = None;
                this.refresh(cx);
            }
        });
        // The jar changes with responses; the list follows while the panel is open.
        let _poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    break;
                }
            }
        });
        let mut this = Self {
            workspace,
            rows: Vec::new(),
            seen: None,
            focus,
            prev_focus,
            _sub,
            _poll,
        };
        this.refresh(cx);
        this
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let jar = self.workspace.read(cx).cookie_panel().jar;
        let generation = jar.as_ref().map(|j| j.generation());
        if self.seen == Some(generation) {
            return;
        }
        self.seen = Some(generation);
        self.rows = jar.map_or_else(Vec::new, |j| {
            rows(&j.snapshot(SystemTime::now()), SystemTime::now())
        });
        cx.notify();
    }

    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(h) = self.prev_focus.take() {
            window.focus(&h, cx);
        }
        cx.emit(DismissEvent);
    }

    fn delete(&mut self, row: &Row, cx: &mut Context<Self>) {
        if let Some(jar) = self.workspace.read(cx).cookie_panel().jar {
            jar.remove(&row.domain, &row.path, &row.name);
        }
        self.refresh(cx);
    }

    /// Ask, then run `then` on the workspace if the answer was the first button.
    fn confirm(
        &mut self,
        title: &str,
        detail: &str,
        yes: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Workspace, &mut Context<Workspace>) + 'static,
    ) {
        if window.has_active_prompt() || self.workspace.read(cx).prompt_up {
            return;
        }
        self.workspace.update(cx, |w, _| w.prompt_up = true);
        let answer = window.prompt(
            PromptLevel::Warning,
            title,
            Some(detail),
            &[yes, "Cancel"],
            cx,
        );
        let ws = self.workspace.clone();
        cx.spawn(async move |_, cx| {
            let yes = matches!(answer.await, Ok(0));
            ws.update(cx, |w, cx| {
                w.prompt_up = false;
                if yes {
                    then(w, cx);
                }
            });
        })
        .detach();
    }

    fn on_toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let panel = self.workspace.read(cx).cookie_panel();
        match panel.persistence {
            Persistence::Off => self
                .workspace
                .update(cx, |w, cx| w.set_cookie_persistence(true, cx)),
            Persistence::On | Persistence::Failed(_) => {
                let name = panel.name.unwrap_or_default();
                self.confirm(
                    &format!("Delete saved cookies for {name}?"),
                    "Turning this off removes the saved file. Cookies in this session are kept.",
                    "Delete",
                    window,
                    cx,
                    |w, cx| w.set_cookie_persistence(false, cx),
                );
            }
            Persistence::Unavailable(_) | Persistence::Pending => {}
        }
    }

    fn on_clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(jar) = self.workspace.read(cx).cookie_panel().jar else {
            return;
        };
        let this = cx.entity();
        self.confirm(
            "Clear all cookies?",
            "This removes every cookie in this collection's jar.",
            "Clear",
            window,
            cx,
            move |_, cx| {
                jar.clear();
                this.update(cx, |t, cx| t.refresh(cx));
            },
        );
    }
}

impl Render for CookieJarView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let panel = self.workspace.read(cx).cookie_panel();
        let title = match &panel.name {
            Some(n) => format!("Cookies for {n}"),
            None => "Cookies".to_string(),
        };
        let (toggle_label, toggle_off, banner) = match &panel.persistence {
            Persistence::On => ("Keep cookies between sessions: on", false, None),
            Persistence::Off => ("Keep cookies between sessions: off", false, None),
            Persistence::Pending => ("Keep cookies between sessions: ...", true, None),
            Persistence::Failed(r) => ("Keep cookies between sessions: on", false, Some(r.clone())),
            Persistence::Unavailable(r) => (
                "Keep cookies between sessions: unavailable",
                true,
                Some(r.clone()),
            ),
        };
        let mut body = v_flex().gap_2().child(div().text_lg().child(title)).child(
            Button::new("cookies-toggle")
                .label(toggle_label)
                .disabled(toggle_off)
                .on_click(cx.listener(|this, _, window, cx| this.on_toggle(window, cx))),
        );
        if let Some(reason) = banner {
            body = body.child(
                div()
                    .text_color(cx.theme().danger)
                    .child(format!("Cookies are not saved: {reason}")),
            );
        }
        if self.rows.is_empty() {
            body = body.child(div().text_color(muted).child("No cookies"));
        } else {
            let list = self.rows.iter().enumerate().map(|(ix, r)| {
                let row = r.clone();
                let flags = match (r.secure, r.http_only) {
                    (true, true) => "Secure, HttpOnly",
                    (true, false) => "Secure",
                    (false, true) => "HttpOnly",
                    (false, false) => "",
                };
                div()
                    .id(("cookie-row", ix))
                    .flex()
                    .flex_row()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .child(
                        div()
                            .w(px(120.))
                            .flex_none()
                            .truncate()
                            .child(SharedString::from(r.name.clone())),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(SharedString::from(r.domain.clone())),
                    )
                    .child(
                        div()
                            .w(px(80.))
                            .flex_none()
                            .truncate()
                            .text_color(muted)
                            .child(SharedString::from(r.path.clone())),
                    )
                    .child(
                        div()
                            .w(px(72.))
                            .flex_none()
                            .text_color(muted)
                            .child(r.expiry.clone()),
                    )
                    .child(div().w(px(112.)).flex_none().text_color(muted).child(flags))
                    .child(
                        Button::new(("cookie-delete", ix))
                            .label("Delete")
                            .on_click(cx.listener(move |this, _, _, cx| this.delete(&row, cx))),
                    )
            });
            body = body.child(
                div()
                    .id("cookie-list")
                    .max_h(px(380.))
                    .overflow_y_scroll()
                    .children(list),
            );
        }
        body = body.child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .child(
                    Button::new("cookies-clear")
                        .label("Clear all")
                        .disabled(self.rows.is_empty())
                        .on_click(cx.listener(|this, _, window, cx| this.on_clear(window, cx))),
                )
                .child(
                    Button::new("cookies-close")
                        .label("Close")
                        .on_click(cx.listener(|this, _, window, cx| this.dismiss(window, cx))),
                ),
        );
        div()
            .id("cookies-panel")
            .track_focus(&self.focus)
            .w(px(720.))
            .p_4()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover)
            .on_action(cx.listener(|this, _: &CancelRequest, window, cx| this.dismiss(window, cx)))
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &str, expires: Option<SystemTime>) -> CookieRecord {
        CookieRecord {
            name: name.into(),
            value: "secret-value".into(),
            domain: "example.test".into(),
            host_only: true,
            path: "/a".into(),
            secure: true,
            http_only: false,
            expires,
        }
    }

    #[test]
    fn cookie_jar_session_cookie_never_reaches_the_stored_list() {
        let t = SystemTime::now() + Duration::from_secs(600);
        let stored = to_stored(vec![record("s", None), record("p", Some(t))]);
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].name, "p");
    }

    #[test]
    fn cookie_jar_mapping_round_trips_the_other_fields() {
        let t = SystemTime::now() + Duration::from_secs(600);
        let original = record("p", Some(t));
        let back = to_records(to_stored(vec![original.clone()]));
        assert!(back == vec![original]);
    }

    #[test]
    fn cookie_jar_debounce_waits_for_two_quiet_seconds() {
        let t0 = Instant::now();
        let mut d = Debounce::new(0);
        assert!(!d.due(t0));
        d.observe(1, t0);
        assert!(!d.due(t0 + Duration::from_millis(1999)));
        assert!(d.due(t0 + Duration::from_secs(2)));
    }

    #[test]
    fn cookie_jar_debounce_restarts_on_each_change_and_clears_when_saved() {
        let t0 = Instant::now();
        let mut d = Debounce::new(0);
        d.observe(1, t0);
        d.observe(2, t0 + Duration::from_millis(1500));
        assert!(!d.due(t0 + Duration::from_secs(2)));
        assert!(d.due(t0 + Duration::from_millis(3500)));
        d.saved(2);
        assert!(!d.dirty());
        assert!(!d.due(t0 + Duration::from_secs(60)));
        d.observe(3, t0 + Duration::from_secs(60));
        assert!(d.dirty());
    }

    #[test]
    fn cookie_jar_partition_is_the_id_when_there_is_one() {
        assert_eq!(
            cookie_jar_partition(Some("abcdef0123456789"), Path::new("/nope")),
            "abcdef0123456789"
        );
    }

    #[test]
    fn cookie_jar_partition_falls_back_to_the_canonical_root() {
        let d = tempfile::tempdir().unwrap();
        let canon = std::fs::canonicalize(d.path()).unwrap();
        let via_dots = d.path().join(".");
        assert_eq!(
            cookie_jar_partition(None, &via_dots),
            canon.to_string_lossy()
        );
    }

    #[test]
    fn cookie_jar_rows_hide_values_and_say_session() {
        let now = SystemTime::now();
        let r = rows(
            &[
                record("s", None),
                record("p", Some(now + Duration::from_secs(7200))),
            ],
            now,
        );
        assert_eq!(r[0].name, "p");
        assert_eq!(r[0].expiry, "in 2h");
        assert_eq!(r[1].expiry, "Session");
        assert!(!format!("{r:?}").contains("secret-value"));
    }

    #[test]
    fn cookie_jar_expiry_labels() {
        let now = SystemTime::now();
        let l = |s| expiry_label(Some(now + Duration::from_secs(s)), now);
        assert_eq!(l(30), "in 30s");
        assert_eq!(l(600), "in 10m");
        assert_eq!(l(3 * 86400), "in 3d");
        assert_eq!(
            expiry_label(Some(now - Duration::from_secs(5)), now),
            "expired"
        );
    }
}
