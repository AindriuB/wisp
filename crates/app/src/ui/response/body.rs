//! Body sub-tab: Pretty|Raw toggle, paged raw viewer and Save.

use std::io::Write as _;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    ClipboardItem, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    KeyDownEvent, MouseButton, ParentElement, Render, ScrollStrategy, SharedString, Size, Styled,
    Subscription, Window, div, px, size,
};
use gpui_component::IconName;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::spinner::Spinner;
use gpui_component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, v_virtual_list,
};
use wisp_core::body::{BodyHandle, BodyId, BodyStore};

use crate::pager::{RAW_DISPLAY_CAP, Utf8Decoder, page};
use crate::pretty::{ContentKind, content_kind, pretty_json};
use crate::tab::{BodyView, Phase, RequestTab, TabEvent};
use crate::ui::response::{error_text, error_words};
use crate::workspace::{Workspace, WorkspaceEvent};

const LINE_HEIGHT: f32 = 18.0;
/// Longer lines are split so one element never holds megabytes of text.
const MAX_LINE: usize = 2000;
/// Load the next page when the visible range is this close to the end.
const PREFETCH_LINES: usize = 200;
const SNIFF: usize = 1024;

/// Pretty is offered only for JSON within the configured limit.
pub fn pretty_allowed(kind: Option<ContentKind>, size: u64, limit: u64) -> bool {
    kind == Some(ContentKind::Json) && size <= limit
}

/// Whether Pretty is the view on screen.
fn pretty_on_screen(view: BodyView, pretty_ok: bool, pretty: &Pretty) -> bool {
    view == BodyView::Pretty && pretty_ok && !matches!(pretty, Pretty::Failed(_))
}

/// Raw pages are prefetched whenever Raw is on screen and the visible range
/// is near the end of what is loaded, regardless of the Pretty state.
fn should_prefetch(
    has_body: bool,
    view: BodyView,
    pretty_ok: bool,
    pretty: &Pretty,
    range_end: usize,
    lines_len: usize,
) -> bool {
    has_body
        && !pretty_on_screen(view, pretty_ok, pretty)
        && range_end + PREFETCH_LINES >= lines_len
}

/// A jump requested from the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Jump {
    Top,
    End,
}

/// Maps a key press on the body list to a jump. End and Cmd/Ctrl+Down go to
/// the end, Home and Cmd/Ctrl+Up to the top (End and Home with any modifier);
/// anything else is not ours.
fn jump_for_key(key: &str, platform: bool) -> Option<Jump> {
    match (key, platform) {
        ("end", _) | ("down", true) => Some(Jump::End),
        ("home", _) | ("up", true) => Some(Jump::Top),
        _ => None,
    }
}

/// Whether a Raw or Pretty line list is on screen (not the placeholder,
/// the spinner or the binary notice), so the jump keys have something to move.
fn list_on_screen(has_body: bool, kind: Option<ContentKind>, pretty_working_shown: bool) -> bool {
    has_body && kind.is_some_and(|k| k != ContentKind::Binary) && !pretty_working_shown
}

/// Index of the last item of the list on screen: the Pretty lines when
/// Pretty is shown, else the raw lines plus the note and spare rows.
fn last_index(pretty_len: Option<usize>, raw_lines: usize) -> usize {
    pretty_len.map_or(raw_lines + 1, |n| n.saturating_sub(1))
}

/// A pending jump belongs to the view it started in; switching views drops it.
fn jump_still_valid(started_in_pretty: bool, pretty_now: bool) -> bool {
    started_in_pretty == pretty_now
}

/// What to do next for a pending jump to the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JumpStep {
    Nothing,
    LoadMore,
    Wait,
    /// Scroll to the bottom and clear the pending flag.
    ScrollToEnd,
}

fn jump_step(
    pending: bool,
    pretty_shown: bool,
    loading: bool,
    done: bool,
    capped: bool,
) -> JumpStep {
    if !pending {
        JumpStep::Nothing
    } else if pretty_shown || done || capped {
        JumpStep::ScrollToEnd
    } else if loading {
        JumpStep::Wait
    } else {
        JumpStep::LoadMore
    }
}

/// Appends `text` to `lines`, carrying an unterminated tail in `partial`.
pub fn push_text(lines: &mut Vec<SharedString>, partial: &mut String, text: &str) {
    partial.push_str(text);
    while let Some(i) = partial.find('\n') {
        push_line(lines, partial[..i].trim_end_matches('\r'));
        partial.drain(..=i);
    }
    while partial.len() > MAX_LINE {
        let cut = chunk_end(partial);
        lines.push(partial[..cut].to_string().into());
        partial.drain(..cut);
    }
}

fn push_line(lines: &mut Vec<SharedString>, mut line: &str) {
    while line.len() > MAX_LINE {
        let cut = chunk_end(line);
        lines.push(line[..cut].to_string().into());
        line = &line[cut..];
    }
    lines.push(line.to_string().into());
}

fn chunk_end(s: &str) -> usize {
    let mut cut = MAX_LINE;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    cut
}

fn split_lines(text: &str) -> Vec<SharedString> {
    let mut lines = Vec::new();
    let mut partial = String::new();
    push_text(&mut lines, &mut partial, text);
    if !partial.is_empty() {
        push_line(&mut lines, &partial);
    }
    lines
}

enum Pretty {
    Idle,
    Working,
    Ready(Vec<SharedString>),
    Failed(String),
}

#[derive(Default)]
struct Raw {
    lines: Vec<SharedString>,
    partial: String,
    decoder: Utf8Decoder,
    next_page: u64,
    loading: bool,
    done: bool,
    capped: bool,
    error: Option<String>,
}

pub struct BodyPane {
    workspace: Entity<Workspace>,
    tab: Entity<RequestTab>,
    body: Option<BodyHandle>,
    kind: Option<ContentKind>,
    probing: bool,
    pretty: Pretty,
    raw: Raw,
    sizes: Rc<Vec<Size<gpui::Pixels>>>,
    scroll: gpui_component::VirtualListScrollHandle,
    focus: FocusHandle,
    pending_jump_end: bool,
    /// Whether Pretty was on screen when the pending jump started.
    jump_in_pretty: bool,
    _sub: Subscription,
}

impl BodyPane {
    pub fn new(
        workspace: Entity<Workspace>,
        tab: Entity<RequestTab>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let sub = cx.subscribe(&tab, |this, _, event: &TabEvent, cx| {
            if *event == TabEvent::ResponseChanged {
                this.sync(cx);
            }
        });
        let mut this = Self {
            workspace,
            tab,
            body: None,
            kind: None,
            probing: false,
            pretty: Pretty::Idle,
            raw: Raw::default(),
            sizes: Rc::new(Vec::new()),
            scroll: gpui_component::VirtualListScrollHandle::new(),
            focus,
            pending_jump_end: false,
            jump_in_pretty: false,
            _sub: sub,
        };
        this.sync(cx);
        this
    }

    fn store(&self, cx: &Context<Self>) -> BodyStore {
        self.workspace.read(cx).bodies()
    }

    /// Drops all held text when the response carries a different body.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let handle = self.tab.read(cx).response.body;
        if handle.map(|h| h.id) != self.body.map(|h| h.id) {
            self.body = handle;
            self.kind = None;
            self.probing = false;
            self.pretty = Pretty::Idle;
            self.raw = Raw::default();
            self.pending_jump_end = false;
            self.sizes = Rc::new(Vec::new());
        }
        cx.notify();
    }

    fn is_current(&self, id: BodyId) -> bool {
        self.body.map(|h| h.id) == Some(id)
    }

    fn pretty_ok(&self, cx: &Context<Self>) -> bool {
        let limit = self.workspace.read(cx).settings.pretty_limit;
        let size = self.body.map_or(0, |h| h.len);
        pretty_allowed(self.kind, size, limit)
    }

    fn showing_pretty(&self, cx: &Context<Self>) -> bool {
        pretty_on_screen(
            self.tab.read(cx).body_view,
            self.pretty_ok(cx),
            &self.pretty,
        )
    }

    /// Starts whatever background work the current view needs. Idempotent.
    fn ensure(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = self.body else { return };
        if self.kind.is_none() {
            if !self.probing {
                self.probing = true;
                self.probe(handle, cx);
            }
            return;
        }
        if self.kind == Some(ContentKind::Binary) {
            return;
        }
        if self.showing_pretty(cx) {
            if matches!(self.pretty, Pretty::Idle) {
                self.start_pretty(handle, cx);
            }
        } else if self.raw.lines.is_empty() && self.raw.partial.is_empty() {
            self.load_more(cx);
        }
    }

    fn probe(&self, handle: BodyHandle, cx: &mut Context<Self>) {
        let store = self.store(cx);
        let headers = self.tab.read(cx).response.headers.clone();
        cx.spawn(async move |this, cx| {
            let kind = cx
                .background_executor()
                .spawn(async move {
                    let sniff = store.read(&handle, 0, SNIFF).unwrap_or_default();
                    content_kind(&headers, &sniff)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.is_current(handle.id) {
                    this.kind = Some(kind);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn start_pretty(&mut self, handle: BodyHandle, cx: &mut Context<Self>) {
        self.pretty = Pretty::Working;
        let store = self.store(cx);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut bytes = Vec::with_capacity(handle.len as usize);
                    let mut index = 0;
                    while let Some(p) = page(handle.len, index) {
                        let chunk = store
                            .read(&handle, p.offset, p.len)
                            .map_err(|e| e.to_string())?;
                        bytes.extend_from_slice(&chunk);
                        index += 1;
                    }
                    pretty_json(&bytes)
                        .map(|s| split_lines(&s))
                        .map_err(|e| format!("{} (at byte {})", e.message, e.offset))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.is_current(handle.id) {
                    this.set_sizes(match &result {
                        Ok(lines) => lines.len(),
                        Err(_) => 0,
                    });
                    this.pretty = match result {
                        Ok(lines) => Pretty::Ready(lines),
                        Err(e) => Pretty::Failed(e),
                    };
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn set_sizes(&mut self, n: usize) {
        self.sizes = Rc::new(vec![size(px(1.), px(LINE_HEIGHT)); n]);
    }

    /// Reads the next page of the raw view on the background executor.
    fn load_more(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = self.body else { return };
        if self.raw.loading || self.raw.done {
            return;
        }
        let Some(p) = page(handle.len, self.raw.next_page) else {
            self.finish_raw();
            cx.notify();
            return;
        };
        if p.offset >= RAW_DISPLAY_CAP {
            self.raw.capped = true;
            self.finish_raw();
            cx.notify();
            return;
        }
        self.raw.loading = true;
        let store = self.store(cx);
        cx.spawn(async move |this, cx| {
            let bytes = cx
                .background_executor()
                .spawn(async move { store.read(&handle, p.offset, p.len) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if !this.is_current(handle.id) {
                    return;
                }
                this.raw.loading = false;
                match bytes {
                    Ok(bytes) => {
                        let text = this.raw.decoder.push(&bytes);
                        let raw = &mut this.raw;
                        push_text(&mut raw.lines, &mut raw.partial, &text);
                        raw.next_page += 1;
                    }
                    Err(e) => {
                        this.raw.error = Some(e.to_string());
                        this.raw.done = true;
                    }
                }
                let n = this.raw.lines.len() + 2;
                this.set_sizes(n);
                this.advance_jump(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn pretty_working_shown(&self, cx: &Context<Self>) -> bool {
        matches!(self.pretty, Pretty::Working) && self.showing_pretty(cx)
    }

    fn on_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        if !list_on_screen(
            self.body.is_some(),
            self.kind,
            self.pretty_working_shown(cx),
        ) {
            return;
        }
        match jump_for_key(&k.key, k.modifiers.platform || k.modifiers.control) {
            Some(Jump::End) => {
                self.pending_jump_end = true;
                self.jump_in_pretty = self.showing_pretty(cx);
                self.advance_jump(cx);
            }
            Some(Jump::Top) => {
                self.pending_jump_end = false;
                self.scroll.scroll_to_item(0, ScrollStrategy::Top);
            }
            None => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Takes the next step of a pending jump to the end.
    fn advance_jump(&mut self, cx: &mut Context<Self>) {
        if !jump_still_valid(self.jump_in_pretty, self.showing_pretty(cx)) {
            self.pending_jump_end = false;
            return;
        }
        let step = jump_step(
            self.pending_jump_end,
            self.showing_pretty(cx),
            self.raw.loading,
            self.raw.done,
            self.raw.capped,
        );
        match step {
            JumpStep::Nothing | JumpStep::Wait => {}
            JumpStep::LoadMore => {
                self.load_more(cx);
                // load_more may finish at once without a page landing.
                if self.raw.done || self.raw.capped {
                    self.advance_jump(cx);
                }
            }
            JumpStep::ScrollToEnd => {
                self.pending_jump_end = false;
                let pretty_len = match &self.pretty {
                    Pretty::Ready(l) if self.showing_pretty(cx) => Some(l.len()),
                    _ => None,
                };
                let last = last_index(pretty_len, self.raw.lines.len());
                self.scroll.scroll_to_item(last, ScrollStrategy::Top);
            }
        }
    }

    fn finish_raw(&mut self) {
        let tail = self.raw.decoder.finish();
        let raw = &mut self.raw;
        let mut partial = std::mem::take(&mut raw.partial);
        partial.push_str(&tail);
        if !partial.is_empty() {
            push_line(&mut raw.lines, &partial);
        }
        raw.done = true;
        let n = raw.lines.len() + 2;
        self.set_sizes(n);
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = self.body else { return };
        let store = self.store(cx);
        let workspace = self.workspace.clone();
        let dir = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
        let name = match self.kind {
            Some(ContentKind::Json) => "response.json",
            Some(ContentKind::Xml) => "response.xml",
            Some(ContentKind::Html) => "response.html",
            Some(ContentKind::Text) => "response.txt",
            _ => "response.bin",
        };
        let rx = cx.prompt_for_new_path(&dir, Some(name));
        cx.spawn(async move |_, cx| {
            let notice = match rx.await {
                Ok(Ok(Some(path))) => {
                    let shown = path.display().to_string();
                    let result = cx
                        .background_executor()
                        .spawn(async move { write_body(&store, &handle, &path) })
                        .await;
                    match result {
                        Ok(()) => format!("Saved {shown}"),
                        Err(e) => format!("Save failed: {e}"),
                    }
                }
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(e)) => format!("Save failed: {e}"),
            };
            workspace.update(cx, |_, cx| {
                cx.emit(WorkspaceEvent::Notice(notice.into()));
            });
        })
        .detach();
    }

    fn lines_view(
        &mut self,
        id: &'static str,
        count: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if self.sizes.len() != count {
            self.set_sizes(count);
        }
        let sizes = self.sizes.clone();
        let list = v_virtual_list(cx.entity(), id, sizes, move |this, range, _window, cx| {
            if should_prefetch(
                this.body.is_some(),
                this.tab.read(cx).body_view,
                this.pretty_ok(cx),
                &this.pretty,
                range.end,
                this.raw.lines.len(),
            ) {
                this.load_more(cx);
            }
            let mono = cx.theme().mono_font_family.clone();
            let muted = cx.theme().muted_foreground;
            let pretty = if let Pretty::Ready(l) = &this.pretty {
                this.showing_pretty(cx).then_some(l)
            } else {
                None
            };
            range
                .map(|i| {
                    let text = match pretty {
                        Some(lines) => lines.get(i).cloned(),
                        None => this.raw_item(i),
                    };
                    let is_note = pretty.is_none() && i >= this.raw.lines.len();
                    div()
                        .h(px(LINE_HEIGHT))
                        .px_2()
                        .font_family(mono.clone())
                        .text_sm()
                        .whitespace_nowrap()
                        .when_note(is_note, muted)
                        .child(text.unwrap_or_default())
                })
                .collect::<Vec<_>>()
        })
        .track_scroll(&self.scroll);
        // v_virtual_list has no scrollbar of its own, so the list sits in a
        // relative wrapper and the shared handle drives a vertical scrollbar
        // layer (ScrollableElement::vertical_scrollbar, as in scroll/scrollable.rs).
        div()
            .id(SharedString::from(format!("{id}-scroll")))
            .relative()
            .size_full()
            .child(list)
            .vertical_scrollbar(&self.scroll)
            .into_any_element()
    }

    fn raw_item(&self, i: usize) -> Option<SharedString> {
        if let Some(l) = self.raw.lines.get(i) {
            return Some(l.clone());
        }
        if i == self.raw.lines.len() {
            if self.raw.capped {
                return Some("Showing the first 4 MB — Save to read the rest".into());
            }
            return self.raw.error.clone().map(Into::into);
        }
        None
    }
}

trait NoteStyle: Styled + Sized {
    fn when_note(self, note: bool, color: gpui::Hsla) -> Self {
        if note { self.text_color(color) } else { self }
    }
}
impl NoteStyle for gpui::Div {}

fn write_body(
    store: &BodyStore,
    handle: &BodyHandle,
    path: &std::path::Path,
) -> Result<(), String> {
    // Only a file we created is ours to remove; a failed create leaves the target alone.
    let mut file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let result = write_pages(store, handle, &mut file);
    if result.is_err() {
        drop(file);
        let _ = std::fs::remove_file(path);
    }
    result
}

fn write_pages(
    store: &BodyStore,
    handle: &BodyHandle,
    file: &mut std::fs::File,
) -> Result<(), String> {
    let mut index = 0;
    while let Some(p) = page(handle.len, index) {
        let chunk = store
            .read(handle, p.offset, p.len)
            .map_err(|e| e.to_string())?;
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        index += 1;
    }
    file.flush().map_err(|e| e.to_string())
}

impl Render for BodyPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure(cx);
        if self.tab.read(cx).response.phase == Phase::Failed {
            return self.error_panel(cx).into_any_element();
        }
        let size = self.body.map_or(0, |h| h.len);
        let view = self.tab.read(cx).body_view;
        let pretty_ok = self.pretty_ok(cx);
        let pretty_shown = self.showing_pretty(cx);
        let limit = self.workspace.read(cx).settings.pretty_limit;

        let tab = self.tab.clone();
        let tab2 = self.tab.clone();
        let pretty_btn = Button::new("view-pretty")
            .label("Pretty")
            .small()
            .ghost()
            .selected(pretty_shown)
            .disabled(!pretty_ok && self.kind.is_some())
            .on_click(move |_, _, cx| tab.update(cx, |t, cx| set_view(t, BodyView::Pretty, cx)));
        let pretty_btn = if !pretty_ok && self.kind.is_some() {
            let why = if self.kind == Some(ContentKind::Json) {
                format!(
                    "Body is over the pretty-print limit ({})",
                    crate::format::format_size(limit)
                )
            } else {
                "Pretty is only available for JSON".to_string()
            };
            pretty_btn.tooltip(why)
        } else {
            pretty_btn
        };
        let raw_btn = Button::new("view-raw")
            .label("Raw")
            .small()
            .ghost()
            .selected(!pretty_shown)
            .on_click(move |_, _, cx| tab2.update(cx, |t, cx| set_view(t, BodyView::Raw, cx)));
        let save = cx.listener(|this, _, _, cx| this.save(cx));
        let toolbar = div()
            .flex()
            .items_center()
            .gap_1()
            .p_1()
            .child(pretty_btn)
            .child(raw_btn)
            .child(div().flex_1())
            .child(
                Button::new("save-body")
                    .label("Save")
                    .small()
                    .on_click(save),
            );

        let mut root = div().flex().flex_col().size_full().child(toolbar);
        if let Pretty::Failed(e) = &self.pretty
            && view == BodyView::Pretty
        {
            root = root.child(
                div()
                    .px_2()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(format!("Not valid JSON, showing raw: {e}")),
            );
        }

        let body = if self.body.is_none() {
            div().p_2().child("No body")
        } else if self.kind.is_none() || matches!(self.pretty, Pretty::Working) && pretty_shown {
            div().p_2().child(Spinner::new())
        } else if self.kind == Some(ContentKind::Binary) {
            div().p_2().child(format!("Binary body, {size} bytes"))
        } else if pretty_shown {
            let n = match &self.pretty {
                Pretty::Ready(l) => l.len(),
                _ => 0,
            };
            div().flex_1().child(self.lines_view("pretty-lines", n, cx))
        } else {
            let n = self.raw.lines.len() + 2;
            div().flex_1().child(self.lines_view("raw-lines", n, cx))
        };
        root.id("body-pane")
            .track_focus(&self.focus)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| window.focus(&this.focus, cx)),
            )
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| this.on_key(ev, cx)))
            .child(body.min_h_0())
            .into_any_element()
    }
}

impl BodyPane {
    /// The whole failure in place of the body: heading, wrapped scrollable message, Copy.
    fn error_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let err = self.tab.read(cx).response.error.clone();
        let text = error_text(err.as_ref());
        let words = err
            .as_ref()
            .map_or("Request failed", |e| error_words(e.kind));
        let message = err.map(|e| e.message).unwrap_or_default();
        let danger = cx.theme().danger;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .size_full()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().text_color(danger).child(words))
                    .child(
                        Button::new("copy-error")
                            .icon(IconName::Copy)
                            .label("Copy error")
                            .small()
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                            }),
                    ),
            )
            .child(
                div()
                    .id("error-message")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(div().w_full().child(message)),
            )
    }
}

fn set_view(t: &mut RequestTab, view: BodyView, cx: &mut Context<RequestTab>) {
    t.body_view = view;
    cx.emit(TabEvent::ResponseChanged);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn save_fixture() -> (BodyStore, BodyHandle, std::path::PathBuf) {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let store = BodyStore::new(1 << 20);
        let mut w = store.writer();
        w.write(b"hello body").unwrap();
        let handle = w.finish().unwrap();
        let dir = std::env::temp_dir().join(format!("wisp-save-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        (store, handle, dir)
    }

    #[cfg(unix)]
    #[test]
    fn failed_create_keeps_existing_target() {
        use std::os::unix::fs::PermissionsExt;
        let (store, handle, dir) = save_fixture();
        let target = dir.join("existing.txt");
        std::fs::write(&target, b"original").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o444)).unwrap();
        let denied = std::fs::OpenOptions::new()
            .write(true)
            .open(&target)
            .is_err();
        if denied {
            assert!(write_body(&store, &handle, &target).is_err());
            assert_eq!(std::fs::read(&target).unwrap(), b"original");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failure_after_create_removes_partial_file() {
        let (store, handle, dir) = save_fixture();
        let target = dir.join("out.bin");
        store.release(&handle);
        assert!(write_body(&store, &handle, &target).is_err());
        assert!(!target.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn successful_save_writes_body() {
        let (store, handle, dir) = save_fixture();
        let target = dir.join("ok.bin");
        write_body(&store, &handle, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"hello body");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pretty_only_for_small_json() {
        assert!(pretty_allowed(Some(ContentKind::Json), 10, 10));
        assert!(!pretty_allowed(Some(ContentKind::Json), 11, 10));
        assert!(!pretty_allowed(Some(ContentKind::Text), 1, 10));
        assert!(!pretty_allowed(None, 1, 10));
    }

    #[test]
    fn lines_split_across_pushes() {
        let mut lines = Vec::new();
        let mut partial = String::new();
        push_text(&mut lines, &mut partial, "a\r\nb");
        push_text(&mut lines, &mut partial, "c\n\nd");
        let got: Vec<&str> = lines.iter().map(|l| l.as_ref()).collect();
        assert_eq!(got, ["a", "bc", ""]);
        assert_eq!(partial, "d");
    }

    #[test]
    fn long_lines_are_chunked_on_char_boundaries() {
        let text = "é".repeat(MAX_LINE);
        let lines = split_lines(&text);
        assert!(lines.len() >= 2);
        assert!(lines.iter().all(|l| l.len() <= MAX_LINE));
        assert_eq!(lines.concat(), text);
    }

    #[test]
    fn page_size_is_a_divisor_of_the_cap() {
        use crate::pager::PAGE_SIZE;
        assert_eq!(RAW_DISPLAY_CAP % PAGE_SIZE as u64, 0);
    }

    #[test]
    fn a_pending_jump_loads_waits_or_scrolls_by_state() {
        // (a) pending, idle, not finished: load the next page.
        assert_eq!(
            jump_step(true, false, false, false, false),
            JumpStep::LoadMore
        );
        // (b) pending, a page in flight: wait.
        assert_eq!(jump_step(true, false, true, false, false), JumpStep::Wait);
        // (c) pending, done or capped: scroll to the end.
        assert_eq!(
            jump_step(true, false, false, true, false),
            JumpStep::ScrollToEnd
        );
        assert_eq!(
            jump_step(true, false, false, false, true),
            JumpStep::ScrollToEnd
        );
        // (d) Pretty on screen: scroll at once, no raw load.
        assert_eq!(
            jump_step(true, true, false, false, false),
            JumpStep::ScrollToEnd
        );
        assert_eq!(
            jump_step(true, true, true, false, false),
            JumpStep::ScrollToEnd
        );
        // (e) nothing pending: nothing.
        assert_eq!(
            jump_step(false, false, false, false, false),
            JumpStep::Nothing
        );
        assert_eq!(jump_step(false, true, false, true, true), JumpStep::Nothing);
    }

    #[test]
    fn list_gating_and_last_index_follow_the_view_on_screen() {
        let t = Some(ContentKind::Text);
        assert!(list_on_screen(true, t, false));
        assert!(!list_on_screen(false, t, false));
        assert!(!list_on_screen(true, None, false));
        assert!(!list_on_screen(true, Some(ContentKind::Binary), false));
        assert!(!list_on_screen(true, Some(ContentKind::Json), true));
        assert_eq!(last_index(None, 10), 11);
        assert_eq!(last_index(Some(5), 10), 4);
        assert_eq!(last_index(Some(0), 10), 0);
    }

    #[test]
    fn end_home_and_command_arrows_map_to_jumps() {
        assert_eq!(jump_for_key("end", false), Some(Jump::End));
        assert_eq!(jump_for_key("down", true), Some(Jump::End));
        assert_eq!(jump_for_key("home", false), Some(Jump::Top));
        assert_eq!(jump_for_key("up", true), Some(Jump::Top));
        assert_eq!(jump_for_key("end", true), Some(Jump::End));
        assert_eq!(jump_for_key("home", true), Some(Jump::Top));
        assert!(jump_still_valid(false, false));
        assert!(jump_still_valid(true, true));
        assert!(!jump_still_valid(false, true));
        assert!(!jump_still_valid(true, false));
        assert_eq!(jump_for_key("down", false), None);
        assert_eq!(jump_for_key("up", false), None);
        assert_eq!(jump_for_key("a", true), None);
    }

    #[test]
    fn raw_view_prefetches_even_when_pretty_is_ready() {
        let ready = Pretty::Ready(vec![]);
        // Raw on screen, Pretty already Ready, near the end: must prefetch.
        assert!(should_prefetch(
            true,
            BodyView::Raw,
            true,
            &ready,
            990,
            1000
        ));
        // Pretty on screen: no raw prefetch.
        assert!(!should_prefetch(
            true,
            BodyView::Pretty,
            true,
            &ready,
            990,
            1000
        ));
        // Failed Pretty falls back to Raw, which must prefetch.
        let failed = Pretty::Failed(String::new());
        assert!(should_prefetch(
            true,
            BodyView::Pretty,
            true,
            &failed,
            990,
            1000
        ));
        // Far from the end, or no body: no prefetch.
        assert!(!should_prefetch(
            true,
            BodyView::Raw,
            true,
            &ready,
            10,
            1000
        ));
        assert!(!should_prefetch(
            false,
            BodyView::Raw,
            true,
            &ready,
            990,
            1000
        ));
    }
}
