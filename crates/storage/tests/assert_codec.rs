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
    assert!(saved.contains("value = 201"));
    assert!(!saved.contains("value = 200"));
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

fn st(n: i64) -> Assertion {
    Assertion::Status {
        op: Op::Equals,
        expected: Expected::Int(n),
    }
}

fn status_file(codes: &[i64]) -> String {
    let mut t =
        String::from("name = \"n\"\nmethod = \"GET\"\nurl = \"u\"\n\n[auth]\ntype = \"none\"\n");
    for c in codes {
        t.push_str(&format!(
            "\n[[assert]]\nkind = \"status\"\nop = \"equals\"\nvalue = {c}\n"
        ));
    }
    t
}

fn unrec(raw: &str, reason: &str) -> Assertion {
    Assertion::Unrecognised {
        raw: raw.into(),
        reason: reason.into(),
    }
}

fn save_reload(text: &str, edit: impl FnOnce(&mut Vec<Assertion>)) -> (Vec<Assertion>, String) {
    let (_d, path) = write_tmp(text);
    let mut r = load_request(&path).unwrap();
    edit(&mut r.assertions);
    save_request(&path, &r).unwrap();
    let saved = fs::read_to_string(&path).unwrap();
    assert_eq!(
        format!("{:?}", load_request(&path).unwrap().assertions),
        format!("{:?}", r.assertions),
        "{saved}"
    );
    (r.assertions, saved)
}

#[test]
fn inserting_before_existing_entries_keeps_the_given_order() {
    save_reload(&status_file(&[202, 201, 203]), |a| {
        a.insert(0, st(301));
        a.insert(0, st(300));
    });
}

#[test]
fn appended_unrecognised_entry_is_written_last() {
    let (a, _) = save_reload(&status_file(&[201, 202]), |a| {
        a.push(unrec(
            "[[assert]]\nkind = \"future\"\nop = \"x\"\n",
            "not supported",
        ));
    });
    assert!(matches!(a[2], Assertion::Unrecognised { .. }));
}

#[test]
fn child_tables_move_with_their_parent() {
    let text = "name = \"n\"\nmethod = \"GET\"\nurl = \"u\"\n\n[[assert]]\nkind = \"status\"\nop = \"equals\"\nvalue = 200\n\n\
[[assert]]\nkind = \"status\"\nop = \"equals\"\nvalue = 201\n\n[assert.mod]\nowner = \"b\"\n\n\
[[assert]]\nkind = \"status\"\nop = \"equals\"\nvalue = 202\n\n[assert.mod]\nowner = \"c\"\n";
    let (a, saved) = save_reload(text, |a| {
        a.reverse();
        a.insert(0, st(500));
    });
    let pos = |s: &str| saved.find(s).unwrap_or_else(|| panic!("{s} in {saved}"));
    assert!(pos("owner = \"c\"") < pos("owner = \"b\""));
    for (i, owner) in [(1, "c"), (2, "b")] {
        let Assertion::Unrecognised { raw, .. } = &a[i] else {
            panic!("{a:?}")
        };
        assert!(raw.contains(&format!("owner = \"{owner}\"")), "{raw}");
    }
}

fn load_one(entry: &str) -> Assertion {
    let (_d, path) = write_tmp(&format!(
        "name = \"n\"\nmethod = \"GET\"\nurl = \"u\"\n\n[[assert]]\n{entry}\n"
    ));
    load_request(&path).unwrap().assertions.remove(0)
}

fn reason(a: &Assertion) -> &str {
    match a {
        Assertion::Unrecognised { reason, .. } => reason,
        _ => "typed",
    }
}

#[test]
fn target_keys_of_another_kind_are_malformed() {
    for e in [
        "kind = \"status\"\nop = \"equals\"\nvalue = 200\nname = \"x\"",
        "kind = \"status\"\nop = \"equals\"\nvalue = 200\npath = \"$.a\"",
        "kind = \"header\"\nname = \"a\"\nop = \"exists\"\npath = \"$.a\"",
        "kind = \"json\"\npath = \"$.a\"\nop = \"exists\"\nname = \"a\"",
    ] {
        assert_eq!(reason(&load_one(e)), "malformed", "{e}");
    }
}

#[test]
fn unknown_keys_on_known_kinds_are_not_supported() {
    for e in [
        "kind = \"status\"\nop = \"equals\"\nvalue = 200\nnegate = true",
        "kind = \"header\"\nname = \"a\"\nop = \"exists\"\nnegate = true",
        "kind = \"json\"\npath = \"$.a\"\nop = \"exists\"\nnegate = true",
        "kind = \"status\"\nop = \"equals\"\nvalue = 200\n[assert.sub]\nx = 1",
    ] {
        assert_eq!(reason(&load_one(e)), "not supported", "{e}");
    }
    // Kept verbatim on save.
    let text = "name = \"n\"\nmethod = \"GET\"\nurl = \"u\"\n\n[[assert]]\nkind = \"status\"\nop = \"equals\"\nvalue = 200\nnegate = true\n";
    let (_a, saved) = save_reload(text, |a| a.push(st(201)));
    assert!(saved.contains("negate = true"));
}

#[test]
fn nan_value_round_trips_unchanged_with_comments() {
    let text = "name = \"n\"\nmethod = \"GET\"\nurl = \"u\"\n\n[[assert]]\n# keep me\nkind = \"json\"\npath = \"$.a\"\nop = \"equals\"\nvalue = nan\n";
    let (_d, path) = write_tmp(text);
    let r = load_request(&path).unwrap();
    save_request(&path, &r).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    let (_a, saved) = save_reload(text, |a| a.push(st(201)));
    assert!(saved.contains("# keep me"));
}

#[test]
fn non_array_assert_key_errors_when_adding_assertions() {
    let text = "name = \"n\"\nmethod = \"GET\"\nurl = \"u\"\nassert = []\n";
    let (_d, path) = write_tmp(text);
    let mut r = load_request(&path).unwrap();
    assert!(r.assertions.is_empty());
    save_request(&path, &r).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    r.assertions.push(st(200));
    assert!(save_request(&path, &r).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
}
