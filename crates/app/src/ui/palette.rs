use gpui::{
    Context, DismissEvent, Entity, EventEmitter, IntoElement, ParentElement, Render, Window, div,
};

use crate::palette::PaletteMode;
use crate::workspace::Workspace;

/// Placeholder; the owning task fills in the behaviour.
pub struct PaletteView {
    _workspace: Entity<Workspace>,
    _mode: PaletteMode,
}

impl PaletteView {
    pub fn new(
        workspace: Entity<Workspace>,
        mode: PaletteMode,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            _workspace: workspace,
            _mode: mode,
        }
    }
}

impl EventEmitter<DismissEvent> for PaletteView {}

impl Render for PaletteView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child("Palette")
    }
}
