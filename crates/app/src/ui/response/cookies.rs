use gpui::{Context, Entity, IntoElement, ParentElement, Render, Window, div};

use crate::tab::RequestTab;
use crate::workspace::Workspace;

/// Placeholder; the owning task fills in the behaviour.
pub struct CookiesPane {
    _workspace: Entity<Workspace>,
    _tab: Entity<RequestTab>,
}

impl CookiesPane {
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

impl Render for CookiesPane {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child("CookiesPane")
    }
}
