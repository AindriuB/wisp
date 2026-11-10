//! Keyboard actions and their default bindings.

use gpui::{App, KeyBinding, OperatingSystem, actions};

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
        ToggleLayout,
        OpenImport,
        Quit
    ]
);

#[cfg(target_os = "macos")]
actions!(wisp, [HideApp, HideOthers, ShowAll]);

/// Every default binding as (keystroke, action name), with the primary modifier
/// (`cmd` on macOS, `ctrl` elsewhere) spelled out per platform.
#[cfg(target_os = "macos")]
const KEYS: [(&str, &str); 13] = [
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
    ("cmd-q", "quit"),
    ("cmd-h", "hide"),
    ("alt-cmd-h", "hide-others"),
];
#[cfg(not(target_os = "macos"))]
const KEYS: [(&str, &str); 11] = [
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
    ("ctrl-q", "quit"),
];

/// The keystroke of entry `index` in `KEYS`; the order below mirrors it.
const fn key(index: usize) -> &'static str {
    KEYS[index].0
}

/// Format a keystroke string for display: `⌘K` on macOS, `Ctrl+K` elsewhere.
/// Falls back to the raw string if it does not parse.
pub fn format_hint(keystroke: &'static str, os: OperatingSystem) -> String {
    let Ok(ks) = gpui::Keystroke::parse(keystroke) else {
        return keystroke.to_string();
    };
    let mac = os == OperatingSystem::Mac;
    let m = &ks.modifiers;
    let mut out = String::new();
    let mut push = |on: bool, mac_sym: &str, name: &str| {
        if on {
            if mac {
                out.push_str(mac_sym);
            } else {
                out.push_str(name);
                out.push('+');
            }
        }
    };
    push(m.control, "⌃", "Ctrl");
    push(m.alt, "⌥", "Alt");
    push(m.shift, "⇧", "Shift");
    push(m.platform, "⌘", "Win");
    let key = match ks.key.as_str() {
        "enter" => "Enter".to_string(),
        "escape" => "Esc".to_string(),
        "tab" => "Tab".to_string(),
        k => k.to_uppercase(),
    };
    out.push_str(&key);
    out
}

/// The hint for the command palette, read from the same `KEYS` entry that
/// `bind_keys` registers, formatted for the host OS.
pub fn palette_hint() -> String {
    let os = if cfg!(target_os = "macos") {
        OperatingSystem::Mac
    } else if cfg!(target_os = "windows") {
        OperatingSystem::Windows
    } else {
        OperatingSystem::Linux
    };
    format_hint(key(2), os)
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
        KeyBinding::new(key(10), Quit, None),
    ]);
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new(key(11), HideApp, None),
        KeyBinding::new(key(12), HideOthers, None),
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
    fn hints_per_platform() {
        assert_eq!(format_hint("ctrl-k", OperatingSystem::Linux), "Ctrl+K");
        assert_eq!(format_hint("ctrl-k", OperatingSystem::Windows), "Ctrl+K");
        assert_eq!(format_hint("cmd-k", OperatingSystem::Mac), "⌘K");
        assert_eq!(
            format_hint("ctrl-shift-tab", OperatingSystem::Linux),
            "Ctrl+Shift+Tab"
        );
    }

    #[test]
    fn quit_hint_reads_cmd_q_on_mac_and_ctrl_q_elsewhere() {
        let quit = KEYS.iter().find(|(_, n)| *n == "quit").unwrap().0;
        let mac = if cfg!(target_os = "macos") {
            quit
        } else {
            "cmd-q"
        };
        assert_eq!(format_hint(mac, OperatingSystem::Mac), "⌘Q");
        assert_eq!(format_hint("ctrl-q", OperatingSystem::Linux), "Ctrl+Q");
        if !cfg!(target_os = "macos") {
            assert_eq!(quit, "ctrl-q");
        }
    }

    #[test]
    fn palette_hint_reads_keys() {
        let expected = if cfg!(target_os = "macos") {
            "⌘K"
        } else {
            "Ctrl+K"
        };
        assert_eq!(palette_hint(), expected);
    }

    #[test]
    fn bind_keys_indices_match_names() {
        let names = [
            "send",
            "save",
            "palette",
            "env",
            "new",
            "close",
            "open",
            "next",
            "prev",
            "cancel",
            "quit",
            #[cfg(target_os = "macos")]
            "hide",
            #[cfg(target_os = "macos")]
            "hide-others",
        ];
        for (i, n) in names.iter().enumerate() {
            assert_eq!(KEYS[i].1, *n);
            assert_eq!(key(i), KEYS[i].0);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn hide_bindings_parse_on_mac() {
        for i in [11, 12] {
            gpui::Keystroke::parse(key(i)).expect("keystroke parses");
        }
    }
}
