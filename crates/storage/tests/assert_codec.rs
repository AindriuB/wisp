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

use wisp_core::assert::{Assertion, Expected, Op};
use wisp_core::request::Request;

fn write_tmp(text: &str) -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("r.toml");
    fs::write(&path, text).unwrap();
    (d, path)
}

#[test]
fn loads_known_and_unknown_entries() {
    let (_d, path) = write_tmp(&fixture("with_assert.toml"));
    let r = load_request(&path).unwrap();
    assert_eq!(r.assertions.len(), 4);
    assert_eq!(
        r.assertions[0],
        Assertion::Status {
            op: Op::Equals,
            expected: Expected::Int(200)
        }
    );
    assert_eq!(
        r.assertions[1],
        Assertion::Header {
            name: "content-type".into(),
            op: Op::Contains,
            expected: Expected::Str("json".into())
        }
    );
    assert_eq!(
        r.assertions[2],
        Assertion::Json {
            path: "$.data.items[0].id".into(),
            op: Op::Exists,
            expected: Expected::Bool(true)
        }
    );
    let Assertion::Unrecognised { raw, reason } = &r.assertions[3] else {
        panic!("expected unrecognised");
    };
    assert_eq!(reason, "not supported");
    assert!(raw.contains("teleports"));
}

#[test]
fn malformed_known_kind_loads_as_unrecognised_malformed() {
    let (_d, path) = write_tmp(
        "name = \"n\"\nmethod = \"GET\"\nurl = \"u\"\n\n[[assert]]\nkind = \"status\"\nop = \"equals\"\nvalue = \"200\"\n\n[[assert]]\nkind = \"header\"\nop = \"exists\"\n\n[[assert]]\nkind = \"status\"\nop = \"equals\"\nvalue = 99\n",
    );
    let r = load_request(&path).unwrap();
    assert_eq!(r.assertions.len(), 3);
    for a in &r.assertions {
        assert!(
            matches!(a, Assertion::Unrecognised { reason, .. } if reason == "malformed"),
            "{a:?}"
        );
    }
}

#[test]
fn unchanged_save_is_byte_exact_with_comments() {
    let text = fixture("with_assert.toml");
    let (_d, path) = write_tmp(&text);
    let r = load_request(&path).unwrap();
    save_request(&path, &r).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
}

#[test]
fn unknown_kind_survives_load_save_reload_byte_for_byte() {
    let (_d, path) = write_tmp(&fixture("with_assert.toml"));
    let mut r = load_request(&path).unwrap();
    // Edit a neighbour so the array is rewritten.
    r.assertions[0] = Assertion::Status {
        op: Op::Equals,
        expected: Expected::Int(201),
    };
    save_request(&path, &r).unwrap();
    let saved = fs::read_to_string(&path).unwrap();
    assert!(
        saved.contains("kind = \"future\"\nop = \"teleports\"\npayload = { a = 1, b = \"two\" }\n")
    );
    let again = load_request(&path).unwrap();
    assert_eq!(again.assertions, r.assertions);
    assert!(saved.contains("# status must be ok") || saved.contains("value = 201"));
}

#[test]
fn no_entry_is_dropped_on_save_and_reorder_and_add_work() {
    let (_d, path) = write_tmp(&fixture("with_assert.toml"));
    let mut r = load_request(&path).unwrap();
    r.assertions.reverse();
    r.assertions.push(Assertion::Header {
        name: "x".into(),
        op: Op::Exists,
        expected: Expected::Bool(false),
    });
    save_request(&path, &r).unwrap();
    let again = load_request(&path).unwrap();
    assert_eq!(again.assertions, r.assertions);
    assert_eq!(again.assertions.len(), 5);
}

#[test]
fn array_written_only_when_non_empty_or_present() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("new.toml");
    let mut r = Request {
        name: "n".into(),
        url: "u".into(),
        ..Request::default()
    };
    save_request(&path, &r).unwrap();
    assert!(!fs::read_to_string(&path).unwrap().contains("assert"));
    r.assertions.push(Assertion::Json {
        path: "$.a".into(),
        op: Op::Equals,
        expected: Expected::Float(1.5),
    });
    save_request(&path, &r).unwrap();
    assert!(fs::read_to_string(&path).unwrap().contains("[[assert]]"));
    assert_eq!(load_request(&path).unwrap().assertions, r.assertions);
    r.assertions.clear();
    save_request(&path, &r).unwrap();
    assert!(!fs::read_to_string(&path).unwrap().contains("assert"));
}
