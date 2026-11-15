//! Assertions sub-tab (stub; content lands in a later task).

use gpui::{Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div};

use crate::tab::RequestTab;
use crate::workspace::Workspace;

pub struct AssertionsEditor {
    _workspace: Entity<Workspace>,
    _tab: Entity<RequestTab>,
}

impl AssertionsEditor {
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

impl Render for AssertionsEditor {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().p_2().child("Assertions: coming soon")
    }
}
