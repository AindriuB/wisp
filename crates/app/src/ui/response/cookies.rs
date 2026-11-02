//! Cookies sub-tab: `Set-Cookie` headers of this response only.

use gpui::{
    Context, Entity, InteractiveElement as _, IntoElement, ParentElement, Render, Styled,
    Subscription, Window, div, px,
};
use gpui_component::ActiveTheme as _;
use gpui_component::scroll::ScrollableElement as _;

use crate::cookies::{ResponseCookie, parse_set_cookies};
use crate::tab::{RequestTab, TabEvent};
use crate::workspace::Workspace;

pub const NOTE: &str =
    "Cookies from this response only; Wisp does not store or send cookies in this version.";

fn expiry(c: &ResponseCookie) -> String {
    match (&c.expires, c.max_age) {
        (_, Some(n)) => format!("max-age {n}"),
        (Some(e), None) => e.clone(),
        (None, None) => "session".to_string(),
    }
}

fn flags(c: &ResponseCookie) -> String {
    let mut f = Vec::new();
    if c.secure {
        f.push("Secure".to_string());
    }
    if c.http_only {
        f.push("HttpOnly".to_string());
    }
    if let Some(s) = &c.same_site {
        f.push(format!("SameSite={s}"));
    }
    f.join(", ")
}

pub struct CookiesPane {
    tab: Entity<RequestTab>,
    _sub: Subscription,
}

impl CookiesPane {
    pub fn new(
        _workspace: Entity<Workspace>,
        tab: Entity<RequestTab>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let sub = cx.subscribe(&tab, |_, _, event: &TabEvent, cx| {
            if *event == TabEvent::ResponseChanged {
                cx.notify();
            }
        });
        Self { tab, _sub: sub }
    }
}

/// Preferred column widths, shared by the header and every row. Columns grow
/// and shrink from these bases to fit the pane and truncate their text.
const WIDTHS: [f32; 6] = [140., 200., 140., 100., 160., 200.];

fn cell(text: String, w: f32) -> gpui::Div {
    div()
        .flex_basis(px(w))
        .flex_grow(1.)
        .flex_shrink(1.)
        .min_w_0()
        .truncate()
        .child(text)
}

impl Render for CookiesPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cookies = parse_set_cookies(&self.tab.read(cx).response.headers);
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;
        let mut root = div()
            .flex()
            .flex_col()
            .size_full()
            .p_2()
            .gap_1()
            .child(div().text_color(muted).child(NOTE));
        if cookies.is_empty() {
            return root.child(div().child("No cookies in this response"));
        }
        let head = [
            "Name",
            "Value",
            "Domain",
            "Path",
            "Expires / Max-Age",
            "Flags",
        ];
        let widths = WIDTHS;
        let mut header = div().flex().gap_2().border_b_1().border_color(border);
        for (h, w) in head.iter().zip(widths) {
            header = header.child(cell(h.to_string(), w).font_weight(gpui::FontWeight::SEMIBOLD));
        }
        root = root.child(header);
        let mut rows = div()
            .id("cookies-rows")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0();
        for c in &cookies {
            rows = rows.child(
                div()
                    .flex()
                    .gap_2()
                    .py_0p5()
                    .child(cell(c.name.clone(), widths[0]))
                    .child(cell(c.value.clone(), widths[1]))
                    .child(cell(c.domain.clone().unwrap_or_default(), widths[2]))
                    .child(cell(c.path.clone().unwrap_or_default(), widths[3]))
                    .child(cell(expiry(c), widths[4]))
                    .child(cell(flags(c), widths[5])),
            );
        }
        root.child(rows.overflow_y_scrollbar())
    }
}
