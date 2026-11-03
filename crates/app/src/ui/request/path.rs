//! Path sub-tab: one labelled input per `{name}` in the URL.

use gpui::{
    AppContext as _, Context, Entity, EntityId, IntoElement, ParentElement, Render, Styled,
    Subscription, Window, div, px,
};
use gpui_component::ActiveTheme as _;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement as _;
use wisp_core::template::path_params;

use crate::tab::{RequestTab, TabEvent};
use crate::workspace::Workspace;

struct Slot {
    name: String,
    label: String,
    input: Entity<InputState>,
    _sub: Subscription,
}

pub struct PathEditor {
    _workspace: Entity<Workspace>,
    tab: Entity<RequestTab>,
    slots: Vec<Slot>,
    _sub: Subscription,
}

impl PathEditor {
    pub fn new(
        workspace: Entity<Workspace>,
        tab: Entity<RequestTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let sub = cx.subscribe_in(&tab, window, Self::on_tab);
        let mut this = Self {
            _workspace: workspace,
            tab,
            slots: Vec::new(),
            _sub: sub,
        };
        this.sync(true, window, cx);
        this
    }

    fn on_tab(
        &mut self,
        _: &Entity<RequestTab>,
        event: &TabEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TabEvent::DraftReplaced => self.sync(true, window, cx),
            TabEvent::DraftEdited => self.sync(false, window, cx),
            _ => {}
        }
    }

    /// Match the inputs to `draft.path`; existing inputs for surviving names are kept.
    /// Values are written into the inputs only when `write_values` (a reload) or the input is new.
    fn sync(&mut self, write_values: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (vars, url) = {
            let d = &self.tab.read(cx).draft;
            (d.path.clone(), d.url.clone())
        };
        let params = path_params(&url);
        let mut old: Vec<Slot> = std::mem::take(&mut self.slots);
        for v in vars {
            let label = match params.iter().find(|p| p.name == v.name) {
                Some(p) if p.catch_all => format!("{{*{}}}", v.name),
                _ => format!("{{{}}}", v.name),
            };
            let reuse = old.iter().position(|s| s.name == v.name);
            let slot = match reuse {
                Some(i) => {
                    let mut s = old.remove(i);
                    s.label = label;
                    if write_values && s.input.read(cx).value().as_ref() != v.value {
                        let val = v.value.clone();
                        s.input.update(cx, |i, cx| i.set_value(val, window, cx));
                    }
                    s
                }
                None => {
                    let val = v.value.clone();
                    let input = cx.new(|cx| InputState::new(window, cx).default_value(val));
                    let sub = cx.subscribe(&input, Self::on_input);
                    Slot {
                        name: v.name.clone(),
                        label,
                        input,
                        _sub: sub,
                    }
                }
            };
            self.slots.push(slot);
        }
        cx.notify();
    }

    fn on_input(&mut self, input: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let id: EntityId = input.entity_id();
        let Some(name) = self
            .slots
            .iter()
            .find(|s| s.input.entity_id() == id)
            .map(|s| s.name.clone())
        else {
            return;
        };
        let text = input.read(cx).value().to_string();
        let differs = self
            .tab
            .read(cx)
            .draft
            .path
            .iter()
            .any(|v| v.name == name && v.value != text);
        if differs {
            self.tab.update(cx, |t, cx| {
                t.edit(
                    |d| {
                        if let Some(v) = d.path.iter_mut().find(|v| v.name == name) {
                            v.value = text;
                        }
                    },
                    cx,
                )
            });
        }
    }
}

impl Render for PathEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut col = div().flex().flex_col().gap_1().p_2();
        if self.slots.is_empty() {
            return div().size_full().child(
                col.size_full()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("No path parameters. Put {name} in the URL path to add one."),
                    )
                    .overflow_y_scrollbar(),
            );
        }
        for s in &self.slots {
            col = col.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(140.)).text_sm().child(s.label.clone()))
                    .child(div().flex_1().child(Input::new(&s.input))),
            );
        }
        div()
            .size_full()
            .child(col.size_full().overflow_y_scrollbar())
    }
}
