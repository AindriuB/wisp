use gpui::{Context, Entity, IntoElement, ParentElement, Render, Window, div};

use crate::tab::RequestTab;
use crate::workspace::Workspace;

/// Placeholder; the owning task fills in the behaviour.
pub struct UrlBar {
    _workspace: Entity<Workspace>,
    _tab: Entity<RequestTab>,
}

impl UrlBar {
    pub fn new(
        workspace: Entity<Workspace>,
        tab: Entity<RequestTab>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            _workspace: workspace,
            _tab: tab,
        }
    }
}

impl Render for UrlBar {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child("URL bar")
    }
}
