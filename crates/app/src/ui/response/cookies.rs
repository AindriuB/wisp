//! Cookies sub-tab: `Set-Cookie` headers of this response only.

use gpui::{
    Context, Entity, InteractiveElement as _, IntoElement, ParentElement, Render,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div, px,
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

/// Column names, in display order.
const HEAD: [&str; 6] = [
    "Name",
    "Value",
    "Domain",
    "Path",
    "Expires / Max-Age",
    "Flags",
];

/// Preferred column widths, shared by the header and every row. Columns grow
/// and shrink from these bases to fit the pane and truncate their text.
const WIDTHS: [f32; 6] = [140., 200., 140., 100., 160., 256.];

/// Narrowest each column may get. Below their sum the table scrolls sideways.
const MIN_WIDTHS: [f32; 6] = [60., 56., 56., 36., 68., 256.];

/// Right gutter reserved by the header and every row for the vertical scrollbar.
const GUTTER: f32 = 12.;

/// Column gap (`gap_2`) in pixels.
const GAP: f32 = 8.;

/// Minimum width of the whole table: columns, gaps and the scrollbar gutter.
fn min_table_width() -> f32 {
    MIN_WIDTHS.iter().sum::<f32>() + GAP * (MIN_WIDTHS.len() - 1) as f32 + GUTTER
}

fn cell(text: String, i: usize) -> gpui::Div {
    div()
        .flex_basis(px(WIDTHS[i]))
        .flex_grow(1.)
        .flex_shrink(1.)
        .min_w(px(MIN_WIDTHS[i]))
        .truncate()
        .child(text)
}

impl Render for CookiesPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cookies = parse_set_cookies(&self.tab.read(cx).response.headers);
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;
        let root = div()
            .flex()
            .flex_col()
            .size_full()
            .p_2()
            .gap_1()
            .child(div().text_color(muted).child(NOTE));
        if cookies.is_empty() {
            return root.child(div().child("No cookies in this response"));
        }
        let mut header = div()
            .flex()
            .gap(px(GAP))
            .pr(px(GUTTER))
            .border_b_1()
            .border_color(border);
        for (i, h) in HEAD.iter().enumerate() {
            header = header.child(cell(h.to_string(), i).font_weight(gpui::FontWeight::SEMIBOLD));
        }
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
                    .gap(px(GAP))
                    .py_0p5()
                    .pr(px(GUTTER))
                    .child(cell(c.name.clone(), 0))
                    .child(cell(c.value.clone(), 1))
                    .child(cell(c.domain.clone().unwrap_or_default(), 2))
                    .child(cell(c.path.clone().unwrap_or_default(), 3))
                    .child(cell(expiry(c), 4))
                    .child(cell(flags(c), 5)),
            );
        }
        let table = div()
            .flex()
            .flex_col()
            .h_full()
            .w_full()
            .min_w(px(min_table_width()))
            .child(header)
            .child(rows.overflow_y_scrollbar());
        root.child(
            div()
                .id("cookies-scroll")
                .flex_1()
                .min_h_0()
                .overflow_x_scroll()
                .restrict_scroll_to_axis()
                .child(table),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_table_width_sums_columns_gaps_and_gutter() {
        assert_eq!(min_table_width(), 532. + 40. + 12.);
    }

    #[test]
    fn minimums_never_exceed_preferred() {
        for (m, w) in MIN_WIDTHS.iter().zip(WIDTHS) {
            assert!(*m <= w);
        }
    }

    #[test]
    fn table_fits_a_600px_pane_and_still_scrolls_at_200px() {
        // 600 px pane less the root's `p_2` padding (8 px each side).
        assert!(min_table_width() <= 584.);
        assert!(min_table_width() > 200.);
    }

    #[test]
    fn flags_minimum_fits_the_longest_flag_string() {
        // "Secure, HttpOnly, SameSite=Strict" measured with AppKit in the
        // SF system font at 16 px (regular weight): 249.45 px, rounded up.
        const FLAGS_TEXT_PX: f32 = 250.;
        const { assert!(MIN_WIDTHS[5] >= FLAGS_TEXT_PX) };
    }
}
