use gpui::{Context, Entity, IntoElement, ParentElement, Render, Window, div};

use crate::workspace::Workspace;

/// Placeholder; the owning task fills in the behaviour.
pub struct Sidebar {
    _workspace: Entity<Workspace>,
}

impl Sidebar {
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

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child("Sidebar")
    }
}
