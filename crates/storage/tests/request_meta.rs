use std::fs;
use std::path::PathBuf;

use wisp_storage::request::{load_request, requests_equal, save_request};

const BASE: &str = "name = \"r\"\nmethod = \"GET\"\nurl = \"https://example.com/a\"\n";

fn write_tmp(text: &str) -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("r.toml");
    fs::write(&path, text).unwrap();
    (d, path)
}

#[test]
fn expected_status_loads_valid_forms() {
    for (lit, want) in [
        ("200", vec![200]),
        ("100", vec![100]),
        ("599", vec![599]),
        ("[200, 204]", vec![200, 204]),
    ] {
        let (_d, p) = write_tmp(&format!("expected_status = {lit}\n{BASE}"));
        assert_eq!(load_request(&p).unwrap().expected_status, want, "{lit}");
    }
    let (_d, p) = write_tmp(&format!("description = \"hi\"\n{BASE}"));
    let r = load_request(&p).unwrap();
    assert_eq!(r.description, "hi");
    assert!(r.expected_status.is_empty());
}

#[test]
fn expected_status_rejects_bad_values_naming_the_key() {
    let seventeen = format!("[{}]", vec!["200"; 17].join(", "));
    let cases = [
        "0",
        "99",
        "600",
        "\"200\"",
        "[]",
        "2.5",
        "[200, \"x\"]",
        "[200, 600]",
        "true",
        &seventeen,
    ];
    for lit in cases {
        let (_d, p) = write_tmp(&format!("expected_status = {lit}\n{BASE}"));
        let err = load_request(&p).unwrap_err().to_string();
        assert!(err.contains("expected_status"), "{lit}: {err}");
    }
    let sixteen = format!("[{}]", vec!["200"; 16].join(", "));
    let (_d, p) = write_tmp(&format!("expected_status = {sixteen}\n{BASE}"));
    assert_eq!(load_request(&p).unwrap().expected_status.len(), 16);
}

#[test]
fn file_without_either_key_round_trips_after_url_change() {
    let (_d, p) = write_tmp(BASE);
    let mut r = load_request(&p).unwrap();
    r.url = "https://example.com/b".into();
    save_request(&p, &r).unwrap();
    assert_eq!(fs::read_to_string(&p).unwrap(), BASE.replace("/a", "/b"));
}

#[test]
fn save_writes_single_as_integer_and_many_as_array() {
    let (_d, p) = write_tmp(BASE);
    let mut r = load_request(&p).unwrap();
    r.expected_status = vec![201];
    r.description = "d".into();
    save_request(&p, &r).unwrap();
    let t = fs::read_to_string(&p).unwrap();
    assert!(t.contains("expected_status = 201"), "{t}");
    assert!(t.contains("description = \"d\""), "{t}");
    assert_eq!(load_request(&p).unwrap().expected_status, vec![201]);

    r.expected_status = vec![200, 204];
    save_request(&p, &r).unwrap();
    let t = fs::read_to_string(&p).unwrap();
    assert!(t.contains("expected_status = [200, 204]"), "{t}");
    assert_eq!(load_request(&p).unwrap().expected_status, vec![200, 204]);
}

#[test]
fn unknown_to_old_code_save_keeps_both_keys_byte_for_byte() {
    let text = format!(
        "# note\ndescription = \"Creates a thing\"  # why\nexpected_status = [200,  201]\n{BASE}"
    );
    let (_d, p) = write_tmp(&text);
    let mut r = load_request(&p).unwrap();
    r.url = "https://example.com/b".into();
    save_request(&p, &r).unwrap();
    assert_eq!(fs::read_to_string(&p).unwrap(), text.replace("/a", "/b"));
}

#[test]
fn requests_equal_sees_outside_edits_to_either_field() {
    let (_d, p) = write_tmp(BASE);
    let a = load_request(&p).unwrap();
    let mut b = a.clone();
    assert!(requests_equal(&a, &b));
    b.description = "x".into();
    assert!(!requests_equal(&a, &b));
    let mut c = a.clone();
    c.expected_status = vec![200];
    assert!(!requests_equal(&a, &c));
}

#[test]
fn empty_draft_over_file_with_key_removes_it_and_reloads() {
    let (_d, p) = write_tmp(BASE);
    let r = load_request(&p).unwrap();
    fs::write(&p, format!("expected_status = 200\n{BASE}")).unwrap();
    save_request(&p, &r).unwrap();
    let t = fs::read_to_string(&p).unwrap();
    assert!(!t.contains("expected_status"), "{t}");
    assert!(load_request(&p).unwrap().expected_status.is_empty());
}
