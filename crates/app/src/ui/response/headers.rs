//! Headers sub-tab: HTTP version and a two-column table in received order.

use gpui::{
    ClipboardItem, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement, Render, StatefulInteractiveElement as _, Styled, Subscription, Window, div, px,
};
use gpui_component::ActiveTheme as _;
use gpui_component::scroll::ScrollableElement as _;
use wisp_core::HttpVersion;

use crate::tab::{RequestTab, TabEvent};
use crate::workspace::Workspace;

pub fn version_text(v: HttpVersion) -> &'static str {
    match v {
        HttpVersion::Http09 => "HTTP/0.9",
        HttpVersion::Http10 => "HTTP/1.0",
        HttpVersion::Http11 => "HTTP/1.1",
        HttpVersion::Http2 => "HTTP/2",
        HttpVersion::Http3 => "HTTP/3",
    }
}

/// Text copied for a selected header cell: column 0 is the name, anything else the value.
pub fn cell_text(headers: &[wisp_core::Header], row: usize, col: usize) -> Option<String> {
    let h = headers.get(row)?;
    Some(if col == 0 {
        h.name.to_string()
    } else {
        h.value.to_string()
    })
}

trait SelectedStyle: Styled + Sized {
    fn when_selected(self, on: bool, bg: gpui::Hsla) -> Self {
        if on { self.bg(bg) } else { self }
    }
}
impl SelectedStyle for gpui::Stateful<gpui::Div> {}

pub struct HeadersPane {
    tab: Entity<RequestTab>,
    focus: FocusHandle,
    selected: Option<(usize, usize)>,
    _sub: Subscription,
}

impl HeadersPane {
    pub fn new(
        _workspace: Entity<Workspace>,
        tab: Entity<RequestTab>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let sub = cx.subscribe(&tab, |this, _, event: &TabEvent, cx| {
            if *event == TabEvent::ResponseChanged {
                this.selected = None;
                cx.notify();
            }
        });
        Self {
            tab,
            focus: cx.focus_handle(),
            selected: None,
            _sub: sub,
        }
    }
}

impl Render for HeadersPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let response = &self.tab.read(cx).response;
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;
        let mut root = div()
            .id("headers-pane")
            .flex()
            .flex_col()
            .size_full()
            .p_2()
            .gap_1();
        if let Some(v) = response.version {
            root = root.child(div().text_color(muted).child(version_text(v)));
        }
        if response.headers.is_empty() {
            return root.child(div().text_color(muted).child("No headers"));
        }
        let mut table = div()
            .id("headers-rows")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0();
        let selected_bg = cx.theme().selection;
        for (row, h) in response.headers.iter().enumerate() {
            let cell = |col: usize, text: String| {
                let on = self.selected == Some((row, col));
                div()
                    .id(("header-cell", row * 2 + col))
                    .child(text)
                    .when_selected(on, selected_bg)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.selected = Some((row, col));
                        window.focus(&this.focus, cx);
                        cx.notify();
                    }))
            };
            table = table.child(
                div()
                    .flex()
                    .gap_2()
                    .py_0p5()
                    .border_b_1()
                    .border_color(border)
                    .child(
                        cell(0, h.name.to_string())
                            .w(px(220.))
                            .flex_none()
                            .font_weight(gpui::FontWeight::SEMIBOLD),
                    )
                    .child(cell(1, h.value.to_string()).flex_1()),
            );
        }
        // The rows div becomes the scroll area; the bar overlays it.
        let table = table.overflow_y_scrollbar();
        root.track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _, cx| {
                let k = &ev.keystroke;
                if k.key == "c"
                    && (k.modifiers.platform || k.modifiers.control)
                    && let Some((row, col)) = this.selected
                    && let Some(t) = cell_text(&this.tab.read(cx).response.headers, row, col)
                {
                    cx.write_to_clipboard(ClipboardItem::new_string(t));
                }
            }))
            .child(table)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_core::Header;

    fn hs() -> Vec<Header> {
        vec![
            Header {
                name: "A".into(),
                value: "1".into(),
            },
            Header {
                name: "B".into(),
                value: "2".into(),
            },
        ]
    }

    #[test]
    fn cell_text_maps_name_and_value() {
        assert_eq!(cell_text(&hs(), 1, 0).as_deref(), Some("B"));
        assert_eq!(cell_text(&hs(), 1, 1).as_deref(), Some("2"));
        assert_eq!(cell_text(&hs(), 2, 0), None);
    }
}
