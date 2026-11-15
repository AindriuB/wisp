use std::fs;
use std::path::PathBuf;

use wisp_storage::request::{load_request, save_request};

fn fixture(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/assert")
        .join(name);
    fs::read_to_string(p).unwrap()
}

/// 0.2.0 compatibility: a save that changes only `url` leaves `[[assert]]` byte-for-byte.
#[test]
fn save_changing_only_url_keeps_assert_tables_byte_for_byte() {
    let text = fixture("with_assert.toml");
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("r.toml");
    fs::write(&path, &text).unwrap();

    let mut req = load_request(&path).unwrap();
    req.url = "https://example.com/other".into();
    save_request(&path, &req).unwrap();

    let expected = text.replace("https://example.com/thing", "https://example.com/other");
    assert_eq!(fs::read_to_string(&path).unwrap(), expected);
}
