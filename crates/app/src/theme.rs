//! Wisp's own light and dark themes, and applying the configured mode.

use std::rc::Rc;

use gpui::{App, Hsla, Rgba, Window};
use gpui_component::theme::{Theme, ThemeConfig, ThemeMode as ComponentMode, ThemeSet};

use crate::settings::ThemeMode;

/// The compiled-in theme set: "Wisp Light" and "Wisp Dark".
const THEME_JSON: &str = include_str!("theme.json");

/// Share of the warning colour blended over the background for banners.
const BANNER_ALPHA: f32 = 0.17;

/// Alpha the library applies to `list_active` when a theme is applied
/// (gpui-component 0.8.1 `theme/schema.rs:1025-1056`).
#[cfg(test)]
const LIST_ACTIVE_ALPHA: f32 = 0.2;
/// Alpha the library applies to `selection` (same place).
#[cfg(test)]
const SELECTION_ALPHA: f32 = 0.3;

/// Parses a theme set and returns its `(light, dark)` configs.
fn parse_themes(json: &str) -> Result<(Rc<ThemeConfig>, Rc<ThemeConfig>), String> {
    let set: ThemeSet = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let find = |mode: ComponentMode| {
        set.themes
            .iter()
            .find(|t| t.mode == mode)
            .cloned()
            .map(Rc::new)
            .ok_or_else(|| format!("theme set has no {mode:?} theme"))
    };
    Ok((find(ComponentMode::Light)?, find(ComponentMode::Dark)?))
}

/// Installs Wisp's themes as the light and dark themes. On a parse failure
/// logs and leaves the library defaults in place.
fn install(json: &str, cx: &mut App) {
    match parse_themes(json) {
        Ok((light, dark)) => {
            if cx.has_global::<Theme>() {
                let theme = Theme::global_mut(cx);
                theme.light_theme = light;
                theme.dark_theme = dark;
            }
        }
        Err(e) => eprintln!("wisp: theme: using default themes: {e}"),
    }
}

/// System follows the OS appearance; Light and Dark force that mode.
pub fn apply(mode: ThemeMode, window: Option<&mut Window>, cx: &mut App) {
    install(THEME_JSON, cx);
    match mode {
        ThemeMode::System => Theme::sync_system_appearance(window, cx),
        ThemeMode::Light => Theme::change(ComponentMode::Light, window, cx),
        ThemeMode::Dark => Theme::change(ComponentMode::Dark, window, cx),
    }
}

/// Blends `fg` over `bg` at `alpha`, opaque.
fn tint(fg: Rgba, bg: Rgba, alpha: f32) -> Rgba {
    let mix = |f: f32, b: f32| b + alpha * (f - b);
    Rgba {
        r: mix(fg.r, bg.r),
        g: mix(fg.g, bg.g),
        b: mix(fg.b, bg.b),
        a: 1.0,
    }
}

/// Background of a warning banner: the warning colour tinted over the page.
pub fn banner_bg(theme: &Theme) -> Hsla {
    tint(theme.warning.into(), theme.background.into(), BANNER_ALPHA).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const COLOR_KEYS: &[&str] = &[
        "accent.background",
        "accent.foreground",
        "accordion.background",
        "background",
        "border",
        "group_box.background",
        "group_box.foreground",
        "caret",
        "chart.1",
        "chart.2",
        "chart.3",
        "chart.4",
        "chart.5",
        "chart.bullish",
        "chart.bearish",
        "chart.grid",
        "danger.background",
        "danger.foreground",
        "description_list_label.background",
        "description_list_label.foreground",
        "drag_border",
        "drop_target.background",
        "foreground",
        "info.background",
        "info.foreground",
        "input.border",
        "link.foreground",
        "link.active.foreground",
        "link.hover.foreground",
        "list.background",
        "list.active.background",
        "list.active.border",
        "list.even.background",
        "list.head.background",
        "muted.background",
        "muted.foreground",
        "popover.background",
        "popover.foreground",
        "primary.background",
        "primary.active.background",
        "primary.foreground",
        "primary.hover.background",
        "progress_bar.background",
        "ring",
        "scrollbar.background",
        "scrollbar.thumb.background",
        "scrollbar.thumb.hover.background",
        "secondary.background",
        "secondary.active.background",
        "secondary.foreground",
        "secondary.hover.background",
        "selection.background",
        "sidebar.background",
        "sidebar.accent.background",
        "sidebar.accent.foreground",
        "sidebar.border",
        "sidebar.foreground",
        "sidebar.primary.background",
        "sidebar.primary.foreground",
        "skeleton.background",
        "slider.bar.background",
        "slider.thumb.background",
        "success.background",
        "success.foreground",
        "switch.background",
        "tab.background",
        "tab.active.background",
        "tab.active.foreground",
        "tab_bar.background",
        "tab_bar.segmented.background",
        "tab.foreground",
        "table.background",
        "table.head.foreground",
        "table.row.border",
        "title_bar.background",
        "title_bar.border",
        "status_bar.background",
        "status_bar.border",
        "warning.background",
        "warning.foreground",
        "overlay",
        "window.border",
        "base.red",
        "base.red.light",
        "base.green",
        "base.green.light",
        "base.blue",
        "base.blue.light",
        "base.yellow",
        "base.yellow.light",
        "base.magenta",
        "base.magenta.light",
        "base.cyan",
        "base.cyan.light",
    ];

    const HIGHLIGHT_KEYS: &[&str] = &[
        "editor.foreground",
        "editor.background",
        "editor.active_line.background",
        "editor.line_number",
        "editor.active_line_number",
        "editor.invisible",
    ];

    fn doc() -> Value {
        serde_json::from_str(THEME_JSON).unwrap()
    }

    fn theme(mode: &str) -> Value {
        doc()["themes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["mode"] == mode)
            .unwrap()
            .clone()
    }

    fn hex(s: &str) -> Rgba {
        let h = s.trim_start_matches('#');
        let n = u32::from_str_radix(h, 16).unwrap();
        if h.len() == 8 {
            Rgba {
                r: ((n >> 24) & 0xff) as f32 / 255.0,
                g: ((n >> 16) & 0xff) as f32 / 255.0,
                b: ((n >> 8) & 0xff) as f32 / 255.0,
                a: (n & 0xff) as f32 / 255.0,
            }
        } else {
            assert_eq!(h.len(), 6, "bad colour {s}");
            Rgba {
                r: ((n >> 16) & 0xff) as f32 / 255.0,
                g: ((n >> 8) & 0xff) as f32 / 255.0,
                b: (n & 0xff) as f32 / 255.0,
                a: 1.0,
            }
        }
    }

    fn lum(c: Rgba) -> f32 {
        let l = |x: f32| {
            if x <= 0.03928 {
                x / 12.92
            } else {
                ((x + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * l(c.r) + 0.7152 * l(c.g) + 0.0722 * l(c.b)
    }

    fn contrast(a: Rgba, b: Rgba) -> f32 {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    fn color(t: &Value, key: &str) -> Rgba {
        hex(t["colors"][key]
            .as_str()
            .unwrap_or_else(|| panic!("missing {key}")))
    }

    fn at_least(t: &Value, name: &str, fg: Rgba, bg: Rgba, min: f32) {
        let c = contrast(fg, bg);
        assert!(c >= min, "{} {name}: {c:.2} < {min}", t["name"]);
    }

    #[test]
    fn parses_two_themes_light_and_dark() {
        let (light, dark) = parse_themes(THEME_JSON).unwrap();
        assert_eq!(light.mode, ComponentMode::Light);
        assert_eq!(dark.mode, ComponentMode::Dark);
        assert_eq!(light.name.as_ref(), "Wisp Light");
        assert_eq!(dark.name.as_ref(), "Wisp Dark");
        assert_eq!(doc()["themes"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn defines_every_key() {
        assert_eq!(COLOR_KEYS.len(), 94);
        for mode in ["light", "dark"] {
            let t = theme(mode);
            for k in COLOR_KEYS {
                assert!(t["colors"][k].is_string(), "{mode} lacks colour {k}");
            }
            for k in HIGHLIGHT_KEYS {
                assert!(t["highlight"][k].is_string(), "{mode} lacks highlight {k}");
            }
        }
    }

    #[test]
    fn dark_has_no_near_black_background() {
        let t = theme("dark");
        let mut keys: Vec<&String> = t["colors"]
            .as_object()
            .unwrap()
            .keys()
            .filter(|k| k.ends_with("background"))
            .collect();
        let editor = "editor.background".to_string();
        keys.push(&editor);
        for k in keys {
            let v = if *k == editor {
                t["highlight"][k].as_str().unwrap()
            } else {
                t["colors"][k].as_str().unwrap()
            };
            let c = hex(v);
            if c.a < 1.0 {
                continue;
            }
            assert!(lum(c) >= 0.008, "{k} {v} is too dark");
        }
    }

    #[test]
    fn contrast_meets_wcag() {
        for mode in ["light", "dark"] {
            let t = theme(mode);
            let c = |k: &str| color(&t, k);
            let (page, panel, raised) = (
                c("background"),
                c("sidebar.background"),
                c("popover.background"),
            );
            let hover = c("muted.background");
            let selected = tint(c("list.active.background"), page, LIST_ACTIVE_ALPHA);
            let selection = tint(c("selection.background"), page, SELECTION_ALPHA);
            let (text, muted) = (c("foreground"), c("muted.foreground"));
            for (n, bg) in [
                ("page", page),
                ("panel", panel),
                ("raised", raised),
                ("hover", hover),
                ("selected", selected),
            ] {
                at_least(&t, &format!("text/{n}"), text, bg, 4.5);
            }
            for (n, bg) in [("page", page), ("panel", panel), ("raised", raised)] {
                at_least(&t, &format!("muted/{n}"), muted, bg, 4.5);
                for k in [
                    "primary.background",
                    "success.background",
                    "warning.background",
                    "danger.background",
                    "info.background",
                ] {
                    at_least(&t, &format!("{k}/{n}"), c(k), bg, 4.5);
                }
            }
            at_least(&t, "muted/selected", muted, selected, 4.5);
            for k in ["primary", "success", "warning", "danger", "info"] {
                at_least(
                    &t,
                    &format!("{k}.foreground"),
                    color(&t, &format!("{k}.foreground")),
                    color(&t, &format!("{k}.background")),
                    4.5,
                );
            }
            at_least(&t, "text/selection", text, selection, 4.5);
            let open = c("secondary.active.background");
            for (n, fg) in [
                ("text", text),
                ("muted", muted),
                ("success", c("success.background")),
                ("warning", c("warning.background")),
                ("danger", c("danger.background")),
                ("info", c("info.background")),
                ("accent", c("primary.background")),
            ] {
                at_least(&t, &format!("{n}/open row"), fg, open, 4.5);
            }
            let banner = tint(c("warning.background"), page, BANNER_ALPHA);
            at_least(&t, "warning/banner", c("warning.background"), banner, 4.5);
            at_least(&t, "text/banner", text, banner, 4.5);
            for (n, bg) in [("page", page), ("raised", raised)] {
                at_least(&t, &format!("ring/{n}"), c("ring"), bg, 3.0);
                at_least(&t, &format!("input.border/{n}"), c("input.border"), bg, 3.0);
            }
            let editor = hex(t["highlight"]["editor.background"].as_str().unwrap());
            for (k, v) in t["highlight"]["syntax"].as_object().unwrap() {
                if let Some(col) = v["color"].as_str() {
                    at_least(&t, &format!("syntax {k}"), hex(col), editor, 4.5);
                }
            }
        }
    }

    /// CIE76 colour difference (sRGB, D65).
    fn delta_e(a: Rgba, b: Rgba) -> f32 {
        fn lab(c: Rgba) -> [f32; 3] {
            let lin = |x: f32| {
                if x <= 0.04045 {
                    x / 12.92
                } else {
                    ((x + 0.055) / 1.055).powf(2.4)
                }
            };
            let (r, g, bl) = (lin(c.r), lin(c.g), lin(c.b));
            let x = (0.4124 * r + 0.3576 * g + 0.1805 * bl) / 0.95047;
            let y = 0.2126 * r + 0.7152 * g + 0.0722 * bl;
            let z = (0.0193 * r + 0.1192 * g + 0.9505 * bl) / 1.08883;
            let f = |t: f32| {
                if t > 0.008856 {
                    t.cbrt()
                } else {
                    7.787 * t + 16.0 / 116.0
                }
            };
            let (fx, fy, fz) = (f(x), f(y), f(z));
            [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
        }
        let (p, q) = (lab(a), lab(b));
        ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()
    }

    #[test]
    fn highlights_are_distinguishable() {
        for mode in ["light", "dark"] {
            let t = theme(mode);
            let c = |k: &str| color(&t, k);
            let (page, panel, hover) = (
                c("background"),
                c("sidebar.background"),
                c("list.hover.background"),
            );
            let open = c("secondary.active.background");
            let list = tint(c("list.active.background"), page, LIST_ACTIVE_ALPHA);
            let sel = tint(c("selection.background"), page, SELECTION_ALPHA);
            let checks = [
                ("open/page", open, page, 8.0),
                ("open/panel", open, panel, 8.0),
                ("open/hover", open, hover, 6.0),
                ("list.active/page", list, page, 8.0),
                ("list.active/hover", list, hover, 6.0),
                ("selection/page", sel, page, 8.0),
            ];
            for (n, a, b, min) in checks {
                let d = delta_e(a, b);
                println!("{mode} {n}: {d:.1}");
                assert!(d >= min, "{mode} {n}: dE {d:.1} < {min}");
            }
        }
    }

    #[test]
    fn broken_json_is_an_error_not_a_panic() {
        assert!(parse_themes("{ not json").is_err());
        assert!(parse_themes(r#"{"themes": []}"#).is_err());
    }

    #[test]
    fn install_keeps_defaults_on_bad_json() {
        // With no Theme global, `install` must log and return without panicking.
        let cx = gpui::TestAppContext::single();
        cx.update(|cx| install("{ broken", cx));
    }
}
