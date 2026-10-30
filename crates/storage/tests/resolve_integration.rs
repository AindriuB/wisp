//! End to end: fixture collection on disk through load, scope and resolve, with no window.

use std::path::{Path, PathBuf};
use std::time::Duration;

use wisp_core::request::{Auth, Body};
use wisp_core::resolve::resolve;
use wisp_storage::collection::{
    load_collection_defaults, load_folder_defaults, open_collection, save_collection_defaults,
    save_folder_defaults,
};
use wisp_storage::environment::{load_environment, save_environment};
use wisp_storage::request::{load_request, save_request};
use wisp_storage::watcher::{Changed, Watcher};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/my-api")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dest);
        } else {
            std::fs::copy(entry.path(), dest).unwrap();
        }
    }
}

fn files(root: &Path, rel: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(root.join(rel)).unwrap() {
        let entry = entry.unwrap();
        let r = rel.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            files(root, &r, out);
        } else {
            out.push(r);
        }
    }
}

fn all_files(root: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    files(root, Path::new(""), &mut v);
    v.sort();
    v
}

#[test]
fn fixture_resolves_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    copy_dir(&fixture(), tmp.path());
    let tree = open_collection(tmp.path()).unwrap();
    let req_path = tree.root.join("users/create-user.toml");
    let request = load_request(&req_path).unwrap();
    let env = load_environment(&tree.root, "dev").unwrap();
    let scope = tree.scope_for(&req_path, Some(&env));
    let prepared = resolve(&request, &scope).unwrap();

    assert_eq!(
        prepared.url,
        "https://api.example.com/orgs/acme/files/report.pdf?notify=true"
    );
    let header = |n: &str| {
        prepared
            .headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(n))
            .map(|h| h.value.clone())
    };
    assert_eq!(
        header("authorization").as_deref(),
        Some("Bearer fixture-not-a-secret")
    );
    assert_eq!(header("x-trace"), None);
    assert_eq!(header("content-type").as_deref(), Some("application/json"));
    let sidecar = std::fs::read(fixture().join("users/create-user.body.json")).unwrap();
    assert_eq!(prepared.body, wisp_core::PreparedBody::Bytes(sidecar));
}

#[test]
fn every_file_round_trips_byte_for_byte() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    copy_dir(&fixture(), root);

    let defaults = load_collection_defaults(root).unwrap();
    assert!(matches!(defaults.auth, Auth::Bearer { .. }));
    save_collection_defaults(root, &defaults).unwrap();
    let folder = load_folder_defaults(&root.join("users")).unwrap();
    save_folder_defaults(&root.join("users"), &folder).unwrap();
    for rel in ["users/get-user.toml", "users/create-user.toml"] {
        let p = root.join(rel);
        let r = load_request(&p).unwrap();
        if rel.contains("create") {
            assert!(matches!(r.body, Body::Json(_)));
        }
        save_request(&p, &r).unwrap();
    }
    let env = load_environment(root, "dev").unwrap();
    save_environment(root, &env).unwrap();

    let expected = all_files(&fixture());
    assert_eq!(all_files(root), expected, "file set changed");
    for rel in expected {
        assert_eq!(
            std::fs::read(root.join(&rel)).unwrap(),
            std::fs::read(fixture().join(&rel)).unwrap(),
            "{} differs",
            rel.display()
        );
    }
}

/// Drain events until `want` arrives; false on timeout or a closed channel.
fn wait_for(rx: &async_channel::Receiver<Changed>, want: &Changed) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        match rx.try_recv() {
            Ok(c) if &c == want => return true,
            Ok(_) => {}
            Err(async_channel::TryRecvError::Empty) => std::thread::yield_now(),
            Err(async_channel::TryRecvError::Closed) => return false,
        }
    }
    false
}

#[test]
fn watcher_sees_external_edit() {
    let tmp = tempfile::tempdir().unwrap();
    copy_dir(&fixture(), tmp.path());
    let (_watcher, rx) = Watcher::start(tmp.path()).unwrap();

    // Events from the copy above can still be in flight. Write a sentinel and
    // drain until it arrives, so the edit below is the only thing left to see.
    std::fs::write(tmp.path().join("users/sentinel.toml"), "# sentinel\n").unwrap();
    let sentinel = Changed {
        path: PathBuf::from("users/sentinel.toml"),
    };
    assert!(
        wait_for(&rx, &sentinel),
        "no Changed for the sentinel within 5s"
    );

    let target = tmp.path().join("users/get-user.toml");
    let mut text = std::fs::read_to_string(&target).unwrap();
    text.push_str("# edited outside the API\n");
    std::fs::write(&target, text).unwrap();

    let want = Changed {
        path: PathBuf::from("users/get-user.toml"),
    };
    assert!(
        wait_for(&rx, &want),
        "no Changed for users/get-user.toml within 5s"
    );
}
