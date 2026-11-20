//! The comparison of two history entries, shown inside the History panel.
//!
//! Loading both entries and the ignore list, computing the comparison and saving
//! patterns all run on the background executor; every result carries a generation
//! so a stale one never overwrites a newer one. There is no copy or export action.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use gpui::{
    Context, InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, div, px,
};
use gpui_component::button::Button;
use gpui_component::{ActiveTheme as _, v_flex};
use wisp_service::compare::{
    BodyChange, BodyDiff, ChangeKind, Comparison, HeaderChange, IgnoreList, IgnoreProblem, Shown,
    Side, SideReason, StatusDiff, compare,
};
use wisp_storage::collection::{load_compare_ignore, save_compare_ignore};
use wisp_storage::history::{HistoryEntry, HistoryStore, StoredEntry};

use crate::ui::history::{
    checks_chip, delivery_chip, method_text, now_ms, outcome_label, relative_time,
};

/// The index of the nearest older entry with the same request path. `entries` is
/// newest first, as `HistoryStore::list` returns it.
pub fn previous_entry(entries: &[HistoryEntry], ix: usize) -> Option<usize> {
    let path = entries.get(ix)?.request.as_ref()?;
    (ix + 1..entries.len()).find(|&j| entries[j].request.as_ref() == Some(path))
}

/// The line for one side's body evidence, in the spec's words.
pub fn evidence_note(reason: &SideReason) -> String {
    let mut s = reason.kind.text().to_string();
    if reason.before_0_5_0 {
        s.push_str(" (recorded before 0.5.0)");
    }
    s
}

pub fn header_pattern(name: &str) -> String {
    format!("header:{name}")
}

pub fn body_pattern(pointer: &str) -> String {
    format!("body:{pointer}")
}

fn is_bidi(c: char) -> bool {
    matches!(c, '\u{61C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Escape control and bidi characters so a stored value cannot disguise the view.
pub fn escape_display(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_control() || is_bidi(c) {
            out.push_str(&format!("\\u{{{:x}}}", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}

fn shown_text(s: &Shown) -> String {
    let mut t = escape_display(&s.text);
    if s.cut {
        t.push('…');
    }
    t
}

fn shown_list(v: &[Shown]) -> String {
    v.iter().map(shown_text).collect::<Vec<_>>().join(", ")
}

fn opt_shown(s: &Option<Shown>) -> String {
    s.as_ref().map_or_else(|| "(none)".into(), shown_text)
}

/// Both entries as loaded, kept so the comparison can be recomputed.
struct Data {
    a: HistoryEntry,
    b: HistoryEntry,
    a_stored: Option<StoredEntry>,
    b_stored: Option<StoredEntry>,
    a_bin: bool,
    b_bin: bool,
}

struct Computed {
    comparison: Comparison,
    problems: Vec<IgnoreProblem>,
}

fn compute(data: &Data, patterns: &[String]) -> Computed {
    let (list, problems) = IgnoreList::parse(patterns);
    let a = Side::from_entry(&data.a, data.a_stored.as_ref(), data.a_bin);
    let b = Side::from_entry(&data.b, data.b_stored.as_ref(), data.b_bin);
    Computed {
        comparison: compare(&a, &b, &list),
        problems,
    }
}

/// Blocking: the stored bodies are needed, so `load` and not `load_without_body`.
fn load_data(store: &HistoryStore, a: HistoryEntry, b: HistoryEntry) -> Data {
    let a_stored = store.load(&a.id).ok();
    let b_stored = store.load(&b.id).ok();
    Data {
        a_bin: store.body_path(&a.id).is_some(),
        b_bin: store.body_path(&b.id).is_some(),
        a,
        b,
        a_stored,
        b_stored,
    }
}

enum State {
    Loading,
    Failed(String),
    Ready,
}

pub struct CompareView {
    store: HistoryStore,
    root: PathBuf,
    state: State,
    data: Option<Arc<Data>>,
    patterns: Vec<String>,
    computed: Option<Computed>,
    /// A pattern awaiting the user's confirmation, with the id of the row that offered it.
    pending: Option<(String, String)>,
    /// A save error, with the id of the row (or list entry) it belongs under.
    notice: Option<(String, String)>,
    generation: u64,
    /// Serialises read-modify-write saves of the ignore list; holds the last sequence number.
    save_lock: Arc<Mutex<u64>>,
    applied_seq: u64,
}

impl CompareView {
    pub fn new(
        store: HistoryStore,
        root: PathBuf,
        a: HistoryEntry,
        b: HistoryEntry,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            store,
            root,
            state: State::Loading,
            data: None,
            patterns: Vec::new(),
            computed: None,
            pending: None,
            notice: None,
            generation: 0,
            save_lock: Arc::new(Mutex::new(0)),
            applied_seq: 0,
        };
        this.generation += 1;
        let generation = this.generation;
        let store = this.store.clone();
        let root = this.root.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let patterns = load_compare_ignore(&root).map_err(|e| e.to_string())?;
                    let data = load_data(&store, a, b);
                    let computed = compute(&data, &patterns);
                    Ok::<_, String>((Arc::new(data), patterns, computed))
                })
                .await;
            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match result {
                    Ok((data, patterns, computed)) => {
                        this.data = Some(data);
                        this.patterns = patterns;
                        this.computed = Some(computed);
                        this.state = State::Ready;
                    }
                    Err(e) => this.state = State::Failed(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        this
    }

    /// Re-read the saved list, apply `change` and save it, then recompute, off the UI thread.
    fn apply(&mut self, change: Change, anchor: String, cx: &mut Context<Self>) {
        let Some(data) = self.data.clone() else {
            return;
        };
        let root = self.root.clone();
        let lock = self.save_lock.clone();
        self.pending = None;
        self.notice = None;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let (seq, patterns) = save_change(&root, &lock, &change)?;
                    let computed = compute(&data, &patterns);
                    Ok::<_, String>((seq, patterns, computed))
                })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok((seq, patterns, computed)) => {
                        // Saves finish in any order; only the newest list is shown.
                        if let Some(seq) = newer_seq(this.applied_seq, seq) {
                            this.applied_seq = seq;
                            this.patterns = patterns;
                            this.computed = Some(computed);
                        }
                    }
                    Err(e) => {
                        this.notice = Some((anchor, format!("Could not save the ignore list: {e}")))
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn confirm_pending(&mut self, cx: &mut Context<Self>) {
        if let Some((row, p)) = self.pending.take() {
            // Never persist a pattern the parser rejects.
            let (_, problems) = IgnoreList::parse(std::slice::from_ref(&p));
            if let Some(pr) = problems.first() {
                self.notice = Some((row, format!("Not saved: invalid pattern ({})", pr.reason)));
                cx.notify();
                return;
            }
            self.apply(Change::Add(p), row, cx);
        }
    }

    fn remove_pattern(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(p) = self.patterns.get(ix).cloned() {
            self.apply(Change::Remove(p), format!("pattern-{ix}"), cx);
        }
    }

    fn confirm_box(&self, p: &str, cx: &mut Context<Self>) -> gpui::AnyElement {
        v_flex()
            .gap_1()
            .p_2()
            .border_1()
            .border_color(cx.theme().border)
            .child(format!(
                "Ignore `{}`? This pattern is written to this collection's collection.toml, which may be shared through git.",
                escape_display(p)
            ))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        Button::new("ignore-confirm")
                            .label("Confirm")
                            .on_click(cx.listener(|this, _, _, cx| this.confirm_pending(cx))),
                    )
                    .child(Button::new("ignore-cancel").label("Cancel").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.pending = None;
                            cx.notify();
                        }),
                    )),
            )
            .into_any_element()
    }

    fn ignore_row(
        &self,
        id: SharedString,
        pattern: String,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let row = id.to_string();
        Button::new(id)
            .label("Ignore")
            .on_click(cx.listener(move |this, _, _, cx| {
                this.pending = Some((row.clone(), pattern.clone()));
                cx.notify();
            }))
            .into_any_element()
    }

    /// Text of the status row, for tests.
    #[cfg(test)]
    pub fn status_text(&self) -> Option<String> {
        self.computed
            .as_ref()
            .map(|c| status_text(&c.comparison.status))
    }
}

/// `seq` if it is newer than the last applied save, so an older save never replaces a newer one.
fn newer_seq(applied: u64, seq: u64) -> Option<u64> {
    (seq > applied).then_some(seq)
}

/// A change to the ignore list, applied to the list as saved at that moment.
enum Change {
    Add(String),
    Remove(String),
}

/// Re-read the saved list, apply `change` by value, and save. Returns the save's
/// sequence number and the list written.
fn save_change(
    root: &std::path::Path,
    lock: &Mutex<u64>,
    change: &Change,
) -> Result<(u64, Vec<String>), String> {
    let mut seq = lock.lock().unwrap_or_else(|e| e.into_inner());
    let mut list = load_compare_ignore(root).map_err(|e| e.to_string())?;
    match change {
        Change::Add(p) => {
            if !list.contains(p) {
                list.push(p.clone());
            }
        }
        Change::Remove(p) => list.retain(|x| x != p),
    }
    save_compare_ignore(root, &list).map_err(|e| e.to_string())?;
    *seq += 1;
    Ok((*seq, list))
}

fn status_text(s: &StatusDiff) -> String {
    let opt = |v: &Option<u16>| v.map_or_else(|| "none".to_string(), |s| s.to_string());
    match s {
        StatusDiff::Equal(c) => format!("Status: {c} (equal)"),
        StatusDiff::Changed { a, b } => format!("Status: {a} changed to {b}"),
        StatusDiff::Missing { a, b } => {
            format!("Status: missing on one side (A {}, B {})", opt(a), opt(b))
        }
    }
}

fn side_header(label: &str, e: &HistoryEntry, now: u64) -> String {
    let mut line = format!(
        "{label}: {}, {}, {} {}",
        relative_time(now, e.at),
        escape_display(e.request.as_deref().unwrap_or("(no request path)")),
        method_text(e.method),
        escape_display(&e.url)
    );
    line.push_str(&format!(
        ", env {}",
        escape_display(e.environment.as_deref().unwrap_or("none"))
    ));
    line.push_str(&format!(", {}", outcome_label(e)));
    if let Some(c) = delivery_chip(e.delivery) {
        line.push_str(&format!(", {c}"));
    }
    if let Some(c) = checks_chip(&e.assertions) {
        line.push_str(&format!(", {c}"));
    }
    line
}

fn body_change_text(c: &BodyChange) -> String {
    let kind = match c.kind {
        ChangeKind::Added => "added",
        ChangeKind::Removed => "removed",
        ChangeKind::Changed => "changed",
        ChangeKind::TypeChanged => "type changed",
    };
    format!(
        "{} ({kind}): A {} / B {}",
        escape_display(&c.path),
        opt_shown(&c.before),
        opt_shown(&c.after)
    )
}

/// One line of the comparison report, in display order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    Section(String),
    Text(String),
    Note(String),
    /// A change with an "ignore" action for `pattern`.
    Ignorable {
        id: String,
        text: String,
        pattern: String,
    },
}

/// The report's lines, from the status section to the hidden-by-ignore count.
fn report_rows(cmp: &Comparison) -> Vec<Row> {
    let mut rows = vec![
        Row::Section("Status".into()),
        Row::Text(status_text(&cmp.status)),
        Row::Section("Headers".into()),
    ];
    let h = &cmp.headers;
    if h.a_not_recorded {
        rows.push(Row::Text("Headers not recorded for A".into()));
    }
    if h.b_not_recorded {
        rows.push(Row::Text("Headers not recorded for B".into()));
    }
    for (i, ch) in h.changes.iter().enumerate() {
        let (name, text) = match ch {
            HeaderChange::Added { name, values } => {
                (name, format!("added: {}", shown_list(values)))
            }
            HeaderChange::Removed { name, values } => {
                (name, format!("removed: {}", shown_list(values)))
            }
            HeaderChange::Changed { name, a, b } => (
                name,
                format!("changed: A {} / B {}", shown_list(a), shown_list(b)),
            ),
        };
        rows.push(Row::Ignorable {
            id: format!("ignore-header-{i}"),
            text: format!("{}: {text}", escape_display(name)),
            pattern: header_pattern(name),
        });
    }
    if !h.redacted.is_empty() {
        rows.push(Row::Note(format!(
            "Redacted, not compared: {}",
            h.redacted
                .iter()
                .map(|n| escape_display(n))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    if h.changes.is_empty() && !h.a_not_recorded && !h.b_not_recorded {
        rows.push(Row::Note("No header changes".into()));
    }
    rows.push(Row::Section("Body".into()));
    let mut hidden = h.ignored;
    match &cmp.body {
        BodyDiff::Equal { ignored } => {
            hidden += ignored;
            rows.push(Row::Text("Bodies are equal".into()));
        }
        BodyDiff::Different {
            changes,
            total,
            ignored,
        } => {
            hidden += ignored;
            for (i, ch) in changes.iter().enumerate() {
                if ch.path.is_empty() {
                    // The whole body changed; the parser has no root pattern to ignore it with.
                    rows.push(Row::Text(body_change_text(ch)));
                    continue;
                }
                rows.push(Row::Ignorable {
                    id: format!("ignore-body-{i}"),
                    text: body_change_text(ch),
                    pattern: body_pattern(&ch.path),
                });
            }
            if *total > changes.len() {
                rows.push(Row::Note(format!("and {} more", total - changes.len())));
            }
        }
        BodyDiff::NotJson { equal } => rows.push(Row::Text(format!(
            "Bodies are not JSON; text diff not available ({})",
            if *equal { "equal" } else { "different" }
        ))),
        BodyDiff::Incomplete { a, b } => {
            for (label, r) in [("A", a), ("B", b)] {
                if let Some(r) = r {
                    rows.push(Row::Text(format!("{label}: {}", evidence_note(r))));
                }
            }
        }
        BodyDiff::TooDeep => rows.push(Row::Text("Too deeply nested to compare".into())),
    }
    rows.push(Row::Section(format!("Hidden by the ignore list: {hidden}")));
    rows
}

fn scroll(col: gpui::Div) -> gpui::Stateful<gpui::Div> {
    div()
        .max_h(px(480.))
        .id("compare-scroll")
        .overflow_y_scroll()
        .child(col)
}

impl Render for CompareView {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let mut col = v_flex().gap_2().child(div().text_lg().child("Compare"));
        match &self.state {
            State::Loading => {
                return scroll(col.child(div().text_color(muted).child("Loading...")));
            }
            State::Failed(e) => {
                return scroll(
                    col.child(
                        div()
                            .text_color(cx.theme().danger)
                            .child(format!("Could not compare: {e}")),
                    ),
                );
            }
            State::Ready => {}
        }
        let (Some(data), Some(c)) = (self.data.clone(), self.computed.as_ref()) else {
            return scroll(col);
        };
        let now = now_ms();
        col = col
            .child(div().child(side_header("A", &data.a, now)))
            .child(div().child(side_header("B", &data.b, now)));
        let mut notice_shown = false;
        for row in report_rows(&c.comparison) {
            col = match row {
                Row::Section(t) => col.child(div().text_color(muted).child(SharedString::from(t))),
                Row::Text(t) => col.child(div().child(t)),
                Row::Note(t) => col.child(div().text_color(muted).child(t)),
                Row::Ignorable { id, text, pattern } => {
                    let ignore = self.ignore_row(SharedString::from(id.clone()), pattern, cx);
                    col = col.child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .child(div().flex_1().min_w_0().child(text))
                            .child(ignore),
                    );
                    // The confirmation and any save error sit directly under the clicked row.
                    if let Some((_, p)) = self.pending.clone().filter(|(r, _)| *r == id) {
                        col = col.child(self.confirm_box(&p, cx));
                    }
                    if let Some(n) = self.notice.as_ref().filter(|(r, _)| *r == id) {
                        notice_shown = true;
                        col = col.child(div().text_color(cx.theme().danger).child(n.1.clone()));
                    }
                    col
                }
            };
        }

        // The current patterns.
        col = col.child(div().text_color(muted).child("Ignore list"));
        if self.patterns.is_empty() {
            col = col.child(div().text_color(muted).child("No patterns"));
        }
        for (i, p) in self.patterns.iter().enumerate() {
            let reason = c
                .problems
                .iter()
                .find(|pr| pr.index == i)
                .map(|pr| format!(" (invalid: {})", pr.reason))
                .unwrap_or_default();
            col =
                col.child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(format!("{}{reason}", escape_display(p))),
                        )
                        .child(Button::new(("ignore-remove", i)).label("Remove").on_click(
                            cx.listener(move |this, _, _, cx| this.remove_pattern(i, cx)),
                        )),
                );
            if let Some(n) = self
                .notice
                .as_ref()
                .filter(|(r, _)| *r == format!("pattern-{i}"))
            {
                notice_shown = true;
                col = col.child(div().text_color(cx.theme().danger).child(n.1.clone()));
            }
        }
        if !notice_shown && let Some(n) = &self.notice {
            col = col.child(div().text_color(cx.theme().danger).child(n.1.clone()));
        }
        scroll(col)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_service::compare::BodyKind;
    use wisp_storage::history::{HistoryEntry, Outcome};

    fn entry(id: &str, path: Option<&str>) -> HistoryEntry {
        HistoryEntry {
            id: id.into(),
            at: 1,
            request: path.map(str::to_string),
            method: wisp_core::Method::Get,
            url: "{{base}}/x".into(),
            outcome: Outcome::Completed,
            status: Some(200),
            ttfb_ms: None,
            total_ms: None,
            dns_ms: None,
            connect_ms: None,
            size: None,
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
    fn compare_view_previous_is_same_path_and_older() {
        let v = vec![
            entry("4", Some("a.toml")),
            entry("3", Some("b.toml")),
            entry("2", Some("a.toml")),
            entry("1", Some("b.toml")),
        ];
        assert_eq!(previous_entry(&v, 0), Some(2));
        assert_eq!(previous_entry(&v, 1), Some(3));
    }

    #[test]
    fn compare_view_previous_skips_other_paths_and_none_at_oldest() {
        let v = vec![
            entry("3", Some("a.toml")),
            entry("2", Some("b.toml")),
            entry("1", Some("c.toml")),
        ];
        assert_eq!(previous_entry(&v, 0), None);
        assert_eq!(previous_entry(&v, 2), None);
        let v = vec![entry("2", None), entry("1", None)];
        assert_eq!(previous_entry(&v, 0), None);
    }

    #[test]
    fn compare_view_evidence_notes_use_the_specs_words() {
        let r = |kind, before| SideReason {
            kind,
            before_0_5_0: before,
        };
        assert_eq!(
            evidence_note(&r(BodyKind::Off, false)),
            "body not stored (history bodies off, or a `wisp-cli run` entry)"
        );
        assert_eq!(
            evidence_note(&r(BodyKind::Unreadable, false)),
            "body could not be read when recorded"
        );
        assert_eq!(
            evidence_note(&r(BodyKind::NoResponse, false)),
            "no response body (request failed or was cancelled)"
        );
        assert_eq!(
            evidence_note(&r(BodyKind::Truncated, false)),
            "body cut at 1 MiB; not compared"
        );
        assert!(evidence_note(&r(BodyKind::Off, true)).ends_with("(recorded before 0.5.0)"));
    }

    #[test]
    fn compare_view_save_rereads_the_file_and_appends() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("collection.toml"),
            "format = 1\n[compare]\nignore = [\"header:date\"]\n",
        )
        .unwrap();
        let lock = Mutex::new(0);
        // Changed outside the view after it loaded.
        save_compare_ignore(root, &["header:date".into(), "body:/meta".into()]).unwrap();
        let (_, list) = save_change(root, &lock, &Change::Add("header:etag".into())).unwrap();
        assert_eq!(list, ["header:date", "body:/meta", "header:etag"]);
        let (_, list) = save_change(root, &lock, &Change::Add("header:etag".into())).unwrap();
        assert_eq!(list.len(), 3);
        let (seq, list) = save_change(root, &lock, &Change::Remove("header:date".into())).unwrap();
        assert_eq!(seq, 3);
        assert_eq!(list, ["body:/meta", "header:etag"]);
        assert_eq!(load_compare_ignore(root).unwrap(), list);
    }

    #[test]
    fn compare_view_header_pattern() {
        assert_eq!(header_pattern("Date"), "header:Date");
    }

    #[test]
    fn compare_view_body_pattern() {
        assert_eq!(body_pattern("/meta/requestId"), "body:/meta/requestId");
    }

    #[test]
    fn compare_view_escapes_bidi_and_control() {
        assert_eq!(escape_display("a\u{202E}b"), "a\\u{202e}b");
        assert_eq!(escape_display("x\ny"), "x\\u{a}y");
        assert_eq!(escape_display("plain é"), "plain é");
    }

    #[test]
    fn compare_view_cut_values_are_marked() {
        let s = Shown {
            text: "ab".into(),
            cut: true,
        };
        assert_eq!(shown_text(&s), "ab…");
    }

    fn sh(t: &str) -> Shown {
        Shown {
            text: t.into(),
            cut: false,
        }
    }

    fn cmp(headers: wisp_service::compare::HeadersDiff, body: BodyDiff) -> Comparison {
        Comparison {
            status: StatusDiff::Equal(200),
            headers,
            body,
        }
    }

    fn no_headers() -> wisp_service::compare::HeadersDiff {
        wisp_service::compare::HeadersDiff {
            a_not_recorded: false,
            b_not_recorded: false,
            changes: Vec::new(),
            redacted: Vec::new(),
            ignored: 0,
        }
    }

    fn texts(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                Row::Section(t) | Row::Text(t) | Row::Note(t) => t.clone(),
                Row::Ignorable { text, .. } => text.clone(),
            })
            .collect()
    }

    #[test]
    fn compare_view_report_orders_sections_and_lists_header_changes() {
        let mut h = no_headers();
        h.changes = vec![
            HeaderChange::Added {
                name: "x-new".into(),
                values: vec![sh("1")],
            },
            HeaderChange::Removed {
                name: "x-old".into(),
                values: vec![sh("2")],
            },
            HeaderChange::Changed {
                name: "x-chg".into(),
                a: vec![sh("a")],
                b: vec![sh("b")],
            },
        ];
        h.redacted = vec!["authorization".into()];
        h.ignored = 2;
        let rows = report_rows(&cmp(h, BodyDiff::Equal { ignored: 3 }));
        assert_eq!(
            texts(&rows),
            [
                "Status",
                "Status: 200 (equal)",
                "Headers",
                "x-new: added: 1",
                "x-old: removed: 2",
                "x-chg: changed: A a / B b",
                "Redacted, not compared: authorization",
                "Body",
                "Bodies are equal",
                "Hidden by the ignore list: 5",
            ]
        );
        assert!(matches!(&rows[3], Row::Ignorable { pattern, .. } if pattern == "header:x-new"));
    }

    #[test]
    fn compare_view_report_notes_unrecorded_headers() {
        let mut h = no_headers();
        h.a_not_recorded = true;
        let t = texts(&report_rows(&cmp(h, BodyDiff::TooDeep)));
        assert!(t.contains(&"Headers not recorded for A".to_string()));
        assert!(!t.contains(&"Headers not recorded for B".to_string()));
        assert!(!t.contains(&"No header changes".to_string()));
        assert!(t.contains(&"Too deeply nested to compare".to_string()));
        let t = texts(&report_rows(&cmp(no_headers(), BodyDiff::TooDeep)));
        assert!(t.contains(&"No header changes".to_string()));
    }

    #[test]
    fn compare_view_report_body_cases() {
        let t = texts(&report_rows(&cmp(
            no_headers(),
            BodyDiff::NotJson { equal: false },
        )));
        assert!(t.contains(&"Bodies are not JSON; text diff not available (different)".into()));
        let change = |p: &str| BodyChange {
            path: p.into(),
            kind: ChangeKind::Changed,
            before: Some(sh("1")),
            after: Some(sh("2")),
        };
        let rows = report_rows(&cmp(
            no_headers(),
            BodyDiff::Different {
                changes: vec![change("/a"), change("/b")],
                total: 5,
                ignored: 1,
            },
        ));
        let t = texts(&rows);
        assert!(t.contains(&"/a (changed): A 1 / B 2".to_string()));
        assert!(t.contains(&"and 3 more".to_string()));
        assert_eq!(t.last().unwrap(), "Hidden by the ignore list: 1");
        assert!(
            rows.iter()
                .any(|r| matches!(r, Row::Ignorable { pattern, .. } if pattern == "body:/b"))
        );
    }

    #[test]
    fn compare_view_root_body_change_offers_no_ignore() {
        let change = |p: &str| BodyChange {
            path: p.into(),
            kind: ChangeKind::Changed,
            before: Some(sh("1")),
            after: Some(sh("2")),
        };
        let rows = report_rows(&cmp(
            no_headers(),
            BodyDiff::Different {
                changes: vec![change(""), change("/a")],
                total: 2,
                ignored: 0,
            },
        ));
        let pats: Vec<_> = rows
            .iter()
            .filter_map(|r| match r {
                Row::Ignorable { pattern, .. } => Some(pattern.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(pats, vec!["body:/a".to_string()]);
        // Every offered pattern parses.
        let (_, problems) = IgnoreList::parse(&pats);
        assert!(problems.is_empty());
        // The rule the old code offered is rejected by the parser.
        let (_, problems) = IgnoreList::parse(&["body:".to_string()]);
        assert!(!problems.is_empty());
    }

    #[test]
    fn compare_view_older_save_never_replaces_a_newer_one() {
        let mut applied = 0;
        for seq in [2, 1] {
            if let Some(s) = newer_seq(applied, seq) {
                applied = s;
            }
        }
        assert_eq!(applied, 2);
        assert_eq!(newer_seq(2, 2), None);
        assert_eq!(newer_seq(2, 3), Some(3));
    }

    #[test]
    fn compare_view_incomplete_body_shows_evidence_notes_not_equal() {
        let side = |kind| SideReason {
            kind,
            before_0_5_0: false,
        };
        let a = side(BodyKind::StoredUnavailable);
        let b = side(BodyKind::Truncated);
        let rows = report_rows(&cmp(
            no_headers(),
            BodyDiff::Incomplete {
                a: Some(a),
                b: Some(b),
            },
        ));
        let t = texts(&rows);
        let body = t.iter().position(|s| s == "Body").unwrap();
        assert_eq!(t[body + 1], format!("A: {}", evidence_note(&a)));
        assert_eq!(t[body + 2], format!("B: {}", evidence_note(&b)));
        assert!(t[body + 3].starts_with("Hidden by the ignore list"));
        assert!(!t[body..].iter().any(|s| s.contains("equal")));
    }

    #[gpui::test]
    fn compare_view_opens_a_pair_and_shows_status(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;
        use std::time::SystemTime;
        use wisp_storage::collection::CollectionId;
        use wisp_storage::data_dir::DataDir;
        use wisp_storage::history::NewEntry;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("coll");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("collection.toml"), "format = 1\n").unwrap();
        let id = CollectionId::parse("abcdef0123456789").unwrap();
        let store = HistoryStore::open(&DataDir::at(dir.path()), &id);
        let add = |status: u16| {
            store
                .append(
                    NewEntry {
                        at: SystemTime::now(),
                        request_path: Some("a.toml".into()),
                        request: crate::draft::blank_request(),
                        outcome: Outcome::Completed,
                        status: Some(status),
                        ttfb: None,
                        total: None,
                        dns: None,
                        connect: None,
                        size: Some(2),
                        response_headers: Vec::new(),
                        environment: None,
                        assertions: Vec::new(),
                        delivery: None,
                    },
                    Some(b"{}"),
                )
                .unwrap()
        };
        let (a, b) = (add(200), add(500));
        let view = cx.new(|cx| CompareView::new(store.clone(), root, a, b, cx));
        cx.run_until_parked();
        let text = view.read_with(cx, |v, _| v.status_text());
        assert_eq!(text.as_deref(), Some("Status: 200 changed to 500"));

        // An invalid pattern is never persisted: the save is refused with a notice at its row.
        view.update(cx, |v, cx| {
            v.pending = Some(("ignore-body-0".into(), "body:".into()));
            v.confirm_pending(cx);
        });
        cx.run_until_parked();
        view.read_with(cx, |v, _| {
            assert!(v.pending.is_none());
            assert!(matches!(&v.notice, Some((r, _)) if r == "ignore-body-0"));
        });
        let saved = std::fs::read_to_string(dir.path().join("coll/collection.toml")).unwrap();
        assert!(!saved.contains("body:"), "{saved}");
    }
}
