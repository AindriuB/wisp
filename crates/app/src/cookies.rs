//! Set-Cookie header parsing for the response Cookies tab.

use wisp_core::Header;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseCookie {
    pub name: String,
    pub value: String,
    pub domain: Option<String>,
    pub path: Option<String>,
    pub expires: Option<String>,
    pub max_age: Option<i64>,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: Option<String>,
}

pub fn parse_set_cookies(_headers: &[Header]) -> Vec<ResponseCookie> {
    Vec::new()
}
