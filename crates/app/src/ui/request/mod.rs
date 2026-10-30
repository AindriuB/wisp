//! Request pane: sub-tab strip over the six editors.

pub mod auth;
pub mod body;
pub mod headers;
pub mod params;
pub mod path;
pub mod settings;

use gpui::{AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Window, div};

use crate::tab::{RequestSubTab, RequestTab};
use crate::workspace::Workspace;

use auth::AuthEditor;
use body::BodyEditor;
use headers::HeadersEditor;
use params::ParamsEditor;
use path::PathEditor;
use settings::SettingsEditor;

/// Placeholder; the owning task fills in the strip and behaviour.
pub struct RequestPane {
    tab: Entity<RequestTab>,
    params: Entity<ParamsEditor>,
    path: Entity<PathEditor>,
    headers: Entity<HeadersEditor>,
    auth: Entity<AuthEditor>,
    body: Entity<BodyEditor>,
    settings: Entity<SettingsEditor>,
}

impl RequestPane {
    pub fn new(
        workspace: Entity<Workspace>,
        tab: Entity<RequestTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (w, t) = (&workspace, &tab);
        Self {
            params: cx.new(|cx| ParamsEditor::new(w.clone(), t.clone(), window, cx)),
            path: cx.new(|cx| PathEditor::new(w.clone(), t.clone(), window, cx)),
            headers: cx.new(|cx| HeadersEditor::new(w.clone(), t.clone(), window, cx)),
            auth: cx.new(|cx| AuthEditor::new(w.clone(), t.clone(), window, cx)),
            body: cx.new(|cx| BodyEditor::new(w.clone(), t.clone(), window, cx)),
            settings: cx.new(|cx| SettingsEditor::new(w.clone(), t.clone(), window, cx)),
            tab,
        }
    }
}

impl Render for RequestPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.tab.read(cx).request_tab;
        let strip = div().child("Params | Path | Headers | Auth | Body | Settings");
        let editor = match active {
            RequestSubTab::Params => div().child(self.params.clone()),
            RequestSubTab::Path => div().child(self.path.clone()),
            RequestSubTab::Headers => div().child(self.headers.clone()),
            RequestSubTab::Auth => div().child(self.auth.clone()),
            RequestSubTab::Body => div().child(self.body.clone()),
            RequestSubTab::Settings => div().child(self.settings.clone()),
        };
        div().child(strip).child(editor)
    }
}
