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

/// Every default binding as (keystroke, action name), with the primary modifier
/// (`cmd` on macOS, `ctrl` elsewhere) spelled out per platform.
#[cfg(target_os = "macos")]
const KEYS: [(&str, &str); 10] = [
    ("cmd-enter", "send"),
    ("cmd-s", "save"),
    ("cmd-k", "palette"),
    ("cmd-e", "env"),
    ("cmd-t", "new"),
    ("cmd-w", "close"),
    ("cmd-o", "open"),
    ("ctrl-tab", "next"),
    ("ctrl-shift-tab", "prev"),
    ("escape", "cancel"),
];
#[cfg(not(target_os = "macos"))]
const KEYS: [(&str, &str); 10] = [
    ("ctrl-enter", "send"),
    ("ctrl-s", "save"),
    ("ctrl-k", "palette"),
    ("ctrl-e", "env"),
    ("ctrl-t", "new"),
    ("ctrl-w", "close"),
    ("ctrl-o", "open"),
    ("ctrl-tab", "next"),
    ("ctrl-shift-tab", "prev"),
    ("escape", "cancel"),
];

/// The keystroke bound to `name` (see `KEYS`); the order below mirrors it.
const fn key(index: usize) -> &'static str {
    KEYS[index].0
}

/// Bind the default keys.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(key(0), SendRequest, None),
        KeyBinding::new(key(1), SaveRequest, None),
        KeyBinding::new(key(2), OpenPalette, None),
        KeyBinding::new(key(3), SwitchEnvironment, None),
        KeyBinding::new(key(4), NewRequest, None),
        KeyBinding::new(key(5), CloseTab, None),
        KeyBinding::new(key(6), OpenCollection, None),
        KeyBinding::new(key(7), NextTab, None),
        KeyBinding::new(key(8), PrevTab, None),
        KeyBinding::new(key(9), CancelRequest, None),
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_binding_string_parses() {
        for (key, name) in KEYS {
            assert!(gpui::Keystroke::parse(key).is_ok(), "{name}: {key}");
        }
    }

    #[test]
    fn bind_keys_indices_match_names() {
        let names = [
            "send", "save", "palette", "env", "new", "close", "open", "next", "prev", "cancel",
        ];
        for (i, n) in names.iter().enumerate() {
            assert_eq!(KEYS[i].1, *n);
            assert_eq!(key(i), KEYS[i].0);
        }
    }
}
