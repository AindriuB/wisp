use gpui::{Context, Entity, IntoElement, ParentElement, Render, Window, div};

use crate::workspace::Workspace;

/// Placeholder root; mounted by task 28.
pub struct WorkspaceView {
    _workspace: Entity<Workspace>,
}

impl WorkspaceView {
    pub fn new(
        workspace: Entity<Workspace>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            _workspace: workspace,
        }
    }
}

impl Render for WorkspaceView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child("Workspace")
    }
}
