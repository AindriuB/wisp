//! Applies the configured theme mode.

use gpui::{App, Window};
use gpui_component::theme::{Theme, ThemeMode as ComponentMode};

use crate::settings::ThemeMode;

/// System follows the OS appearance; Light and Dark force that mode.
pub fn apply(mode: ThemeMode, window: Option<&mut Window>, cx: &mut App) {
    match mode {
        ThemeMode::System => Theme::sync_system_appearance(window, cx),
        ThemeMode::Light => Theme::change(ComponentMode::Light, window, cx),
        ThemeMode::Dark => Theme::change(ComponentMode::Dark, window, cx),
    }
}
