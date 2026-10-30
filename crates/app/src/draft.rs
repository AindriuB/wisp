//! Draft vs saved comparison and the disk-change decision table.

use wisp_core::request::Request;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DirtySections {
    pub name: bool,
    pub method: bool,
    pub url: bool,
    pub params: bool,
    pub path: bool,
    pub headers: bool,
    pub auth: bool,
    pub body: bool,
    pub settings: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadAction {
    Reload,
    ShowBanner,
    MarkDeleted,
    Ignore,
}

pub fn blank_request() -> Request {
    Request::default()
}

pub fn is_dirty(_saved: Option<&Request>, _draft: &Request) -> bool {
    false
}

pub fn dirty_sections(_saved: Option<&Request>, _draft: &Request) -> DirtySections {
    DirtySections::default()
}

pub fn on_disk_change(
    _dirty: bool,
    _saved: Option<&Request>,
    _disk: Option<Result<Request, String>>,
) -> ReloadAction {
    ReloadAction::Ignore
}
