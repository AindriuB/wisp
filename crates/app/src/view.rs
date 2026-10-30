//! The single window view, wired to the core `Engine`.

use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled, Window, div,
    prelude::*, px,
};
use gpui_component::button::Button;
use gpui_component::input::{Input, InputState};
use gpui_component::select::{Select, SelectState};
use gpui_component::{ActiveTheme as _, IndexPath};
use wisp_core::body::BodyHandle;
use wisp_core::{
    ClientSettings, Command, Engine, EngineConfig, Event, EventReceiver, Method, PreparedBody,
    PreparedRequest, RequestId,
};

use crate::format::{BODY_LIMIT, body_text, completed_line};

const METHODS: [(&str, Method); 7] = [
    ("GET", Method::Get),
    ("POST", Method::Post),
    ("PUT", Method::Put),
    ("PATCH", Method::Patch),
    ("DELETE", Method::Delete),
    ("HEAD", Method::Head),
    ("OPTIONS", Method::Options),
];

/// Startup checkpoint on stderr, only when the measurement hook is active, so a
/// hang can be located from the captured stderr.
pub fn trace(stage: &str) {
    if std::env::var_os("WISP_AUTOSEND_URL").is_some() {
        eprintln!("WISP_TRACE {stage}");
    }
}

pub struct WispView {
    engine: Engine,
    method: Entity<SelectState<Vec<&'static str>>>,
    url: Entity<InputState>,
    status: SharedString,
    body: SharedString,
    autosend: Option<RequestId>,
    latest: Option<RequestId>,
    /// Body currently shown; released when a new response replaces it.
    shown: Option<BodyHandle>,
    status_line: Option<(u16, Option<String>)>,
}

impl WispView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (engine, rx) = Engine::start(EngineConfig::default());
        let names: Vec<&'static str> = METHODS.iter().map(|(n, _)| *n).collect();
        let method = cx.new(|cx| SelectState::new(names, Some(IndexPath::default()), window, cx));
        let url = cx.new(|cx| InputState::new(window, cx).placeholder("https://example.com"));
        let mut this = Self {
            engine,
            method,
            url,
            status: "Ready".into(),
            body: SharedString::default(),
            autosend: None,
            latest: None,
            shown: None,
            status_line: None,
        };
        this.spawn_drain(rx, cx);
        if let Ok(target) = std::env::var("WISP_AUTOSEND_URL") {
            let id = this.submit(Method::Get, target);
            this.autosend = Some(id);
            this.latest = Some(id);
            trace("autosend submitted");
        }
        trace("window opened");
        this
    }

    /// One background task drains the event channel; the UI thread only awaits.
    fn spawn_drain(&self, rx: EventReceiver, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let mut first = true;
            while let Ok(event) = rx.recv().await {
                if std::mem::take(&mut first) {
                    trace("first event received");
                }
                let alive = this.update(cx, |this, cx| this.on_event(event, cx));
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn submit(&self, method: Method, url: String) -> RequestId {
        self.engine.submit(Command::Send(PreparedRequest {
            method,
            url,
            headers: Vec::new(),
            body: PreparedBody::None,
            timeout: None,
            client: ClientSettings {
                verify_tls: true,
                follow_redirects: true,
                proxy: None,
            },
            warnings: Vec::new(),
        }))
    }

    fn selected_method(&self, cx: &gpui::App) -> Method {
        let name = self.method.read(cx).selected_value().copied();
        METHODS
            .iter()
            .find(|(n, _)| Some(*n) == name)
            .map_or(Method::Get, |(_, m)| *m)
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let url = self.url.read(cx).value().trim().to_string();
        let method = self.selected_method(cx);
        self.status = "Sending…".into();
        self.latest = Some(self.submit(method, url));
        cx.notify();
    }

    fn release_shown(&mut self) {
        if let Some(h) = self.shown.take() {
            self.engine.bodies().release(&h);
        }
    }

    fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        let id = match &event {
            Event::Started { id, .. }
            | Event::Headers { id, .. }
            | Event::Progress { id, .. }
            | Event::Completed { id, .. }
            | Event::Failed { id, .. }
            | Event::Cancelled { id } => *id,
        };
        if self.latest != Some(id) && self.autosend != Some(id) {
            // A superseded response may still complete; free its body.
            if let Event::Completed { body, .. } = &event {
                self.engine.bodies().release(body);
            }
            return;
        }
        match event {
            Event::Started { .. } | Event::Progress { .. } => return,
            Event::Headers { status, reason, .. } => {
                self.status_line = Some((status, reason));
                return;
            }
            Event::Completed {
                id,
                total,
                size,
                body,
                ..
            } => {
                let (status, reason) = self.status_line.take().unwrap_or((0, None));
                self.status = completed_line(status, reason.as_deref(), total, size).into();
                let read = self.engine.bodies().read(&body, 0, BODY_LIMIT + 1);
                self.release_shown();
                match read {
                    Ok(bytes) => self.body = body_text(&bytes).into(),
                    Err(e) => {
                        self.status = format!("{} (body unreadable: {e})", self.status).into();
                        self.body = SharedString::default();
                    }
                }
                self.shown = Some(body);
                if self.autosend == Some(id) {
                    println!("WISP_EVENT completed status={status} size={size}");
                }
            }
            Event::Failed { id, error } => {
                self.status = format!("Error ({:?}): {}", error.kind, error.message).into();
                self.release_shown();
                self.body = SharedString::default();
                if self.autosend == Some(id) {
                    println!("WISP_EVENT failed kind={:?}", error.kind);
                }
            }
            Event::Cancelled { .. } => {
                self.status = "Cancelled".into();
            }
        }
        cx.notify();
    }
}

impl Render for WispView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bar = div()
            .flex()
            .flex_row()
            .gap_2()
            .p_2()
            .child(div().w(px(120.)).child(Select::new(&self.method)))
            .child(div().flex_1().child(Input::new(&self.url)))
            .child(
                Button::new("send")
                    .label("Send")
                    .on_click(cx.listener(|this, _, _, cx| this.send(cx))),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(bar)
            .child(div().px_2().pb_2().child(self.status.clone()))
            .child(
                div()
                    .id("body")
                    .flex_1()
                    .min_h_0()
                    .p_2()
                    .overflow_y_scroll()
                    .child(self.body.clone()),
            )
    }
}
