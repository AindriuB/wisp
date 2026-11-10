//! Response pane: status line and sub-tab strip over body, headers and cookies.

pub mod body;
pub mod cookies;
pub mod headers;

use std::time::Duration;

use gpui::{
    AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement, Render,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div,
};
use gpui_component::button::Button;
use gpui_component::spinner::Spinner;
use gpui_component::tab::{Tab, TabBar};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme as _, Sizable as _};
use wisp_core::{EngineError, ErrorKind};

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
        ErrorKind::Auth => "Authorization failed",
        ErrorKind::Other => "Request failed",
    }
}

/// Full text of a failure: the heading, then the message when there is one.
pub fn error_text(e: Option<&EngineError>) -> String {
    match e {
        Some(e) if !e.message.is_empty() => format!("{}: {}", error_words(e.kind), e.message),
        Some(e) => error_words(e.kind).to_string(),
        None => "Request failed".to_string(),
    }
}

/// The Done line's timing segment; the total is left out when not recorded.
fn timing_text(total: Option<String>, size: &str) -> String {
    match total {
        Some(t) => format!("· {t} · {size}"),
        None => format!("· {size}"),
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
                let total = r.total.map(format_duration);
                let size = format_size(r.size.unwrap_or(r.received));
                let status_str = status_text(status, r.reason.as_deref());
                let timing = timing_text(total, &size);
                let phases = phase_line(r.dns, r.connect, r.ttfb, r.total);
                let full = done_tooltip(&status_str, &timing, phases);
                let line = row()
                    .id("status-line")
                    .min_w_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(color)
                            .child(status_str),
                    )
                    .child(div().min_w_0().truncate().child(timing))
                    .tooltip(move |window, cx| Tooltip::new(full.clone()).build(window, cx));
                Some(line.into_any_element())
            }
            Phase::Failed => {
                let (words, msg) = r
                    .error
                    .as_ref()
                    .map(|e| (error_words(e.kind), e.message.clone()))
                    .unwrap_or(("Request failed", String::new()));
                let muted = theme.muted_foreground;
                let full = error_text(r.error.as_ref());
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
        let origin = self.tab.read(cx).from_history.clone();
        if let Some(origin) = origin.filter(|_| response.request_id.is_none()) {
            let muted = cx.theme().muted_foreground;
            root = root.child(div().px_2().text_color(muted).child(
                crate::ui::history::origin_line(&origin, crate::ui::history::now_ms()),
            ));
        }
        if let Some(line) = self.status_line(&response, cx) {
            root = root.child(line);
        }
        root.child(strip)
            .child(div().flex_1().min_h_0().child(pane))
    }
}

/// `DNS 3 ms · Connect+TLS 41 ms · First byte 88 ms · Total 95 ms`, leaving out every phase
/// that is `None` (a reused connection has no DNS or connect); empty when none is known.
fn phase_line(
    dns: Option<Duration>,
    connect: Option<Duration>,
    ttfb: Option<Duration>,
    total: Option<Duration>,
) -> String {
    [
        ("DNS", dns),
        ("Connect+TLS", connect),
        ("First byte", ttfb),
        ("Total", total),
    ]
    .into_iter()
    .filter_map(|(name, d)| d.map(|d| format!("{name} {}", format_duration(d))))
    .collect::<Vec<_>>()
    .join(" · ")
}

/// Full text of the Done status line for its tooltip; the phase line goes on a second line.
fn done_tooltip(status: &str, timing: &str, phases: String) -> String {
    if phases.is_empty() {
        format!("{status} {timing}")
    } else {
        format!("{status} {timing}\n{phases}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Option<Duration> {
        Some(Duration::from_millis(n))
    }

    #[test]
    fn phase_line_with_every_phase() {
        assert_eq!(
            phase_line(ms(3), ms(41), ms(88), ms(95)),
            "DNS 3 ms · Connect+TLS 41 ms · First byte 88 ms · Total 95 ms"
        );
    }

    #[test]
    fn phase_line_for_a_reused_connection_has_no_dns_or_connect() {
        assert_eq!(
            phase_line(None, None, ms(88), ms(95)),
            "First byte 88 ms · Total 95 ms"
        );
    }

    #[test]
    fn phase_line_total_only_and_none() {
        assert_eq!(phase_line(None, None, None, ms(95)), "Total 95 ms");
        assert_eq!(phase_line(None, None, None, None), "");
    }

    #[test]
    fn done_tooltip_with_and_without_phases() {
        assert_eq!(
            done_tooltip(
                "500 Internal Server Error",
                "· 12 ms · 1.2 KB",
                "First byte 5 ms · Total 12 ms".into()
            ),
            "500 Internal Server Error · 12 ms · 1.2 KB\nFirst byte 5 ms · Total 12 ms"
        );
        assert_eq!(
            done_tooltip("200 OK", "· 12 ms · 1.2 KB", String::new()),
            "200 OK · 12 ms · 1.2 KB"
        );
    }

    #[test]
    fn error_text_cases() {
        let e = |m: &str| EngineError {
            kind: ErrorKind::Connect,
            message: m.into(),
        };
        assert_eq!(
            error_text(Some(&e("error sending request: client error: refused"))),
            "Connection failed: error sending request: client error: refused"
        );
        assert_eq!(error_text(Some(&e(""))), "Connection failed");
        assert_eq!(error_text(None), "Request failed");
    }

    #[test]
    fn timing_text_with_and_without_total() {
        assert_eq!(
            timing_text(Some("12 ms".into()), "1.2 KB"),
            "· 12 ms · 1.2 KB"
        );
        assert_eq!(timing_text(None, "1.2 KB"), "· 1.2 KB");
    }

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
        assert_eq!(error_words(ErrorKind::Auth), "Authorization failed");
    }
}
