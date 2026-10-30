//! Keyboard actions and their default bindings.

use gpui::{App, KeyBinding, actions};

actions!(
    wisp,
    [
        SendRequest,
        CancelRequest,
        SaveRequest,
        NewRequest,
        CloseTab,
        NextTab,
        PrevTab,
        OpenPalette,
        SwitchEnvironment,
        OpenCollection,
        ReloadFromDisk,
        KeepMine,
        ToggleTheme,
        ToggleLayout
    ]
);

/// Bind the default keys. "secondary" is cmd on macOS and ctrl on Windows/Linux.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-enter", SendRequest, None),
        KeyBinding::new("secondary-s", SaveRequest, None),
        KeyBinding::new("secondary-k", OpenPalette, None),
        KeyBinding::new("secondary-e", SwitchEnvironment, None),
        KeyBinding::new("secondary-t", NewRequest, None),
        KeyBinding::new("secondary-w", CloseTab, None),
        KeyBinding::new("secondary-o", OpenCollection, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PrevTab, None),
        KeyBinding::new("escape", CancelRequest, None),
    ]);
}
