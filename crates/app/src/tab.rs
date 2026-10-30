//! One open request: its draft, last saved snapshot and latest response.

use std::path::PathBuf;
use std::time::Duration;

use gpui::{Context, EventEmitter, SharedString};
use wisp_core::body::BodyHandle;
use wisp_core::request::Request;
use wisp_core::{EngineError, Header, HttpVersion, RequestId};

use crate::draft;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestSubTab {
    Params,
    Path,
    Headers,
    Auth,
    Body,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseSubTab {
    Body,
    Headers,
    Cookies,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyView {
    Pretty,
    Raw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskState {
    InSync,
    ChangedOnDisk,
    DeletedOnDisk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Idle,
    Sending,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResponseState {
    pub phase: Phase,
    pub request_id: Option<RequestId>,
    pub status: Option<u16>,
    pub reason: Option<String>,
    pub version: Option<HttpVersion>,
    pub headers: Vec<Header>,
    pub ttfb: Option<Duration>,
    pub total: Option<Duration>,
    pub size: Option<u64>,
    pub received: u64,
    pub body: Option<BodyHandle>,
    pub error: Option<EngineError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabEvent {
    DraftEdited,
    DraftReplaced,
    ResponseChanged,
    DiskStateChanged,
    Saved,
}

pub struct RequestTab {
    /// Relative to the collection root; `None` is a scratch tab.
    pub path: Option<PathBuf>,
    /// Snapshot as last loaded or saved.
    pub saved: Option<Request>,
    pub draft: Request,
    /// Bumped only by `replace_draft`.
    pub revision: u64,
    pub disk: DiskState,
    pub response: ResponseState,
    pub request_tab: RequestSubTab,
    pub response_tab: ResponseSubTab,
    pub body_view: BodyView,
    pub load_error: Option<String>,
}

impl EventEmitter<TabEvent> for RequestTab {}

impl RequestTab {
    fn with(path: Option<PathBuf>, saved: Option<Request>, draft: Request) -> Self {
        Self {
            path,
            saved,
            draft,
            revision: 0,
            disk: DiskState::InSync,
            response: ResponseState::default(),
            request_tab: RequestSubTab::Params,
            response_tab: ResponseSubTab::Body,
            body_view: BodyView::Pretty,
            load_error: None,
        }
    }

    pub fn new_scratch(url: String) -> Self {
        let mut draft = draft::blank_request();
        draft.url = url;
        Self::with(None, None, draft)
    }

    pub fn from_saved(path: PathBuf, request: Request) -> Self {
        Self::with(Some(path), Some(request.clone()), request)
    }

    pub fn title(&self) -> SharedString {
        if self.draft.name.is_empty() {
            "Untitled".into()
        } else {
            self.draft.name.clone().into()
        }
    }

    pub fn is_dirty(&self) -> bool {
        draft::is_dirty(self.saved.as_ref(), &self.draft)
    }

    pub fn edit(&mut self, f: impl FnOnce(&mut Request), cx: &mut Context<Self>) {
        f(&mut self.draft);
        cx.emit(TabEvent::DraftEdited);
    }

    pub fn replace_draft(&mut self, request: Request, cx: &mut Context<Self>) {
        self.draft = request;
        self.revision += 1;
        cx.emit(TabEvent::DraftReplaced);
    }

    pub fn mark_saved(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.path = Some(path);
        self.saved = Some(self.draft.clone());
        cx.emit(TabEvent::Saved);
    }

    pub fn set_disk_state(&mut self, state: DiskState, cx: &mut Context<Self>) {
        self.disk = state;
        cx.emit(TabEvent::DiskStateChanged);
    }
}
