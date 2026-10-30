//! App settings stored as TOML.

use std::path::{Path, PathBuf};

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

pub fn location(
    _exe_dir: &Path,
    _portable_marker: bool,
    _config_dir: Option<&Path>,
) -> Option<PathBuf> {
    None
}

pub fn settings_path() -> Option<PathBuf> {
    None
}

pub fn load(_path: &Path) -> (Settings, Option<String>) {
    (Settings::default(), None)
}

pub fn save(_path: &Path, _settings: &Settings) -> anyhow::Result<()> {
    Ok(())
}
