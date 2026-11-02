//! Body sub-tab: type select plus the editor for the selected body type.

use std::path::{Path, PathBuf};

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, PathPromptOptions, Render,
    SharedString, Styled, Subscription, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Editor, EditorState, Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::select::{Select, SelectEvent, SelectState};
use gpui_component::{ActiveTheme as _, IconName, IndexPath, Sizable as _};
use wisp_core::request::{Body, KeyValue, Part, PartValue, TextBody};

use crate::pretty::pretty_json;
use crate::tab::{RequestTab, TabEvent};
use crate::ui::kv::{KvEvent, KvOptions, KvTable};
use crate::workspace::Workspace;

type LabelSelect = Entity<SelectState<Vec<&'static str>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    None,
    Json,
    Text,
    Xml,
    Form,
    Multipart,
    File,
}

const KINDS: [BodyKind; 7] = [
    BodyKind::None,
    BodyKind::Json,
    BodyKind::Text,
    BodyKind::Xml,
    BodyKind::Form,
    BodyKind::Multipart,
    BodyKind::File,
];

impl BodyKind {
    pub fn of(body: &Body) -> Self {
        match body {
            Body::None => Self::None,
            Body::Json(_) => Self::Json,
            Body::Text(_) => Self::Text,
            Body::Xml(_) => Self::Xml,
            Body::Form(_) => Self::Form,
            Body::Multipart(_) => Self::Multipart,
            Body::File(_) => Self::File,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Json => "JSON",
            Self::Text => "Text",
            Self::Xml => "XML",
            Self::Form => "Form",
            Self::Multipart => "Multipart",
            Self::File => "File",
        }
    }

    fn from_label(label: &str) -> Option<Self> {
        KINDS.into_iter().find(|k| k.label() == label)
    }

    fn is_text(self) -> bool {
        matches!(self, Self::Json | Self::Text | Self::Xml)
    }
}

/// Content remembered for the life of the tab so a switch loses nothing.
/// The three text types share one `TextBody`, sidecar included.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stash {
    pub text: TextBody,
    pub form: Vec<KeyValue>,
    pub multipart: Vec<Part>,
    pub file: PathBuf,
}

impl Stash {
    /// Records the content of `body`; `Body::None` records nothing.
    pub fn absorb(&mut self, body: &Body) {
        match body {
            Body::None => {}
            Body::Json(t) | Body::Text(t) | Body::Xml(t) => self.text = t.clone(),
            Body::Form(rows) => self.form = rows.clone(),
            Body::Multipart(parts) => self.multipart = parts.clone(),
            Body::File(path) => self.file = path.clone(),
        }
    }

    pub fn body_for(&self, kind: BodyKind) -> Body {
        match kind {
            BodyKind::None => Body::None,
            BodyKind::Json => Body::Json(self.text.clone()),
            BodyKind::Text => Body::Text(self.text.clone()),
            BodyKind::Xml => Body::Xml(self.text.clone()),
            BodyKind::Form => Body::Form(self.form.clone()),
            BodyKind::Multipart => Body::Multipart(self.multipart.clone()),
            BodyKind::File => Body::File(self.file.clone()),
        }
    }
}

/// Body to use after switching `current` to `kind`: text content survives
/// among JSON/Text/XML, and every type gets back what it last held.
pub fn convert(current: &Body, kind: BodyKind, stash: &mut Stash) -> Body {
    stash.absorb(current);
    stash.body_for(kind)
}

/// Offset of the first JSON syntax error; `None` for valid or empty content.
fn json_error_offset(content: &str) -> Option<usize> {
    if content.trim().is_empty() {
        return None;
    }
    pretty_json(content.as_bytes()).err().map(|e| e.offset)
}

struct PartRow {
    enabled: bool,
    is_file: bool,
    name: Entity<InputState>,
    text: Entity<InputState>,
    path: Entity<InputState>,
    _subs: Vec<Subscription>,
}

impl PartRow {
    fn is_empty(&self, cx: &gpui::App) -> bool {
        [&self.name, &self.text, &self.path]
            .iter()
            .all(|i| i.read(cx).value().is_empty())
    }

    fn part(&self, cx: &gpui::App) -> Part {
        let value = if self.is_file {
            PartValue::File(PathBuf::from(self.path.read(cx).value().to_string()))
        } else {
            PartValue::Text(self.text.read(cx).value().to_string())
        };
        Part {
            name: self.name.read(cx).value().to_string(),
            value,
            enabled: self.enabled,
        }
    }
}

#[derive(Clone, Copy)]
enum PathTarget {
    Part(usize),
    Body,
}

pub struct BodyEditor {
    _workspace: Entity<Workspace>,
    tab: Entity<RequestTab>,
    kind: BodyKind,
    stash: Stash,
    json_error: Option<usize>,
    kind_select: LabelSelect,
    editor: Entity<EditorState>,
    form: Entity<KvTable>,
    /// Real parts followed by one empty trailing row.
    rows: Vec<PartRow>,
    file_path: Entity<InputState>,
    _subs: Vec<Subscription>,
}

impl BodyEditor {
    pub fn new(
        workspace: Entity<Workspace>,
        tab: Entity<RequestTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let kind_select = cx.new(|cx| {
            let items: Vec<&'static str> = KINDS.iter().map(|k| k.label()).collect();
            SelectState::new(items, Some(IndexPath::new(0)), window, cx)
        });
        let editor = cx.new(|cx| EditorState::new(window, cx));
        let form = cx.new(|cx| {
            KvTable::new(
                Vec::new(),
                KvOptions {
                    name_placeholder: "Name".into(),
                    value_placeholder: "Value".into(),
                    toggles: true,
                    mask_values: false,
                },
                window,
                cx,
            )
        });
        let file_path = cx.new(|cx| InputState::new(window, cx).placeholder("File path"));

        let subs = vec![
            cx.subscribe_in(&kind_select, window, Self::on_kind),
            cx.subscribe_in(&editor, window, Self::on_text),
            cx.subscribe_in(&form, window, Self::on_form),
            cx.subscribe_in(&file_path, window, Self::on_file_path),
            cx.subscribe_in(&tab, window, Self::on_tab),
        ];
        let mut this = Self {
            _workspace: workspace,
            tab,
            kind: BodyKind::None,
            stash: Stash::default(),
            json_error: None,
            kind_select,
            editor,
            form,
            rows: Vec::new(),
            file_path,
            _subs: subs,
        };
        this.resync(window, cx);
        this
    }

    /// One `tab.edit` per real change; echoes of a resync compare equal.
    fn commit(&mut self, cx: &mut Context<Self>) {
        let body = self.stash.body_for(self.kind);
        let tab = self.tab.clone();
        tab.update(cx, |t, cx| {
            if t.draft.body != body {
                t.edit(|r| r.body = body, cx);
            }
        });
        cx.notify();
    }

    fn refresh_json(&mut self) {
        self.json_error = if self.kind == BodyKind::Json {
            json_error_offset(&self.stash.text.content)
        } else {
            None
        };
    }

    fn on_kind(
        &mut self,
        _: &LabelSelect,
        event: &SelectEvent<Vec<&'static str>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let SelectEvent::Confirm(Some(label)) = event else {
            return;
        };
        let Some(kind) = BodyKind::from_label(label) else {
            return;
        };
        let current = self.tab.read(cx).draft.body.clone();
        convert(&current, kind, &mut self.stash);
        self.kind = kind;
        self.refresh_json();
        self.commit(cx);
    }

    fn on_text(
        &mut self,
        editor: &Entity<EditorState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let content = editor.read(cx).value().to_string();
        if self.stash.text.content == content {
            return;
        }
        self.stash.text.content = content;
        self.refresh_json();
        if self.kind.is_text() {
            self.commit(cx);
        }
    }

    fn on_form(
        &mut self,
        _: &Entity<KvTable>,
        event: &KvEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let KvEvent::Changed(rows) = event;
        self.stash.form = rows.clone();
        if self.kind == BodyKind::Form {
            self.commit(cx);
        }
    }

    fn on_file_path(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        self.stash.file = PathBuf::from(input.read(cx).value().to_string());
        if self.kind == BodyKind::File {
            self.commit(cx);
        }
    }

    fn on_tab(
        &mut self,
        _: &Entity<RequestTab>,
        event: &TabEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, TabEvent::DraftReplaced) {
            self.resync(window, cx);
        }
    }

    /// Loads the draft's body into the widgets; other types keep their memory.
    fn resync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.tab.read(cx).draft.body.clone();
        self.stash.absorb(&body);
        self.kind = BodyKind::of(&body);
        self.refresh_json();

        let content = self.stash.text.content.clone();
        if self.editor.read(cx).value().as_ref() != content {
            self.editor
                .update(cx, |e, cx| e.set_value(content, window, cx));
        }
        let rows = self.stash.form.clone();
        self.form.update(cx, |f, cx| f.set_rows(rows, window, cx));
        let path = self.stash.file.to_string_lossy().into_owned();
        if self.file_path.read(cx).value().as_ref() != path {
            self.file_path
                .update(cx, |p, cx| p.set_value(path, window, cx));
        }
        let parts = self.stash.multipart.clone();
        self.rows.clear();
        for part in &parts {
            let row = self.make_row(Some(part), window, cx);
            self.rows.push(row);
        }
        let trailing = self.make_row(None, window, cx);
        self.rows.push(trailing);

        let label = self.kind.label();
        self.kind_select
            .update(cx, |s, cx| s.set_selected_value(&label, window, cx));
        cx.notify();
    }

    fn make_row(
        &mut self,
        part: Option<&Part>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> PartRow {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Name"));
        let text = cx.new(|cx| InputState::new(window, cx).placeholder("Value"));
        let path = cx.new(|cx| InputState::new(window, cx).placeholder("File path"));
        let mut enabled = true;
        let mut is_file = false;
        if let Some(part) = part {
            enabled = part.enabled;
            name.update(cx, |s, cx| s.set_value(part.name.clone(), window, cx));
            match &part.value {
                PartValue::Text(t) => text.update(cx, |s, cx| s.set_value(t.clone(), window, cx)),
                PartValue::File(p) => {
                    is_file = true;
                    let p = p.to_string_lossy().into_owned();
                    path.update(cx, |s, cx| s.set_value(p, window, cx));
                }
            }
        }
        let subs = [&name, &text, &path]
            .into_iter()
            .map(|i| cx.subscribe_in(i, window, Self::on_part_input))
            .collect();
        PartRow {
            enabled,
            is_file,
            name,
            text,
            path,
            _subs: subs,
        }
    }

    fn on_part_input(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::Change) {
            self.sync_parts(window, cx);
        }
    }

    /// Grows the trailing row when used, then publishes the real parts.
    fn sync_parts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.rows.last().is_none_or(|r| !r.is_empty(cx)) {
            let row = self.make_row(None, window, cx);
            self.rows.push(row);
        }
        let count = self.rows.len() - 1;
        let parts: Vec<Part> = self.rows[..count].iter().map(|r| r.part(cx)).collect();
        if self.stash.multipart != parts {
            self.stash.multipart = parts;
            if self.kind == BodyKind::Multipart {
                self.commit(cx);
            }
        }
        cx.notify();
    }

    fn set_part_enabled(
        &mut self,
        i: usize,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(row) = self.rows.get_mut(i) {
            row.enabled = enabled;
            self.sync_parts(window, cx);
        }
    }

    fn toggle_part_file(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get_mut(i) {
            row.is_file = !row.is_file;
            self.sync_parts(window, cx);
        }
    }

    fn delete_part(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        if i + 1 < self.rows.len() {
            self.rows.remove(i);
            self.sync_parts(window, cx);
        }
    }

    fn format_json(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Ok(pretty) = pretty_json(self.stash.text.content.as_bytes()) {
            self.editor
                .update(cx, |e, cx| e.set_value(pretty.clone(), window, cx));
            // `set_value` emits no Change, so publish by hand.
            self.stash.text.content = pretty;
            self.refresh_json();
            if self.kind.is_text() {
                self.commit(cx);
            }
        }
    }

    fn choose(&mut self, target: PathTarget, window: &mut Window, cx: &mut Context<Self>) {
        let is_body = matches!(target, PathTarget::Body);
        let input = match target {
            PathTarget::Body => self.file_path.clone(),
            PathTarget::Part(i) => match self.rows.get(i) {
                Some(r) => r.path.clone(),
                None => return,
            },
        };
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(path) = paths.into_iter().next()
            {
                this.update_in(cx, move |this, window, cx| {
                    let text = path.to_string_lossy().into_owned();
                    input.update(cx, |s, cx| s.set_value(text.clone(), window, cx));
                    // `set_value` emits no Change, so publish by hand.
                    if is_body {
                        this.stash.file = PathBuf::from(text);
                        if this.kind == BodyKind::File {
                            this.commit(cx);
                        }
                    } else {
                        this.sync_parts(window, cx);
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    fn render_text(&self, cx: &mut Context<Self>) -> gpui::Div {
        let mut col = div().flex().flex_col().gap_2();
        if self.kind == BodyKind::Json {
            let view = cx.entity();
            let status: SharedString = match self.json_error {
                Some(offset) => format!("invalid JSON at offset {offset}").into(),
                None => "".into(),
            };
            col = col.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(Button::new("body-format").label("Format").small().on_click(
                        move |_, window, cx| {
                            view.update(cx, |this, cx| this.format_json(window, cx));
                        },
                    ))
                    .child(div().text_color(cx.theme().warning).child(status)),
            );
        }
        col.child(Editor::new(&self.editor).h(px(260.)))
    }

    /// Bounded, vertically scrolling area for the editors whose content can outgrow the pane.
    fn scroll_area(id: &'static str, content: gpui::Div) -> impl IntoElement {
        div().flex_1().min_h_0().child(
            div()
                .size_full()
                .child(content)
                .overflow_y_scrollbar()
                .id(id),
        )
    }

    fn render_multipart(&self, cx: &mut Context<Self>) -> gpui::Div {
        let view = cx.entity();
        let last = self.rows.len().saturating_sub(1);
        let mut list = div().flex().flex_col().gap_1();
        for (i, row) in self.rows.iter().enumerate() {
            let trailing = i == last;
            let mut line = div().flex().flex_row().items_center().gap_1().h(px(30.));
            line = line.child(div().w(px(20.)).when_not_trailing(trailing, || {
                let view = view.clone();
                Checkbox::new(("part-enabled", i))
                    .tab_stop(false)
                    .checked(row.enabled)
                    .on_click(move |checked, window, cx| {
                        let checked = *checked;
                        view.update(cx, |this, cx| this.set_part_enabled(i, checked, window, cx));
                    })
            }));
            line = line.child(div().flex_1().child(Input::new(&row.name).small()));
            line = line.child({
                let view = view.clone();
                Button::new(("part-kind", i))
                    .tab_stop(false)
                    .label(if row.is_file { "File" } else { "Text" })
                    .ghost()
                    .xsmall()
                    .on_click(move |_, window, cx| {
                        view.update(cx, |this, cx| this.toggle_part_file(i, window, cx));
                    })
            });
            if row.is_file {
                let view = view.clone();
                line = line
                    .child(div().flex_1().child(Input::new(&row.path).small()))
                    .child(
                        Button::new(("part-choose", i))
                            .tab_stop(false)
                            .label("Choose…")
                            .small()
                            .on_click(move |_, window, cx| {
                                view.update(cx, |this, cx| {
                                    this.choose(PathTarget::Part(i), window, cx)
                                });
                            }),
                    );
            } else {
                line = line.child(div().flex_1().child(Input::new(&row.text).small()));
            }
            line = line.child(div().w(px(24.)).when_not_trailing(trailing, || {
                let view = view.clone();
                Button::new(("part-delete", i))
                    .tab_stop(false)
                    .icon(IconName::Close)
                    .ghost()
                    .xsmall()
                    .tooltip("Delete row")
                    .on_click(move |_, window, cx| {
                        view.update(cx, |this, cx| this.delete_part(i, window, cx));
                    })
            }));
            list = list.child(line);
        }
        list
    }

    fn render_file(&self, cx: &mut Context<Self>) -> gpui::Div {
        let view = cx.entity();
        let mut col = div().flex().flex_col().gap_2().child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .child(div().flex_1().child(Input::new(&self.file_path)))
                .child(Button::new("body-choose").label("Choose…").on_click(
                    move |_, window, cx| {
                        view.update(cx, |this, cx| this.choose(PathTarget::Body, window, cx));
                    },
                )),
        );
        let path: &Path = &self.stash.file;
        if !path.as_os_str().is_empty() && !path.exists() {
            col = col.child(div().text_color(cx.theme().warning).child("File not found"));
        }
        col
    }
}

/// Lets a row builder skip the checkbox and delete button on the trailing row.
trait TrailingExt: Sized {
    fn when_not_trailing<E: IntoElement>(self, trailing: bool, f: impl FnOnce() -> E) -> Self;
}

impl TrailingExt for gpui::Div {
    fn when_not_trailing<E: IntoElement>(self, trailing: bool, f: impl FnOnce() -> E) -> Self {
        if trailing { self } else { self.child(f()) }
    }
}

impl Render for BodyEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut col = div().size_full().flex().flex_col().gap_2().p_2().child(
            div()
                .w(px(180.))
                .child(Select::new(&self.kind_select).id("body-kind")),
        );
        col = match self.kind {
            BodyKind::None => col,
            BodyKind::Json | BodyKind::Text | BodyKind::Xml => col.child(self.render_text(cx)),
            BodyKind::Form => col.child(div().flex_1().min_h_0().child(self.form.clone())),
            BodyKind::Multipart => col.child(Self::scroll_area(
                "body-multipart",
                self.render_multipart(cx),
            )),
            BodyKind::File => col.child(Self::scroll_area("body-file", self.render_file(cx))),
        };
        col
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(content: &str, sidecar: Option<&str>) -> TextBody {
        TextBody {
            content: content.into(),
            sidecar: sidecar.map(String::from),
        }
    }

    fn row(name: &str) -> KeyValue {
        KeyValue {
            name: name.into(),
            value: "v".into(),
            enabled: true,
        }
    }

    #[test]
    fn text_content_and_sidecar_survive_among_text_types() {
        let mut s = Stash::default();
        let json = Body::Json(text("{}", Some("body.json")));
        assert_eq!(
            convert(&json, BodyKind::Xml, &mut s),
            Body::Xml(text("{}", Some("body.json")))
        );
        assert_eq!(
            convert(
                &Body::Xml(text("{}", Some("body.json"))),
                BodyKind::Text,
                &mut s
            ),
            Body::Text(text("{}", Some("body.json")))
        );
    }

    #[test]
    fn none_target_is_none() {
        let mut s = Stash::default();
        assert_eq!(
            convert(&Body::Json(text("x", None)), BodyKind::None, &mut s),
            Body::None
        );
    }

    #[test]
    fn leaving_and_returning_restores_content() {
        let mut s = Stash::default();
        let form = Body::Form(vec![row("a")]);
        let away = convert(&form, BodyKind::Json, &mut s);
        assert_eq!(away, Body::Json(TextBody::default()));
        assert_eq!(convert(&away, BodyKind::Form, &mut s), form);
    }

    #[test]
    fn text_survives_a_detour_through_another_type() {
        let mut s = Stash::default();
        let json = Body::Json(text("abc", None));
        let form = convert(&json, BodyKind::Form, &mut s);
        assert_eq!(
            convert(&form, BodyKind::Text, &mut s),
            Body::Text(text("abc", None))
        );
    }

    #[test]
    fn file_and_multipart_are_remembered() {
        let mut s = Stash::default();
        let file = Body::File(PathBuf::from("/tmp/x"));
        let part = Body::Multipart(vec![Part {
            name: "p".into(),
            value: PartValue::Text("t".into()),
            enabled: true,
        }]);
        let none = convert(&file, BodyKind::None, &mut s);
        let mp = convert(&part, BodyKind::Multipart, &mut s);
        assert_eq!(mp, part);
        assert_eq!(convert(&none, BodyKind::File, &mut s), file);
    }

    #[test]
    fn kind_round_trips_through_labels() {
        for k in KINDS {
            assert_eq!(BodyKind::from_label(k.label()), Some(k));
        }
    }

    #[test]
    fn json_error_offset_reports_invalid_only() {
        assert_eq!(json_error_offset(""), None);
        assert_eq!(json_error_offset("{\"a\": 1}"), None);
        assert!(json_error_offset("{\"a\": }").is_some());
    }
}
