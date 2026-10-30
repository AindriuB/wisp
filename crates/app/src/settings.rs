//! App settings stored as TOML.

use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, value};

use crate::pretty::PRETTY_LIMIT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub theme: ThemeMode,
    pub split: SplitAxis,
    pub pretty_limit: u64,
    pub body_memory_limit: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            split: SplitAxis::Vertical,
            pretty_limit: PRETTY_LIMIT,
            body_memory_limit: wisp_core::body::DEFAULT_MEMORY_LIMIT,
        }
    }
}

/// Where the settings file lives: beside the executable when portable, else in the config dir.
pub fn location(
    exe_dir: &Path,
    portable_marker: bool,
    config_dir: Option<&Path>,
) -> Option<PathBuf> {
    if portable_marker {
        return Some(exe_dir.join("settings.toml"));
    }
    config_dir.map(|c| c.join("wisp").join("settings.toml"))
}

fn config_dir() -> Option<PathBuf> {
    let var = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library").join("Application Support"))
    } else if cfg!(windows) {
        var("APPDATA")
    } else {
        var("XDG_CONFIG_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| var("HOME").map(|h| h.join(".config")))
    }
}

pub fn settings_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    let portable = exe_dir.join("portable").exists();
    location(exe_dir, portable, config_dir().as_deref())
}

fn theme_str(t: ThemeMode) -> &'static str {
    match t {
        ThemeMode::System => "system",
        ThemeMode::Light => "light",
        ThemeMode::Dark => "dark",
    }
}

fn split_str(s: SplitAxis) -> &'static str {
    match s {
        SplitAxis::Vertical => "vertical",
        SplitAxis::Horizontal => "horizontal",
    }
}

/// Read settings. A missing file yields defaults silently; bad keys yield defaults plus a warning.
/// Never writes.
pub fn load(path: &Path) -> (Settings, Option<String>) {
    let mut settings = Settings::default();
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (settings, None),
        Err(e) => return (settings, Some(format!("cannot read settings: {e}"))),
    };
    let doc = match text.parse::<DocumentMut>() {
        Ok(d) => d,
        Err(e) => {
            return (
                settings,
                Some(format!(
                    "settings.toml is not valid TOML, using defaults: {e}"
                )),
            );
        }
    };
    let mut bad: Vec<&str> = Vec::new();
    if let Some(item) = doc.get("theme") {
        match item.as_str() {
            Some("system") => settings.theme = ThemeMode::System,
            Some("light") => settings.theme = ThemeMode::Light,
            Some("dark") => settings.theme = ThemeMode::Dark,
            _ => bad.push("theme"),
        }
    }
    if let Some(item) = doc.get("split") {
        match item.as_str() {
            Some("vertical") => settings.split = SplitAxis::Vertical,
            Some("horizontal") => settings.split = SplitAxis::Horizontal,
            _ => bad.push("split"),
        }
    }
    for (key, slot) in [
        ("pretty_limit", &mut settings.pretty_limit),
        ("body_memory_limit", &mut settings.body_memory_limit),
    ] {
        if let Some(item) = doc.get(key) {
            match item.as_integer().and_then(|n| u64::try_from(n).ok()) {
                Some(n) => *slot = n,
                None => bad.push(key),
            }
        }
    }
    let warning = (!bad.is_empty()).then(|| {
        format!(
            "invalid value for setting(s): {}; using defaults",
            bad.join(", ")
        )
    });
    (settings, warning)
}

fn set_if_differs(doc: &mut DocumentMut, key: &str, new: Item, same: impl Fn(&Item) -> bool) {
    match doc.get(key) {
        Some(existing) if same(existing) => {}
        Some(existing) => {
            // Keep the original line's decor (spacing, trailing comment).
            let decor = existing.as_value().map(|v| v.decor().clone());
            doc[key] = new;
            if let (Some(d), Some(v)) = (decor, doc[key].as_value_mut()) {
                *v.decor_mut() = d;
            }
        }
        None => doc[key] = new,
    }
}

/// Write settings, editing the existing file in place so comments and unknown keys survive.
pub fn save(path: &Path, settings: &Settings) -> anyhow::Result<()> {
    let mut doc = match std::fs::read_to_string(path) {
        Ok(t) => t.parse::<DocumentMut>()?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => DocumentMut::new(),
        Err(e) => return Err(e.into()),
    };
    set_if_differs(&mut doc, "theme", value(theme_str(settings.theme)), |i| {
        i.as_str() == Some(theme_str(settings.theme))
    });
    set_if_differs(&mut doc, "split", value(split_str(settings.split)), |i| {
        i.as_str() == Some(split_str(settings.split))
    });
    for (key, n) in [
        ("pretty_limit", settings.pretty_limit),
        ("body_memory_limit", settings.body_memory_limit),
    ] {
        let n = i64::try_from(n).unwrap_or(i64::MAX);
        set_if_differs(&mut doc, key, value(n), |i| i.as_integer() == Some(n));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    wisp_storage::fs::atomic_write(path, doc.to_string().as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wisp-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn location_prefers_env_then_config_dir_then_none() {
        let e = Path::new("/x");
        assert_eq!(
            location(e, true, None),
            Some(PathBuf::from("/x/settings.toml"))
        );
        assert_eq!(
            location(e, false, Some(Path::new("/c"))),
            Some(PathBuf::from("/c/wisp/settings.toml"))
        );
        assert_eq!(location(e, false, None), None);
    }

    #[test]
    fn missing_file_defaults_no_write() {
        let p = dir("missing").join("settings.toml");
        let (s, w) = load(&p);
        assert_eq!(s, Settings::default());
        assert!(w.is_none());
        assert!(!p.exists());
    }

    #[test]
    fn bad_toml_warns() {
        let p = dir("badtoml").join("settings.toml");
        std::fs::write(&p, "theme = [").unwrap();
        let (s, w) = load(&p);
        assert_eq!(s, Settings::default());
        assert!(w.unwrap().contains("TOML"));
    }

    #[test]
    fn bad_value_keeps_valid_keys() {
        let p = dir("badval").join("settings.toml");
        std::fs::write(&p, "theme = \"purple\"\nsplit = \"horizontal\"\npretty_limit = -5\nbody_memory_limit = 7\n")
            .unwrap();
        let (s, w) = load(&p);
        let w = w.unwrap();
        assert!(w.contains("theme") && w.contains("pretty_limit"));
        assert!(!w.contains("split") && !w.contains("body_memory_limit"));
        assert_eq!(s.theme, ThemeMode::System);
        assert_eq!(s.split, SplitAxis::Horizontal);
        assert_eq!(s.pretty_limit, Settings::default().pretty_limit);
        assert_eq!(s.body_memory_limit, 7);
    }

    #[test]
    fn bad_split_and_non_integer_and_out_of_range_values_warn() {
        let p = dir("badmore").join("settings.toml");
        std::fs::write(&p, "split = \"diagonal\"\nbody_memory_limit = \"big\"\npretty_limit = 9223372036854775807\n").unwrap();
        let (s, w) = load(&p);
        let w = w.unwrap();
        assert!(w.contains("split") && w.contains("body_memory_limit"));
        assert!(!w.contains("pretty_limit"));
        assert_eq!(s.split, Settings::default().split);
        assert_eq!(s.body_memory_limit, Settings::default().body_memory_limit);
        let p2 = dir("badrange").join("settings.toml");
        std::fs::write(&p2, "pretty_limit = -1\n").unwrap();
        assert!(load(&p2).1.unwrap().contains("pretty_limit"));
    }

    #[test]
    fn unparsable_file_is_left_untouched() {
        let p = dir("unparsable").join("settings.toml");
        let before = "# my notes\ntheme = \"dark\nextra = \"keep\"\n";
        std::fs::write(&p, before).unwrap();
        assert!(save(&p, &Settings::default()).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), before);
    }

    #[test]
    fn non_utf8_file_is_left_untouched() {
        let p = dir("nonutf8").join("settings.toml");
        let before = [0xff, 0xfe, b'a', b'\n'];
        std::fs::write(&p, before).unwrap();
        assert!(save(&p, &Settings::default()).is_err());
        assert_eq!(std::fs::read(&p).unwrap(), before);
    }

    #[test]
    fn save_creates_dirs_and_roundtrips() {
        let p = dir("create").join("a").join("b").join("settings.toml");
        let s = Settings {
            theme: ThemeMode::Dark,
            ..Settings::default()
        };
        save(&p, &s).unwrap();
        assert_eq!(load(&p), (s, None));
    }

    #[test]
    fn save_preserves_comments_and_unknown_keys() {
        let p = dir("preserve").join("settings.toml");
        let before = "# my settings\ntheme = \"system\" # follow os\nextra = \"keep\"\nsplit = \"vertical\"\npretty_limit = 100\nbody_memory_limit = 200\n";
        std::fs::write(&p, before).unwrap();
        let s = Settings {
            theme: ThemeMode::Dark,
            split: SplitAxis::Vertical,
            pretty_limit: 100,
            body_memory_limit: 200,
        };
        save(&p, &s).unwrap();
        let after = std::fs::read_to_string(&p).unwrap();
        assert_eq!(after, before.replace("\"system\"", "\"dark\""));
    }
}
