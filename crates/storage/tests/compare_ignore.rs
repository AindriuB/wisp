use wisp_storage::StorageError;
use wisp_storage::collection::{
    load_collection_defaults, load_compare_ignore, save_collection_defaults, save_compare_ignore,
};

fn root(body: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("collection.toml"), body).unwrap();
    d
}

fn text(d: &tempfile::TempDir) -> String {
    std::fs::read_to_string(d.path().join("collection.toml")).unwrap()
}

#[test]
fn compare_ignore_missing_table_is_empty() {
    let d = root("format = 1\n");
    assert!(load_compare_ignore(d.path()).unwrap().is_empty());
}

#[test]
fn compare_ignore_round_trips() {
    let d = root("format = 1\n");
    let v = vec!["header:Date".to_string(), "body:/a/*/id".to_string()];
    save_compare_ignore(d.path(), &v).unwrap();
    assert_eq!(load_compare_ignore(d.path()).unwrap(), v);
}

#[test]
fn compare_ignore_keeps_comments_and_tables() {
    let d = root("format = 1\n# keep me\n[vars]\na = \"1\"\n\n[other]\nx = 1\n");
    save_compare_ignore(d.path(), &["header:date".into()]).unwrap();
    let t = text(&d);
    assert!(t.contains("# keep me"), "{t}");
    assert!(t.contains("[other]"), "{t}");
    assert!(t.contains("[compare]"), "{t}");
}

#[test]
fn compare_ignore_errors_name_the_key() {
    let many = format!(
        "format = 1\n[compare]\nignore = [{}]\n",
        vec!["\"header:a\""; 257].join(",")
    );
    let long = format!(
        "format = 1\n[compare]\nignore = [\"{}\"]\n",
        "a".repeat(1025)
    );
    for body in [
        many,
        long,
        "format = 1\n[compare]\nignore = [1]\n".to_string(),
    ] {
        let d = root(&body);
        match load_compare_ignore(d.path()) {
            Err(StorageError::Invalid { key, .. }) => assert_eq!(key, "compare.ignore"),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn compare_ignore_save_rejects_oversize() {
    let d = root("format = 1\n");
    let many = vec!["header:a".to_string(); 257];
    assert!(save_compare_ignore(d.path(), &many).is_err());
    assert!(save_compare_ignore(d.path(), &["x".repeat(1025)]).is_err());
}

#[test]
fn compare_ignore_survives_save_collection_defaults() {
    let d = root("format = 1\n[compare]\nignore = [\"header:date\"]\n");
    let c = load_collection_defaults(d.path()).unwrap();
    save_collection_defaults(d.path(), &c).unwrap();
    assert_eq!(
        load_compare_ignore(d.path()).unwrap(),
        vec!["header:date".to_string()]
    );
}

#[test]
fn compare_ignore_save_keeps_comments_on_the_ignore_key() {
    let d = root("format = 1\n[compare]\n# lead\nignore = [\"header:a\"] # trail\n");
    save_compare_ignore(d.path(), &["header:b".into()]).unwrap();
    assert_eq!(
        text(&d),
        "format = 1\n[compare]\n# lead\nignore = [\"header:b\"] # trail\n"
    );
}
