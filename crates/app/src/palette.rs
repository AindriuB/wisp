//! Command palette items and ranking.

use std::path::PathBuf;

use wisp_storage::collection::CollectionTree;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandId {
    Send,
    Save,
    NewRequest,
    CloseTab,
    OpenCollection,
    SwitchEnvironment,
    ToggleTheme,
    ToggleLayout,
    ReloadFromDisk,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteKind {
    Request(PathBuf),
    Environment(Option<String>),
    Command(CommandId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteMode {
    All,
    Environments,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteItem {
    pub label: String,
    pub detail: String,
    pub kind: PaletteKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub index: usize,
    pub score: i64,
    pub positions: Vec<usize>,
}

pub fn items(
    _tree: Option<&CollectionTree>,
    _environments: &[String],
    _active_env: Option<&str>,
    _mode: PaletteMode,
) -> Vec<PaletteItem> {
    Vec::new()
}

pub fn filter(_items: &[PaletteItem], _query: &str, _limit: usize) -> Vec<Match> {
    Vec::new()
}
