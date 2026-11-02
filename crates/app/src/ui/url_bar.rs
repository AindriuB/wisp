//! Method select, URL input, Send/Cancel, the variable-highlight strip and resolve problems.

use std::path::{Component, Path};

use gpui::{
    AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled, Subscription,
    Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::select::{Select, SelectEvent, SelectState};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme as _, Disableable as _, IndexPath};
use wisp_core::Method;
use wisp_core::resolve::{Location, Problem, resolve};

use crate::highlight::{self, SpanKind, VarState};
use crate::tab::{Phase, RequestTab, TabEvent};
use crate::url_sync::{apply_url_text, display_url, sync_path_vars};
use crate::workspace::{Workspace, WorkspaceEvent};

const METHODS: [(Method, &str); 7] = [
    (Method::Get, "GET"),
    (Method::Post, "POST"),
    (Method::Put, "PUT"),
    (Method::Patch, "PATCH"),
    (Method::Delete, "DELETE"),
    (Method::Head, "HEAD"),
    (Method::Options, "OPTIONS"),
];

fn method_index(method: Method) -> usize {
    METHODS.iter().position(|(m, _)| *m == method).unwrap_or(0)
}

/// Every directory component of a request's relative path, outermost first.
pub fn folder_names(path: Option<&Path>) -> Vec<String> {
    let Some(parent) = path.and_then(Path::parent) else {
        return Vec::new();
    };
    parent
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

fn location_text(location: &Location) -> String {
    match location {
        Location::Url => "URL".to_string(),
        Location::PathParam(n) => format!("path parameter {n}"),
        Location::Param(n) => format!("param {n}"),
        Location::Header(n) => format!("header {n}"),
        Location::Auth => "auth".to_string(),
        Location::Body => "body".to_string(),
        Location::Var(n) => format!("variable {n}"),
    }
}

/// One plain-words line for a resolve problem.
pub fn problem_text(problem: &Problem) -> String {
    match problem {
        Problem::UnresolvedVar { name, location } => {
            format!(
                "{{{{{name}}}}} is not defined ({})",
                location_text(location)
            )
        }
        Problem::MissingPathParam { name } => format!("{{{name}}} has no value (path)"),
        Problem::Syntax { location, message } => {
            format!("{message} ({})", location_text(location))
        }
        Problem::VarCycle { name } => format!("{{{{{name}}}}} refers to itself"),
    }
}

/// `should_set_text`, and forget the own-edit record when an outside edit overwrites the text.
fn take_resync(focused: bool, own_last: &mut Option<String>, current: &str, want: &str) -> bool {
    let set = should_set_text(focused, own_last.as_deref(), current, want);
    if set {
        *own_last = None;
    }
    set
}

/// Whether the URL input's text must be replaced by `want`. A focused input keeps the text
/// its own last edit produced (it may differ from `display_url`, e.g. a trailing `&`).
fn should_set_text(focused: bool, own_last: Option<&str>, current: &str, want: &str) -> bool {
    if current == want {
        return false;
    }
    !(focused && own_last == Some(want))
}

struct Segment {
    text: String,
    kind: SpanKind,
    tip: Option<SharedString>,
}

pub struct UrlBar {
    workspace: Entity<Workspace>,
    tab: Entity<RequestTab>,
    method: Entity<SelectState<Vec<SharedString>>>,
    input: Entity<InputState>,
    segments: Vec<Segment>,
    problems: Vec<String>,
    /// What the draft's URL text was right after this input's own last edit.
    own_last: Option<String>,
    _subs: Vec<Subscription>,
}

impl UrlBar {
    pub fn new(
        workspace: Entity<Workspace>,
        tab: Entity<RequestTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (text, method) = {
            let d = &tab.read(cx).draft;
            (display_url(&d.url, &d.params), d.method)
        };
        let items: Vec<SharedString> = METHODS.iter().map(|(_, s)| (*s).into()).collect();
        let select = cx.new(|cx| {
            SelectState::new(
                items,
                Some(IndexPath::default().row(method_index(method))),
                window,
                cx,
            )
        });
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://example.com/path")
                .default_value(text)
        });
        let subs = vec![
            cx.subscribe_in(&select, window, Self::on_method),
            cx.subscribe_in(&input, window, Self::on_input),
            cx.subscribe_in(&tab, window, Self::on_tab),
            cx.subscribe(&workspace, Self::on_workspace),
        ];
        let mut this = Self {
            workspace,
            tab,
            method: select,
            input,
            segments: Vec::new(),
            problems: Vec::new(),
            own_last: None,
            _subs: subs,
        };
        this.refresh(cx);
        this
    }

    fn on_method(
        &mut self,
        _: &Entity<SelectState<Vec<SharedString>>>,
        event: &SelectEvent<Vec<SharedString>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let SelectEvent::Confirm(Some(value)) = event else {
            return;
        };
        let Some((method, _)) = METHODS.iter().find(|(_, s)| *s == value.as_ref()) else {
            return;
        };
        let method = *method;
        if self.tab.read(cx).draft.method != method {
            self.tab
                .update(cx, |t, cx| t.edit(|d| d.method = method, cx));
        }
    }

    fn on_input(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let text = input.read(cx).value();
        let current = {
            let d = &self.tab.read(cx).draft;
            display_url(&d.url, &d.params)
        };
        // Our own programmatic write echoes back as a Change; ignore it.
        if text.as_ref() == current {
            return;
        }
        self.tab.update(cx, |t, cx| {
            t.edit(
                |d| {
                    let (url, params) = apply_url_text(text.as_ref(), &d.params);
                    d.path = sync_path_vars(&url, &d.path);
                    d.url = url;
                    d.params = params;
                },
                cx,
            );
        });
        let d = &self.tab.read(cx).draft;
        self.own_last = Some(display_url(&d.url, &d.params));
    }

    fn on_tab(
        &mut self,
        _: &Entity<RequestTab>,
        event: &TabEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TabEvent::DraftEdited => {
                let focused = self.input.read(cx).focus_handle(cx).is_focused(window);
                self.resync(focused, window, cx);
                self.refresh(cx);
            }
            TabEvent::DraftReplaced => {
                self.own_last = None;
                self.resync(false, window, cx);
                self.refresh(cx);
            }
            _ => {}
        }
        cx.notify();
    }

    fn on_workspace(
        &mut self,
        _: Entity<Workspace>,
        event: &WorkspaceEvent,
        cx: &mut Context<Self>,
    ) {
        if matches!(
            event,
            WorkspaceEvent::EnvironmentChanged | WorkspaceEvent::CollectionChanged
        ) {
            self.refresh(cx);
            cx.notify();
        }
    }

    /// Pull the draft into the inputs. A focused URL input keeps the text its own edit produced.
    fn resync(&mut self, focused: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (want, method) = {
            let d = &self.tab.read(cx).draft;
            (display_url(&d.url, &d.params), d.method)
        };
        let current = self.input.read(cx).value();
        if take_resync(focused, &mut self.own_last, current.as_ref(), &want) {
            self.input.update(cx, |s, cx| s.set_value(want, window, cx));
        }
        let ix = method_index(method);
        let shown = self.method.read(cx).selected_index(cx);
        if shown != Some(IndexPath::default().row(ix)) {
            self.method.update(cx, |s, cx| {
                s.set_selected_index(Some(IndexPath::default().row(ix)), window, cx)
            });
        }
    }

    /// Recompute the highlight strip and the resolve problems.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let t = self.tab.read(cx);
        let draft = t.draft.clone();
        let path = t.path.clone();
        let folders = folder_names(path.as_deref());
        let ws = self.workspace.read(cx);
        let scope = ws.scope_for(path.as_deref());
        let text = display_url(&draft.url, &draft.params);
        self.segments = highlight::url_spans(&text, &draft, &scope)
            .into_iter()
            .map(|s| {
                let tip = match &s.kind {
                    SpanKind::Var { name, .. } => {
                        Some(highlight::hover_text(name, &draft, &scope, &folders).into())
                    }
                    _ => None,
                };
                Segment {
                    text: text.get(s.range.clone()).unwrap_or("").to_string(),
                    kind: s.kind,
                    tip,
                }
            })
            .collect();
        self.problems = match resolve(&draft, &scope) {
            Ok(_) => Vec::new(),
            Err(e) => e.problems.iter().map(problem_text).collect(),
        };
    }

    fn on_send(&mut self, cx: &mut Context<Self>) {
        let tab = self.tab.clone();
        if tab.read(cx).response.phase == Phase::Sending {
            self.workspace.update(cx, |w, cx| w.cancel(&tab, cx));
        } else {
            // A refused send is already shown as problems under the strip.
            let _ = self.workspace.update(cx, |w, cx| w.send(&tab, cx));
        }
    }
}

impl Render for UrlBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sending = self.tab.read(cx).response.phase == Phase::Sending;
        let theme = cx.theme();
        let mut strip = div()
            .flex()
            .flex_row()
            .w_full()
            .min_w_0()
            .overflow_hidden()
            .px_2()
            .text_sm();
        for (i, seg) in self.segments.iter().enumerate() {
            let color = match &seg.kind {
                SpanKind::Text => theme.foreground,
                SpanKind::Query => theme.muted_foreground,
                SpanKind::PathParam { filled: true, .. } => theme.info,
                SpanKind::PathParam { filled: false, .. } => theme.warning,
                SpanKind::Var { state, .. } => match state {
                    VarState::Resolved => theme.success,
                    VarState::Dynamic => theme.info,
                    VarState::Unresolved | VarState::Error => theme.danger,
                },
            };
            let mut el = div()
                .id(("url-seg", i))
                .whitespace_nowrap()
                .text_color(color)
                .child(seg.text.clone());
            if let Some(tip) = seg.tip.clone() {
                el = el.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx));
            }
            strip = strip.child(el);
        }
        let mut problems = div()
            .flex()
            .flex_col()
            .px_2()
            .text_sm()
            .text_color(theme.danger);
        for p in &self.problems {
            problems = problems.child(p.clone());
        }
        let blocked = !self.problems.is_empty();
        let button = Button::new("url-send")
            .label(if sending { "Cancel" } else { "Send" })
            .disabled(blocked && !sending)
            .on_click(cx.listener(|this, _, _, cx| this.on_send(cx)));
        let button = if sending { button } else { button.primary() };

        div()
            .flex()
            .flex_col()
            .gap_1()
            .py_1()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .child(
                        div()
                            .w(px(110.))
                            .child(Select::new(&self.method).id("url-method")),
                    )
                    .child(div().flex_1().child(Input::new(&self.input)))
                    .child(button),
            )
            .child(strip)
            .child(problems)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn untick_then_retick_while_focused_resyncs_both_times() {
        // Typed `a?x=1&y=2`: the own edit is recorded.
        let mut own = Some("a?x=1&y=2".to_string());
        // Untick y: outside edit overwrites.
        assert!(take_resync(true, &mut own, "a?x=1&y=2", "a?x=1"));
        // Re-tick y: want equals the old record, but must still overwrite.
        assert!(take_resync(true, &mut own, "a?x=1", "a?x=1&y=2"));
    }

    #[test]
    fn should_set_text_cases() {
        // Focused own edit with a trailing `&` is kept.
        assert!(!should_set_text(true, Some("a?x=1"), "a?x=1&", "a?x=1"));
        // Focused, a Params toggle changed the draft: overwrite.
        assert!(should_set_text(
            true,
            Some("a?x=1&y=2"),
            "a?x=1&y=2",
            "a?x=1"
        ));
        // Unfocused and equal: nothing.
        assert!(!should_set_text(false, None, "a", "a"));
        // Unfocused and different: overwrite, even if it matches own_last.
        assert!(should_set_text(false, Some("a"), "a&", "a"));
    }

    #[test]
    fn folders_are_every_directory_component() {
        assert!(folder_names(None).is_empty());
        assert!(folder_names(Some(&PathBuf::from("r.toml"))).is_empty());
        assert_eq!(
            folder_names(Some(&PathBuf::from("users/admin/get.toml"))),
            ["users", "admin"]
        );
    }

    #[test]
    fn problems_read_in_plain_words() {
        let p = Problem::UnresolvedVar {
            name: "orgId".into(),
            location: Location::Url,
        };
        assert_eq!(problem_text(&p), "{{orgId}} is not defined (URL)");
        let p = Problem::MissingPathParam { name: "id".into() };
        assert_eq!(problem_text(&p), "{id} has no value (path)");
        let p = Problem::UnresolvedVar {
            name: "k".into(),
            location: Location::Header("X-Key".into()),
        };
        assert_eq!(problem_text(&p), "{{k}} is not defined (header X-Key)");
    }

    #[test]
    fn method_index_matches_list() {
        assert_eq!(method_index(Method::Get), 0);
        assert_eq!(method_index(Method::Options), 6);
    }
}
