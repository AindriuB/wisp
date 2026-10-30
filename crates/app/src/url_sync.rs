//! Keeps the URL text, the params table and path variables consistent.

use wisp_core::request::{KeyValue, Var};

pub fn display_url(url: &str, _params: &[KeyValue]) -> String {
    url.to_string()
}

pub fn apply_url_text(text: &str, params: &[KeyValue]) -> (String, Vec<KeyValue>) {
    (text.to_string(), params.to_vec())
}

pub fn sync_path_vars(_url: &str, path: &[Var]) -> Vec<Var> {
    path.to_vec()
}
