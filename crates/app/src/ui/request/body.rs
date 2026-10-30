use gpui::{Context, Entity, IntoElement, ParentElement, Render, Window, div};

use crate::tab::RequestTab;
use crate::workspace::Workspace;

/// Placeholder; the owning task fills in the behaviour.
pub struct BodyEditor {
    _workspace: Entity<Workspace>,
    _tab: Entity<RequestTab>,
}

impl BodyEditor {
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

impl Render for BodyEditor {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child("BodyEditor")
    }
}
