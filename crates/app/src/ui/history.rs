//! The history panel: the open collection's sent requests, newest first.
//!
//! Listing, loading and clearing run on the background executor (see the
//! workspace methods); the pure helpers at the top are what the unit tests
//! cover. Redacted values are never shown or restored: the store keeps them
//! empty and a resend resolves against the current environment.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{
    AppContext as _, Context, DismissEvent, Entity, EventEmitter, FocusHandle,
    InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div, px,
};
use gpui_component::button::Button;
use gpui_component::{ActiveTheme as _, v_flex};
use wisp_core::body::BodyHandle;
use wisp_core::{EngineError, Method};
use wisp_storage::history::{AssertionRecord, AssertionState, Delivery, HistoryEntry, Outcome};

use crate::actions::CancelRequest;
use crate::tab::{HistoryOrigin, Phase, ResponseState};
use crate::ui::compare::{CompareView, previous_entry};
use crate::workspace::{Workspace, WorkspaceEvent};

/// The body handed to the history append: none when the setting is off.
#[cfg(test)]
pub fn body_to_store(enabled: bool, body: Option<Vec<u8>>) -> Option<Vec<u8>> {
    if enabled { body } else { None }
}

pub(super) fn method_text(m: Method) -> String {
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
    had_assertions: bool,
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
    if had_assertions {
        text.push_str("; assertions are not resent");
    }
    text
}

/// `"<passed>/<total> checks"` for an entry's records, or `None` when it has none.
pub fn checks_chip(records: &[AssertionRecord]) -> Option<String> {
    if records.is_empty() {
        return None;
    }
    let passed = records
        .iter()
        .filter(|r| r.state == AssertionState::Pass)
        .count();
    Some(format!("{passed}/{} checks", records.len()))
}

/// The chip text for an entry whose delivery is not known to have succeeded or failed.
pub fn delivery_chip(delivery: Option<Delivery>) -> Option<&'static str> {
    (delivery == Some(Delivery::Unknown)).then_some("outcome unknown")
}

/// Whether any record failed.
pub fn checks_failed(records: &[AssertionRecord]) -> bool {
    records.iter().any(|r| r.state == AssertionState::Fail)
}

/// The line above the response of a from-history tab.
pub fn origin_line(origin: &HistoryOrigin, now_ms: u64) -> String {
    let mut line = format!("From history, {}", relative_time(now_ms, origin.at_ms));
    if origin.body_truncated {
        line.push_str(". Body truncated at 1 MiB");
    } else if !origin.body_stored {
        line.push_str(". Body not stored");
    }
    if origin.request_truncated {
        line.push_str(", request body truncated at 1 MiB");
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
        dns: ms(entry.dns_ms),
        connect: ms(entry.connect_ms),
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
    /// Guards `start_compare` separately from `reload`.
    compare_generation: u64,
    /// The entry id marked "Compare from", and the collection root it belongs to.
    mark: Option<String>,
    marks_root: Option<std::path::PathBuf>,
    /// The comparison shown in place of the list.
    compare: Option<Entity<CompareView>>,
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
            compare_generation: 0,
            mark: None,
            marks_root: None,
            compare: None,
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
        let root = self.active_root(cx);
        if root != self.marks_root {
            self.mark = None;
            self.compare = None;
            self.compare_generation += 1;
            self.listing = Listing::Loading;
            self.marks_root = root;
        }
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

    fn active_root(&self, cx: &Context<Self>) -> Option<std::path::PathBuf> {
        let w = self.workspace.read(cx);
        w.active_collection_ix()
            .and_then(|ix| w.collection_root(ix))
    }

    /// Open the comparison of `a` (earlier) and `b`. The store is opened off the UI thread.
    fn start_compare(&mut self, a: HistoryEntry, b: HistoryEntry, cx: &mut Context<Self>) {
        let Some(root) = self.active_root(cx) else {
            return;
        };
        let Ok(job) = self.workspace.read(cx).history_job() else {
            return;
        };
        self.compare_generation += 1;
        let generation = self.compare_generation;
        cx.spawn(async move |this, cx| {
            let store = cx
                .background_executor()
                .spawn(async move { job.open_existing() })
                .await;
            this.update(cx, |this, cx| {
                if this.compare_generation != generation {
                    return;
                }
                if let Ok(Some(store)) = store {
                    this.compare = Some(cx.new(|cx| CompareView::new(store, root, a, b, cx)));
                    cx.notify();
                }
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
            Listing::Ready(_) if self.compare.is_some() => {
                if let Some(c) = &self.compare {
                    body = body.child(c.clone()).child(
                        Button::new("compare-back")
                            .label("Back to history")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.compare = None;
                                cx.notify();
                            })),
                    );
                }
            }
            Listing::Ready(entries) => {
                let rows = entries.iter().enumerate().map(|(ix, e)| {
                    let entry = e.clone();
                    let failed = !matches!(e.outcome, Outcome::Completed);
                    let hover = cx.theme().secondary;
                    let prev = previous_entry(entries, ix).map(|j| entries[j].clone());
                    let marked = self.mark.as_deref() == Some(e.id.as_str());
                    let from = self
                        .mark
                        .as_ref()
                        .filter(|_| !marked)
                        .and_then(|m| entries.iter().find(|x| &x.id == m).cloned());
                    let this_entry = e.clone();
                    let id = e.id.clone();
                    let actions = div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .px_2()
                        .children(prev.map(|p| {
                            let b = this_entry.clone();
                            Button::new(("compare-prev", ix))
                                .label("Compare with previous")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.start_compare(p.clone(), b.clone(), cx)
                                }))
                        }))
                        .child(
                            Button::new(("compare-from", ix))
                                .label(if marked { "Clear mark" } else { "Compare from" })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.mark = if marked { None } else { Some(id.clone()) };
                                    cx.notify();
                                })),
                        )
                        .children(from.map(|a| {
                            let b = this_entry.clone();
                            Button::new(("compare-to", ix))
                                .label("Compare to")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.start_compare(a.clone(), b.clone(), cx)
                                }))
                        }));
                    let row = div()
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
                        .children(e.environment.clone().map(|name| {
                            div()
                                .flex_none()
                                .text_color(muted)
                                .child(SharedString::from(name))
                        }))
                        .children(
                            delivery_chip(e.delivery)
                                .map(|chip| div().flex_none().text_color(muted).child(chip)),
                        )
                        .children(checks_chip(&e.assertions).map(|chip| {
                            div()
                                .flex_none()
                                .text_color(if checks_failed(&e.assertions) {
                                    cx.theme().danger
                                } else {
                                    muted
                                })
                                .child(chip)
                        }))
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
                        }));
                    v_flex().child(row).child(actions)
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
            dns_ms: Some(3),
            connect_ms: None,
            size: Some(3),
            body_stored: true,
            body_truncated: false,
            redacted: Vec::new(),
            environment: None,
            assertions: Vec::new(),
            delivery: None,
            evidence: None,
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
            resend_notice(Some("dev"), &[], &[], false, false, false),
            "Resent with the current environment (dev)"
        );
        assert_eq!(
            resend_notice(None, &[], &[], false, false, false),
            "Resent with the current environment (no environment)"
        );
        assert_eq!(
            resend_notice(
                Some("dev"),
                &[],
                &["Authorization".into(), "token".into()],
                false,
                false,
                false
            ),
            "Resent with the current environment (dev); redacted values were not stored: Authorization, token"
        );
    }

    #[test]
    fn resend_notice_says_assertions_are_not_resent() {
        assert_eq!(
            resend_notice(Some("dev"), &[], &[], false, false, true),
            "Resent with the current environment (dev); assertions are not resent"
        );
    }

    #[test]
    fn delivery_chip_shows_only_for_unknown() {
        assert_eq!(
            delivery_chip(Some(Delivery::Unknown)),
            Some("outcome unknown")
        );
        assert_eq!(delivery_chip(Some(Delivery::Responded)), None);
        assert_eq!(delivery_chip(Some(Delivery::NotSent)), None);
        assert_eq!(delivery_chip(None), None);
    }

    #[test]
    fn checks_chip_counts_passed_over_total() {
        let rec = |state| AssertionRecord {
            label: "status".into(),
            hash: "h".into(),
            state,
        };
        assert_eq!(checks_chip(&[]), None);
        let v = [
            rec(AssertionState::Pass),
            rec(AssertionState::Fail),
            rec(AssertionState::Skipped),
        ];
        assert_eq!(checks_chip(&v).as_deref(), Some("1/3 checks"));
        assert!(checks_failed(&v));
        assert!(!checks_failed(&v[..1]));
    }

    #[test]
    fn resend_notice_says_when_the_original_request_is_gone() {
        assert_eq!(
            resend_notice(None, &[], &[], true, false, false),
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
                true,
                false
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
            request_truncated: false,
            redacted: Vec::new(),
            adjustments: Vec::new(),
            request: None,
            url: String::new(),
            stored_body: Default::default(),
            had_assertions: false,
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
    fn truncated_request_is_noted_separately_from_response_body() {
        let mut o = HistoryOrigin {
            at_ms: 0,
            body_stored: true,
            body_truncated: true,
            request_truncated: true,
            redacted: Vec::new(),
            adjustments: Vec::new(),
            request: None,
            url: String::new(),
            stored_body: Default::default(),
            had_assertions: false,
        };
        let line = origin_line(&o, 5 * 60_000);
        assert!(line.contains("Body truncated at 1 MiB"));
        assert!(line.contains("request body truncated at 1 MiB"));
        o.body_truncated = false;
        assert!(!origin_line(&o, 0).contains("Body truncated at"));
    }

    #[test]
    fn stored_response_maps_outcome_and_timings() {
        let r = stored_response(&entry(Outcome::Completed, Some(200)), Vec::new(), None);
        assert_eq!(r.phase, Phase::Done);
        assert_eq!(r.status, Some(200));
        assert_eq!(r.total, Some(Duration::from_millis(12)));
        assert_eq!(r.dns, Some(Duration::from_millis(3)));
        assert_eq!(r.connect, None);
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
            dns: None,
            connect: None,
            size: Some(2),
            response_headers: Vec::new(),
            environment: None,
            assertions: Vec::new(),
            delivery: None,
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

    #[gpui::test]
    fn compare_view_root_change_clears_mark_rows_and_pending_compare(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::settings::Settings;
        use std::sync::Arc;
        use wisp_storage::data_dir::DataDir;
        use wisp_storage::vault::MemoryStore;

        cx.update(gpui_component::init);
        let dir = tempfile::tempdir().unwrap();
        let mk = |n: &str| {
            let r = dir.path().join(n);
            std::fs::create_dir_all(&r).unwrap();
            std::fs::write(r.join("collection.toml"), "format = 1\n").unwrap();
            r
        };
        let (x, y) = (mk("x"), mk("y"));
        let data = DataDir::at(dir.path().join("data"));
        let ws = cx.new(|cx| {
            Workspace::with_data_dir(
                Settings::default(),
                None,
                None,
                Arc::new(MemoryStore::new()),
                Some(data),
                cx,
            )
        });
        ws.update(cx, |w, cx| w.open_collection(&x, cx).unwrap());
        cx.run_until_parked();
        let window = cx.add_window(|window, cx| HistoryView::new(ws.clone(), window, cx));
        let view = window.root(cx).unwrap();
        cx.run_until_parked();
        view.update(cx, |v, _| {
            v.listing = Listing::Ready(vec![entry(Outcome::Completed, Some(200))])
        });
        view.update(cx, |v, _| v.mark = Some("e1".into()));
        let before = view.read_with(cx, |v, _| (v.generation, v.compare_generation));
        ws.update(cx, |w, cx| w.open_collection(&y, cx).unwrap());
        view.read_with(cx, |v, _| {
            assert!(
                matches!(v.listing, Listing::Loading),
                "X's rows must not stay under Y"
            );
            assert!(v.mark.is_none(), "the mark must not follow to Y");
            assert_eq!(v.compare_generation, before.1 + 1);
            assert_eq!(v.generation, before.0 + 1);
        });
        // A reload that does not change the root leaves the compare guard alone.
        ws.update(cx, |_, cx| cx.emit(WorkspaceEvent::HistoryChanged));
        view.read_with(cx, |v, _| assert_eq!(v.compare_generation, before.1 + 1));
    }
}
