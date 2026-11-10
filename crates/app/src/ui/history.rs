//! The history panel: the open collection's sent requests, newest first.
//!
//! Listing, loading and clearing run on the background executor (see the
//! workspace methods); the pure helpers at the top are what the unit tests
//! cover. Redacted values are never shown or restored: the store keeps them
//! empty and a resend resolves against the current environment.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{
    Context, DismissEvent, Entity, EventEmitter, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled, Subscription,
    Window, div, px,
};
use gpui_component::button::Button;
use gpui_component::{ActiveTheme as _, v_flex};
use wisp_core::body::BodyHandle;
use wisp_core::{EngineError, Method};
use wisp_storage::history::{HistoryEntry, Outcome};

use crate::actions::CancelRequest;
use crate::tab::{HistoryOrigin, Phase, ResponseState};
use crate::workspace::{Workspace, WorkspaceEvent};

/// The body handed to the history append: none when the setting is off.
pub fn body_to_store(enabled: bool, body: Option<Vec<u8>>) -> Option<Vec<u8>> {
    if enabled { body } else { None }
}

fn method_text(m: Method) -> String {
    format!("{m:?}").to_uppercase()
}

/// Status or outcome of an entry for its list row.
pub fn outcome_label(entry: &HistoryEntry) -> String {
    match &entry.outcome {
        Outcome::Completed => entry
            .status
            .map_or_else(|| "Completed".to_string(), |s| s.to_string()),
        Outcome::Failed { kind } => format!("Failed: {kind:?}"),
        Outcome::Cancelled => "Cancelled".to_string(),
    }
}

/// How long before `now_ms` the entry was recorded, in words.
pub fn relative_time(now_ms: u64, at_ms: u64) -> String {
    let secs = now_ms.saturating_sub(at_ms) / 1000;
    let unit = |n: u64, u: &str| format!("{n} {u} ago");
    match secs {
        0..=9 => "just now".to_string(),
        10..=59 => unit(secs, "s"),
        60..=3599 => unit(secs / 60, "min"),
        3600..=86_399 => unit(secs / 3600, "h"),
        _ => unit(secs / 86_400, "d"),
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// The Notice shown when a from-history tab is sent again.
pub fn resend_notice(
    env: Option<&str>,
    adjustments: &[String],
    redacted: &[String],
    original_missing: bool,
    url_edited: bool,
) -> String {
    let mut text = format!(
        "Resent with the current environment ({})",
        env.unwrap_or("no environment")
    );
    if !adjustments.is_empty() {
        text.push_str("; because values were not stored: ");
        text.push_str(&adjustments.join(", "));
    }
    if !redacted.is_empty() {
        text.push_str("; redacted values were not stored: ");
        text.push_str(&redacted.join(", "));
    }
    if original_missing {
        text.push_str("; the original request was not found, so only collection defaults applied");
    }
    if url_edited {
        text.push_str(
            "; the URL was edited, so only collection defaults applied, not the original folder's",
        );
    }
    text
}

/// The line above the response of a from-history tab.
pub fn origin_line(origin: &HistoryOrigin, now_ms: u64) -> String {
    let mut line = format!("From history, {}", relative_time(now_ms, origin.at_ms));
    if origin.body_truncated {
        line.push_str(". Body truncated at 1 MiB");
    } else if !origin.body_stored {
        line.push_str(". Body not stored");
    }
    line
}

/// The stored response of an entry as a tab's response state.
pub fn stored_response(
    entry: &HistoryEntry,
    headers: Vec<wisp_core::Header>,
    body: Option<BodyHandle>,
) -> ResponseState {
    let ms = |v: Option<u64>| v.map(Duration::from_millis);
    let mut r = ResponseState {
        status: entry.status,
        headers,
        ttfb: ms(entry.ttfb_ms),
        total: ms(entry.total_ms),
        size: entry.size,
        received: entry.size.unwrap_or(0),
        body,
        ..ResponseState::default()
    };
    match &entry.outcome {
        Outcome::Completed => r.phase = Phase::Done,
        Outcome::Failed { kind } => {
            r.phase = Phase::Failed;
            r.error = Some(EngineError {
                kind: *kind,
                message: String::new(),
            });
        }
        Outcome::Cancelled => r.phase = Phase::Cancelled,
    }
    r
}

enum Listing {
    Loading,
    Ready(Vec<HistoryEntry>),
    /// History is off or unavailable; the text says why.
    Unavailable(String),
}

pub struct HistoryView {
    workspace: Entity<Workspace>,
    listing: Listing,
    /// Bumped per load so a slow older load cannot overwrite a newer one.
    generation: u64,
    focus: FocusHandle,
    prev_focus: Option<FocusHandle>,
    _sub: Subscription,
}

impl EventEmitter<DismissEvent> for HistoryView {}

impl HistoryView {
    pub fn new(workspace: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let prev_focus = window.focused(cx);
        window.focus(&focus, cx);
        let _sub = cx.subscribe(&workspace, |this, _, event, cx| {
            if matches!(event, WorkspaceEvent::HistoryChanged) {
                this.reload(cx);
            }
        });
        let mut this = Self {
            workspace,
            listing: Listing::Loading,
            generation: 0,
            focus,
            prev_focus,
            _sub,
        };
        this.reload(cx);
        this
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let job = self.workspace.read(cx).history_job();
        let job = match job {
            Ok(j) => j,
            Err(reason) => {
                self.listing = Listing::Unavailable(reason);
                cx.notify();
                return;
            }
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { job.list() })
                .await;
            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.listing = match result {
                    Ok(v) => Listing::Ready(v),
                    Err(e) => Listing::Unavailable(e),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(h) = self.prev_focus.take() {
            window.focus(&h, cx);
        }
        cx.emit(DismissEvent);
    }

    fn open(&mut self, entry: HistoryEntry, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace
            .update(cx, |w, cx| w.open_history_entry(entry, cx));
        self.dismiss(window, cx);
    }
}

impl Render for HistoryView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let now = now_ms();
        let mut body = v_flex().gap_2().child(div().text_lg().child("History"));
        match &self.listing {
            Listing::Loading => body = body.child(div().text_color(muted).child("Loading...")),
            Listing::Unavailable(reason) => {
                body = body.child(
                    div()
                        .text_color(muted)
                        .child(format!("History is unavailable: {reason}")),
                )
            }
            Listing::Ready(entries) if entries.is_empty() => {
                body = body.child(div().text_color(muted).child("No history yet"))
            }
            Listing::Ready(entries) => {
                let rows = entries.iter().enumerate().map(|(ix, e)| {
                    let entry = e.clone();
                    let failed = !matches!(e.outcome, Outcome::Completed);
                    let hover = cx.theme().secondary;
                    div()
                        .id(("history-row", ix))
                        .flex()
                        .flex_row()
                        .gap_2()
                        .px_2()
                        .py_1()
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .child(div().w(px(56.)).flex_none().child(method_text(e.method)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(SharedString::from(e.url.clone())),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_color(if failed {
                                    cx.theme().danger
                                } else {
                                    cx.theme().foreground
                                })
                                .child(outcome_label(e)),
                        )
                        .child(
                            div()
                                .w(px(80.))
                                .flex_none()
                                .text_color(muted)
                                .child(relative_time(now, e.at)),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open(entry.clone(), window, cx)
                        }))
                });
                body = body.child(
                    div()
                        .id("history-list")
                        .max_h(px(420.))
                        .overflow_y_scroll()
                        .children(rows),
                );
            }
        }
        body = body.child(
            Button::new("history-close")
                .label("Close")
                .on_click(cx.listener(|this, _, window, cx| this.dismiss(window, cx))),
        );
        div()
            .id("history-panel")
            .track_focus(&self.focus)
            .w(px(640.))
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
    use wisp_core::ErrorKind;

    fn entry(outcome: Outcome, status: Option<u16>) -> HistoryEntry {
        HistoryEntry {
            id: "e1".into(),
            at: 1_000,
            request: None,
            method: Method::Get,
            url: "{{base}}/x".into(),
            outcome,
            status,
            ttfb_ms: Some(5),
            total_ms: Some(12),
            size: Some(3),
            body_stored: true,
            body_truncated: false,
            redacted: Vec::new(),
        }
    }

    #[test]
    fn row_label_for_each_outcome() {
        assert_eq!(outcome_label(&entry(Outcome::Completed, Some(404))), "404");
        assert_eq!(outcome_label(&entry(Outcome::Completed, None)), "Completed");
        assert_eq!(
            outcome_label(&entry(
                Outcome::Failed {
                    kind: ErrorKind::Dns
                },
                None
            )),
            "Failed: Dns"
        );
        assert_eq!(outcome_label(&entry(Outcome::Cancelled, None)), "Cancelled");
    }

    #[test]
    fn relative_time_buckets() {
        let at = 1_000_000;
        assert_eq!(relative_time(at + 3_000, at), "just now");
        assert_eq!(relative_time(at + 42_000, at), "42 s ago");
        assert_eq!(relative_time(at + 5 * 60_000, at), "5 min ago");
        assert_eq!(relative_time(at + 3 * 3_600_000, at), "3 h ago");
        assert_eq!(relative_time(at + 2 * 86_400_000, at), "2 d ago");
        // A clock that went backwards never underflows.
        assert_eq!(relative_time(at - 5_000, at), "just now");
    }

    #[test]
    fn resend_notice_with_and_without_names_and_environment() {
        assert_eq!(
            resend_notice(Some("dev"), &[], &[], false, false),
            "Resent with the current environment (dev)"
        );
        assert_eq!(
            resend_notice(None, &[], &[], false, false),
            "Resent with the current environment (no environment)"
        );
        assert_eq!(
            resend_notice(
                Some("dev"),
                &[],
                &["Authorization".into(), "token".into()],
                false,
                false
            ),
            "Resent with the current environment (dev); redacted values were not stored: Authorization, token"
        );
    }

    #[test]
    fn resend_notice_says_when_the_original_request_is_gone() {
        assert_eq!(
            resend_notice(None, &[], &[], true, false),
            "Resent with the current environment (no environment); the original request was not found, so only collection defaults applied"
        );
    }

    #[test]
    fn resend_notice_names_adjustments_and_an_edited_url() {
        assert_eq!(
            resend_notice(
                Some("dev"),
                &[
                    "disabled header Authorization".into(),
                    "reset auth to inherit".into()
                ],
                &[],
                false,
                true
            ),
            "Resent with the current environment (dev); because values were not stored: disabled header Authorization, reset auth to inherit; the URL was edited, so only collection defaults applied, not the original folder's"
        );
    }

    #[test]
    fn origin_line_notes_truncation_and_missing_body() {
        let mut o = HistoryOrigin {
            at_ms: 0,
            body_stored: true,
            body_truncated: false,
            redacted: Vec::new(),
            adjustments: Vec::new(),
            request: None,
            url: String::new(),
        };
        assert_eq!(origin_line(&o, 5 * 60_000), "From history, 5 min ago");
        o.body_truncated = true;
        assert_eq!(
            origin_line(&o, 5 * 60_000),
            "From history, 5 min ago. Body truncated at 1 MiB"
        );
        o.body_truncated = false;
        o.body_stored = false;
        assert_eq!(
            origin_line(&o, 5 * 60_000),
            "From history, 5 min ago. Body not stored"
        );
    }

    #[test]
    fn stored_response_maps_outcome_and_timings() {
        let r = stored_response(&entry(Outcome::Completed, Some(200)), Vec::new(), None);
        assert_eq!(r.phase, Phase::Done);
        assert_eq!(r.status, Some(200));
        assert_eq!(r.total, Some(Duration::from_millis(12)));
        assert!(r.request_id.is_none());
        let f = stored_response(
            &entry(
                Outcome::Failed {
                    kind: ErrorKind::Tls,
                },
                None,
            ),
            Vec::new(),
            None,
        );
        assert_eq!(f.phase, Phase::Failed);
        assert_eq!(f.error.map(|e| e.kind), Some(ErrorKind::Tls));
        let c = stored_response(&entry(Outcome::Cancelled, None), Vec::new(), None);
        assert_eq!(c.phase, Phase::Cancelled);
    }

    #[test]
    fn body_setting_off_drops_the_body() {
        assert_eq!(body_to_store(true, Some(vec![1])), Some(vec![1]));
        assert_eq!(body_to_store(false, Some(vec![1])), None);
        assert_eq!(body_to_store(true, None), None);
    }

    #[test]
    fn append_without_body_stores_none() {
        use wisp_storage::collection::CollectionId;
        use wisp_storage::data_dir::DataDir;
        use wisp_storage::history::{HistoryStore, NewEntry};
        let dir = std::env::temp_dir().join(format!("wisp-t97-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let id = CollectionId::parse("abcdef0123456789").unwrap();
        let store = HistoryStore::open(&DataDir::at(&dir), &id);
        let new = NewEntry {
            at: SystemTime::now(),
            request_path: None,
            request: crate::draft::blank_request(),
            outcome: Outcome::Completed,
            status: Some(200),
            ttfb: None,
            total: None,
            size: Some(2),
            response_headers: Vec::new(),
        };
        let stored = store
            .append(new, body_to_store(false, Some(b"hi".to_vec())).as_deref())
            .unwrap();
        assert!(!stored.body_stored);
        assert!(store.load(&stored.id).unwrap().body.is_empty());
        let bins = std::fs::read_dir(DataDir::at(&dir).history_dir(&id))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "bin"))
            .count();
        assert_eq!(bins, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
