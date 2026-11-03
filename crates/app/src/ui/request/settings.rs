//! Settings sub-tab: timeout, redirects and TLS verification.

use std::time::Duration;

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription,
    Window, div, px,
};
use gpui_component::ActiveTheme as _;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::switch::Switch;

use crate::tab::{RequestTab, TabEvent};
use crate::workspace::Workspace;

/// `Some(None)` for empty (use the default), `Some(Some(d))` for a positive
/// number of milliseconds, `None` for anything else.
pub fn parse_timeout(text: &str) -> Option<Option<Duration>> {
    let t = text.trim();
    if t.is_empty() {
        return Some(None);
    }
    match t.parse::<u64>() {
        Ok(ms) if ms > 0 => Some(Some(Duration::from_millis(ms))),
        _ => None,
    }
}

fn timeout_text(timeout: Option<Duration>) -> String {
    timeout.map_or(String::new(), |d| d.as_millis().to_string())
}

pub struct SettingsEditor {
    _workspace: Entity<Workspace>,
    tab: Entity<RequestTab>,
    timeout: Entity<InputState>,
    _subs: Vec<Subscription>,
}

impl SettingsEditor {
    pub fn new(
        workspace: Entity<Workspace>,
        tab: Entity<RequestTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let text = timeout_text(tab.read(cx).draft.settings.timeout);
        let timeout = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("default")
                .default_value(text)
        });
        let subs = vec![
            cx.subscribe(&timeout, Self::on_timeout),
            cx.subscribe_in(&tab, window, Self::on_tab),
        ];
        Self {
            _workspace: workspace,
            tab,
            timeout,
            _subs: subs,
        }
    }

    fn on_timeout(
        &mut self,
        input: Entity<InputState>,
        event: &InputEvent,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let Some(parsed) = parse_timeout(input.read(cx).value().as_ref()) else {
            return;
        };
        if self.tab.read(cx).draft.settings.timeout != parsed {
            self.tab
                .update(cx, |t, cx| t.edit(|d| d.settings.timeout = parsed, cx));
        }
    }

    fn on_tab(
        &mut self,
        _: &Entity<RequestTab>,
        event: &TabEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TabEvent::DraftReplaced => {
                let text = timeout_text(self.tab.read(cx).draft.settings.timeout);
                self.timeout
                    .update(cx, |i, cx| i.set_value(text, window, cx));
                cx.notify();
            }
            TabEvent::DraftEdited => cx.notify(),
            _ => {}
        }
    }
}

impl Render for SettingsEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.tab.read(cx).draft.settings.clone();
        let label = |text: &'static str| div().w(px(160.)).text_sm().child(text);
        let col =
            div()
                .flex()
                .flex_col()
                .gap_2()
                .p_2()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .child(label("Timeout (ms)"))
                        .child(div().w(px(160.)).child(Input::new(&self.timeout)))
                        .child(
                            div()
                                .pl_2()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("empty = default"),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .child(label("Follow redirects"))
                        .child(
                            Switch::new("follow-redirects")
                                .checked(s.follow_redirects)
                                .on_click(cx.listener(|this, on: &bool, _, cx| {
                                    let on = *on;
                                    this.tab.update(cx, |t, cx| {
                                        t.edit(|d| d.settings.follow_redirects = on, cx)
                                    });
                                })),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .child(label("Verify TLS"))
                        .child(Switch::new("verify-tls").checked(s.verify_tls).on_click(
                            cx.listener(|this, on: &bool, _, cx| {
                                let on = *on;
                                this.tab
                                    .update(cx, |t, cx| t.edit(|d| d.settings.verify_tls = on, cx));
                            }),
                        )),
                );
        div()
            .size_full()
            .child(col.size_full().overflow_y_scrollbar())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_parsing() {
        assert_eq!(parse_timeout(""), Some(None));
        assert_eq!(parse_timeout(" "), Some(None));
        assert_eq!(
            parse_timeout("1500"),
            Some(Some(Duration::from_millis(1500)))
        );
        assert_eq!(parse_timeout("0"), None);
        assert_eq!(parse_timeout("abc"), None);
        assert_eq!(parse_timeout("-5"), None);
    }
}
