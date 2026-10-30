//! Response pane: sub-tab strip over body, headers and cookies.

pub mod body;
pub mod cookies;
pub mod headers;

use gpui::{AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Window, div};

use crate::tab::{RequestTab, ResponseSubTab};
use crate::workspace::Workspace;

use body::BodyPane;
use cookies::CookiesPane;
use headers::HeadersPane;

/// Placeholder; the owning task fills in the strip and behaviour.
pub struct ResponsePane {
    tab: Entity<RequestTab>,
    body: Entity<BodyPane>,
    headers: Entity<HeadersPane>,
    cookies: Entity<CookiesPane>,
}

impl ResponsePane {
    pub fn new(
        workspace: Entity<Workspace>,
        tab: Entity<RequestTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (w, t) = (&workspace, &tab);
        Self {
            body: cx.new(|cx| BodyPane::new(w.clone(), t.clone(), window, cx)),
            headers: cx.new(|cx| HeadersPane::new(w.clone(), t.clone(), window, cx)),
            cookies: cx.new(|cx| CookiesPane::new(w.clone(), t.clone(), window, cx)),
            tab,
        }
    }
}

impl Render for ResponsePane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.tab.read(cx).response_tab;
        let strip = div().child("Body | Headers | Cookies");
        let pane = match active {
            ResponseSubTab::Body => div().child(self.body.clone()),
            ResponseSubTab::Headers => div().child(self.headers.clone()),
            ResponseSubTab::Cookies => div().child(self.cookies.clone()),
        };
        div().child(strip).child(pane)
    }
}
