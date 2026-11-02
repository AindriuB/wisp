//! Response pane: status line and sub-tab strip over body, headers and cookies.

pub mod body;
pub mod cookies;
pub mod headers;

use gpui::{
    AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement, Render,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div,
};
use gpui_component::button::Button;
use gpui_component::spinner::Spinner;
use gpui_component::tab::{Tab, TabBar};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme as _, Sizable as _};
use wisp_core::ErrorKind;

use crate::format::{format_duration, format_size, status_text};
use crate::tab::{Phase, RequestTab, ResponseState, ResponseSubTab, TabEvent};
use crate::workspace::Workspace;

use body::BodyPane;
use cookies::CookiesPane;
use headers::HeadersPane;

const SUB_TABS: [(ResponseSubTab, &str); 3] = [
    (ResponseSubTab::Body, "Body"),
    (ResponseSubTab::Headers, "Headers"),
    (ResponseSubTab::Cookies, "Cookies"),
];

pub fn error_words(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::InvalidUrl => "Invalid URL",
        ErrorKind::Dns => "DNS lookup failed",
        ErrorKind::Connect => "Connection failed",
        ErrorKind::Tls => "TLS error",
        ErrorKind::Timeout => "Timed out",
        ErrorKind::Body => "Body error",
        ErrorKind::BodyFile => "Body file error",
        ErrorKind::Other => "Request failed",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusClass {
    Success,
    Redirect,
    ClientError,
    ServerError,
    Other,
}

fn status_class(status: u16) -> StatusClass {
    match status {
        200..=299 => StatusClass::Success,
        300..=399 => StatusClass::Redirect,
        400..=499 => StatusClass::ClientError,
        500..=599 => StatusClass::ServerError,
        _ => StatusClass::Other,
    }
}

pub struct ResponsePane {
    workspace: Entity<Workspace>,
    tab: Entity<RequestTab>,
    body: Entity<BodyPane>,
    headers: Entity<HeadersPane>,
    cookies: Entity<CookiesPane>,
    _sub: Subscription,
}

impl ResponsePane {
    pub fn new(
        workspace: Entity<Workspace>,
        tab: Entity<RequestTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (w, t) = (&workspace, &tab);
        let sub = cx.subscribe(&tab, |_, _, event: &TabEvent, cx| {
            if *event == TabEvent::ResponseChanged {
                cx.notify();
            }
        });
        Self {
            body: cx.new(|cx| BodyPane::new(w.clone(), t.clone(), window, cx)),
            headers: cx.new(|cx| HeadersPane::new(w.clone(), t.clone(), window, cx)),
            cookies: cx.new(|cx| CookiesPane::new(w.clone(), t.clone(), window, cx)),
            workspace,
            tab,
            _sub: sub,
        }
    }

    fn status_line(&self, r: &ResponseState, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let row = || div().flex().items_center().gap_2().px_2().py_1();
        let theme = cx.theme();
        match r.phase {
            Phase::Idle => None,
            Phase::Sending => {
                let ws = self.workspace.clone();
                let tab = self.tab.clone();
                Some(
                    row()
                        .child(Spinner::new().small())
                        .child(format!("Receiving {}", format_size(r.received)))
                        .child(Button::new("cancel-send").label("Cancel").small().on_click(
                            move |_, _, cx| {
                                ws.update(cx, |w, cx| w.cancel(&tab, cx));
                            },
                        ))
                        .into_any_element(),
                )
            }
            Phase::Done => {
                let status = r.status.unwrap_or(0);
                let color = match status_class(status) {
                    StatusClass::Success => theme.success,
                    StatusClass::Redirect => theme.info,
                    StatusClass::ClientError => theme.warning,
                    StatusClass::ServerError => theme.danger,
                    StatusClass::Other => theme.muted_foreground,
                };
                let total = r.total.map(format_duration).unwrap_or_default();
                let size = format_size(r.size.unwrap_or(r.received));
                let tip = r
                    .ttfb
                    .map(|t| format!("Time to first byte: {}", format_duration(t)));
                let line = row()
                    .id("status-line")
                    .child(
                        div()
                            .text_color(color)
                            .child(status_text(status, r.reason.as_deref())),
                    )
                    .child(format!("· {total} · {size}"));
                Some(match tip {
                    Some(tip) => line
                        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                        .into_any_element(),
                    None => line.into_any_element(),
                })
            }
            Phase::Failed => {
                let (words, msg) = r
                    .error
                    .as_ref()
                    .map(|e| (error_words(e.kind), e.message.clone()))
                    .unwrap_or(("Request failed", String::new()));
                let muted = theme.muted_foreground;
                let full = format!("{words} {msg}").trim_end().to_string();
                Some(
                    row()
                        .id("status-line")
                        .min_w_0()
                        .overflow_hidden()
                        .text_color(theme.danger)
                        .child(div().flex_none().child(words))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .text_color(muted)
                                .child(msg),
                        )
                        .tooltip(move |window, cx| Tooltip::new(full.clone()).build(window, cx))
                        .into_any_element(),
                )
            }
            Phase::Cancelled => Some(row().child("Cancelled").into_any_element()),
        }
    }
}

impl Render for ResponsePane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (active, response) = {
            let t = self.tab.read(cx);
            (t.response_tab, t.response.clone())
        };
        let tab = self.tab.clone();
        let strip = TabBar::new("response-tabs")
            .selected_index(SUB_TABS.iter().position(|(s, _)| *s == active).unwrap_or(0))
            .on_click(move |ix, _, cx| {
                let sub = SUB_TABS[*ix].0;
                tab.update(cx, |t, cx| {
                    t.response_tab = sub;
                    cx.emit(TabEvent::ResponseChanged);
                });
            })
            .children(SUB_TABS.iter().map(|(_, label)| Tab::new().label(*label)));
        let pane = match active {
            ResponseSubTab::Body => self.body.clone().into_any_element(),
            ResponseSubTab::Headers => self.headers.clone().into_any_element(),
            ResponseSubTab::Cookies => self.cookies.clone().into_any_element(),
        };
        let mut root = div().flex().flex_col().size_full();
        if let Some(line) = self.status_line(&response, cx) {
            root = root.child(line);
        }
        root.child(strip)
            .child(div().flex_1().min_h_0().child(pane))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_classes() {
        assert_eq!(status_class(204), StatusClass::Success);
        assert_eq!(status_class(302), StatusClass::Redirect);
        assert_eq!(status_class(404), StatusClass::ClientError);
        assert_eq!(status_class(503), StatusClass::ServerError);
        assert_eq!(status_class(100), StatusClass::Other);
    }

    #[test]
    fn errors_in_words() {
        assert_eq!(error_words(ErrorKind::Dns), "DNS lookup failed");
        assert_eq!(error_words(ErrorKind::Timeout), "Timed out");
    }
}
