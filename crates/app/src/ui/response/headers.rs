use gpui::{Context, Entity, IntoElement, ParentElement, Render, Window, div};

use crate::tab::RequestTab;
use crate::workspace::Workspace;

/// Placeholder; the owning task fills in the behaviour.
pub struct HeadersPane {
    _workspace: Entity<Workspace>,
    _tab: Entity<RequestTab>,
}

impl HeadersPane {
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

impl Render for HeadersPane {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child("HeadersPane")
    }
}
