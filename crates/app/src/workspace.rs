//! The workspace entity: engine, collection, tabs and settings.

use std::path::{Path, PathBuf};

use gpui::{AppContext as _, Context, Entity, EventEmitter, SharedString};
use wisp_core::body::BodyStore;
use wisp_core::request::{CollectionDefaults, Environment};
use wisp_core::resolve::{ResolveError, Scope};
use wisp_core::{Engine, EngineConfig, EventReceiver, RequestId};
use wisp_storage::collection::CollectionTree;

use crate::settings::Settings;
use crate::tab::RequestTab;

pub struct OpenCollection {
    pub tree: CollectionTree,
    pub environments: Vec<String>,
    pub active_env: Option<Environment>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceEvent {
    CollectionChanged,
    EnvironmentChanged,
    TabsChanged,
    ActiveTabChanged,
    Notice(SharedString),
}

pub struct Workspace {
    pub engine: Engine,
    pub settings: Settings,
    pub settings_path: Option<PathBuf>,
    pub collection: Option<OpenCollection>,
    pub tabs: Vec<Entity<RequestTab>>,
    pub active: Option<usize>,
    _events: EventReceiver,
    empty_defaults: CollectionDefaults,
}

impl EventEmitter<WorkspaceEvent> for Workspace {}

impl Workspace {
    pub fn new(
        settings: Settings,
        settings_path: Option<PathBuf>,
        _open: Option<PathBuf>,
        _cx: &mut Context<Self>,
    ) -> Self {
        let (engine, events) = Engine::start(EngineConfig::default());
        Self {
            engine,
            settings,
            settings_path,
            collection: None,
            tabs: Vec::new(),
            active: None,
            _events: events,
            empty_defaults: CollectionDefaults::default(),
        }
    }

    pub fn open_collection(&mut self, _root: &Path, _cx: &mut Context<Self>) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn reload_tree(&mut self, _cx: &mut Context<Self>) {}

    pub fn set_environment(
        &mut self,
        _name: Option<&str>,
        _cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn open_request(&mut self, _rel: &Path, _cx: &mut Context<Self>) {}

    pub fn new_scratch_tab(&mut self, url: String, cx: &mut Context<Self>) -> Entity<RequestTab> {
        let tab = cx.new(|_| RequestTab::new_scratch(url));
        self.tabs.push(tab.clone());
        self.active = Some(self.tabs.len() - 1);
        tab
    }

    pub fn activate_tab(&mut self, _ix: usize, _cx: &mut Context<Self>) {}

    pub fn close_tab(&mut self, _ix: usize, _cx: &mut Context<Self>) {}

    pub fn active_tab(&self) -> Option<Entity<RequestTab>> {
        self.active.and_then(|ix| self.tabs.get(ix).cloned())
    }

    pub fn scope_for(&self, _rel: Option<&Path>) -> Scope<'_> {
        Scope {
            collection: self
                .collection
                .as_ref()
                .map_or(&self.empty_defaults, |c| &c.tree.defaults),
            folders: Vec::new(),
            environment: None,
        }
    }

    pub fn send(
        &mut self,
        _tab: &Entity<RequestTab>,
        _cx: &mut Context<Self>,
    ) -> Result<RequestId, ResolveError> {
        Ok(RequestId(0))
    }

    pub fn cancel(&mut self, _tab: &Entity<RequestTab>, _cx: &mut Context<Self>) {}

    pub fn save(
        &mut self,
        _tab: &Entity<RequestTab>,
        _cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn bodies(&self) -> BodyStore {
        self.engine.bodies()
    }

    pub fn update_settings(&mut self, f: impl FnOnce(&mut Settings), _cx: &mut Context<Self>) {
        f(&mut self.settings);
    }

    pub fn on_node_moved(&mut self, _old_rel: &Path, _new_rel: &Path, _cx: &mut Context<Self>) {}

    pub fn on_node_deleted(&mut self, _rel: &Path, _cx: &mut Context<Self>) {}
}
