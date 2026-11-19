//! H3b: the binding registry and its markers (task 185).

use std::path::{Path, PathBuf};

use wisp_storage::collection::CollectionId;
use wisp_storage::data_dir::DataDir;
use wisp_storage::vault::{
    SecretsError, bind, bind_additional_checkout, bound_roots, rebind, share_with_folder,
};
use wisp_storage::vault_binding::tracked_marker_warning;

fn id() -> CollectionId {
    CollectionId::parse("abcdef0123456789").unwrap()
}

fn other_id() -> CollectionId {
    CollectionId::parse("0123456789abcdef").unwrap()
}

fn data() -> DataDir {
    DataDir::at(tempfile::tempdir().unwrap().keep())
}

fn carry_id(root: &Path, id: &CollectionId) {
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(
        root.join("collection.toml"),
        format!("format = 1\nid = \"{}\"\n", id.as_str()),
    )
    .unwrap();
}

fn root() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let p = std::fs::canonicalize(d.path()).unwrap();
    carry_id(&p, &id());
    (d, p)
}

fn marker(root: &Path) -> PathBuf {
    root.join("environments/.wisp-binding")
}

fn marker_keys(root: &Path) -> Vec<String> {
    std::fs::read_to_string(marker(root))
        .unwrap()
        .lines()
        .filter_map(|l| l.split_once(' ').map(|(k, _)| k.to_string()))
        .collect()
}

fn registry_id(d: &DataDir) -> String {
    let text = std::fs::read_to_string(d.root().join("vault-bindings.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    v["registry_id"].as_str().unwrap().to_string()
}

#[test]
fn h3b_rebind_refused_while_other_root_live() {
    let (_a, a) = root();
    let (_b, b) = root();
    let (_c, c) = root();
    let d = data();
    bind(&d, &id(), &a).unwrap();
    share_with_folder(&d, &id(), &a, &b).unwrap();
    // One bound root is gone; the other is still live.
    drop(_b);
    assert!(!b.exists());
    let e = rebind(&d, &id(), &c).unwrap_err();
    match e {
        SecretsError::LooksLikeCopy { bound_root } => assert_eq!(bound_root, a),
        e => panic!("{e:?}"),
    }
    assert_eq!(bound_roots(&d, &id()).unwrap(), vec![a, b]);
}

#[test]
fn h3b_markers_share_no_id() {
    let (_a, a) = root();
    let (_b, b) = root();
    let d = data();
    bind(&d, &id(), &a).unwrap();
    share_with_folder(&d, &id(), &a, &b).unwrap();
    let (ka, kb) = (marker_keys(&a), marker_keys(&b));
    assert_eq!((ka.len(), kb.len()), (1, 1));
    assert_ne!(ka, kb);
    let reg = registry_id(&d);
    assert!(!ka.contains(&reg) && !kb.contains(&reg));
    // Both still pass the binding check.
    bind(&d, &id(), &a).unwrap();
    bind(&d, &id(), &b).unwrap();
}

#[test]
fn h3b_tracked_marker_warns() {
    let (_a, a) = root();
    let d = data();
    bind(&d, &id(), &a).unwrap();
    assert!(tracked_marker_warning(&a).is_none());
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&a)
                .args(args)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q"]);
    assert!(tracked_marker_warning(&a).is_none());
    git(&["add", "-f", "environments/.wisp-binding"]);
    assert_eq!(tracked_marker_warning(&a), Some(marker(&a)));
}

#[test]
fn h3b_oversized_marker_fails_closed() {
    let (_a, a) = root();
    let d = data();
    bind(&d, &id(), &a).unwrap();
    let before = std::fs::read_to_string(marker(&a)).unwrap();
    // Over the byte cap.
    let big = format!("{before}{}", "x".repeat(40 * 1024));
    std::fs::write(marker(&a), &big).unwrap();
    assert!(bind(&d, &id(), &a).is_err());
    assert_eq!(std::fs::read_to_string(marker(&a)).unwrap(), big);
    // Under the byte cap, over the entry cap.
    let many: String = (0..100).map(|i| format!("k{i} n{i}\n")).collect();
    std::fs::write(marker(&a), format!("{before}{many}")).unwrap();
    assert!(bind(&d, &id(), &a).is_err());
    assert_eq!(
        std::fs::read_to_string(marker(&a)).unwrap(),
        format!("{before}{many}")
    );
}

#[test]
fn h3b_malformed_entries_dropped_on_write() {
    let (_a, a) = root();
    let (_b, b) = root();
    let d = data();
    bind(&d, &id(), &a).unwrap();
    std::fs::create_dir_all(b.join("environments")).unwrap();
    std::fs::write(
        marker(&b),
        "garbage\nbad!key nonce1\nforeign123 nonce456\n\n  \nonlykey \n",
    )
    .unwrap();
    share_with_folder(&d, &id(), &a, &b).unwrap();
    let text = std::fs::read_to_string(marker(&b)).unwrap();
    let keys = marker_keys(&b);
    assert_eq!(keys.len(), 2, "{text}");
    assert!(keys.contains(&"foreign123".to_string()));
    assert!(!text.contains("garbage") && !text.contains("bad!key"));
}

/// A linked worktree entry: `.git` file -> `<main>/.git/worktrees/<name>`.
fn make_worktree(main: &Path, wt: &Path, name: &str) -> PathBuf {
    let gd = main.join(".git/worktrees").join(name);
    std::fs::create_dir_all(&gd).unwrap();
    std::fs::write(gd.join("commondir"), "../..\n").unwrap();
    std::fs::write(
        gd.join("gitdir"),
        format!("{}\n", wt.join(".git").display()),
    )
    .unwrap();
    std::fs::write(wt.join(".git"), format!("gitdir: {}\n", gd.display())).unwrap();
    gd
}

fn unverified(r: Result<(), SecretsError>) {
    assert!(
        matches!(r, Err(SecretsError::UnverifiedCheckout { .. })),
        "{r:?}"
    );
}

#[test]
fn h3b_recreated_worktree_not_verified() {
    let (_a, a) = root();
    std::fs::create_dir_all(a.join(".git")).unwrap();
    let d = data();
    bind(&d, &id(), &a).unwrap();
    // A fresh worktree verifies.
    let (_ok, ok) = root();
    make_worktree(&a, &ok, "fresh");
    bind_additional_checkout(&d, &id(), &ok).unwrap();
    // An entry from long ago whose folder was deleted by hand; something now
    // sits at that path.
    let (_w, w) = root();
    let gd = make_worktree(&a, &w, "stale");
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(86_400);
    std::fs::File::options()
        .write(true)
        .open(gd.join("gitdir"))
        .unwrap()
        .set_modified(old)
        .unwrap();
    unverified(bind_additional_checkout(&d, &id(), &w));
}

#[cfg(unix)]
#[test]
fn h3b_retargeted_component_refused() {
    let (_a, a) = root();
    std::fs::create_dir_all(a.join(".git")).unwrap();
    let d = data();
    bind(&d, &id(), &a).unwrap();
    // `p/wt` is the new root; the entry names it through the link `l -> p`.
    let p = tempfile::tempdir().unwrap();
    let p_canon = std::fs::canonicalize(p.path()).unwrap();
    let wt = p_canon.join("wt");
    carry_id(&wt, &id());
    let l = tempfile::tempdir().unwrap();
    let link = std::fs::canonicalize(l.path()).unwrap().join("l");
    std::os::unix::fs::symlink(&p_canon, &link).unwrap();
    make_worktree(&a, &link.join("wt"), "w1");
    // `make_worktree` wrote the `.git` file through the link; the entry holds
    // the link form of the path.
    assert!(wt.join(".git").exists());
    unverified(bind_additional_checkout(&d, &id(), &wt));
}

#[test]
fn h3b_planted_common_dir_refused() {
    let (_o, outer) = root();
    // The outer collection (another id) is bound; the inner one sits inside it
    // and carries a planted `.git`.
    carry_id(&outer, &other_id());
    let inner = outer.join("sub");
    carry_id(&inner, &id());
    std::fs::create_dir_all(inner.join(".git")).unwrap();
    let (_n, n) = root();
    make_worktree(&inner, &n, "w");
    let d = data();
    bind(&d, &id(), &inner).unwrap();
    // Control: with no collection root around it, the common dir is accepted.
    let (_n2, n2) = root();
    make_worktree(&inner, &n2, "w2");
    bind_additional_checkout(&d, &id(), &n2).unwrap();
    bind(&d, &other_id(), &outer).unwrap();
    unverified(bind_additional_checkout(&d, &id(), &n));
}

#[cfg(windows)]
fn verbatim_worktree(name: &str) -> (PathBuf, DataDir, PathBuf) {
    let (_a, a) = root();
    std::mem::forget(_a);
    std::fs::create_dir_all(a.join(".git")).unwrap();
    let d = data();
    bind(&d, &id(), &a).unwrap();
    let (_w, w) = root();
    std::mem::forget(_w);
    let gd = a.join(".git").join("worktrees").join(name);
    let verbatim = PathBuf::from(format!(r"\\?\{}", gd.display()));
    let _ = std::fs::create_dir_all(&verbatim);
    let _ = std::fs::write(verbatim.join("commondir"), "../..\n");
    let _ = std::fs::write(
        verbatim.join("gitdir"),
        format!("{}\n", w.join(".git").display()),
    );
    std::fs::write(w.join(".git"), format!("gitdir: {}\n", gd.display())).unwrap();
    (a, d, w)
}

#[cfg(windows)]
#[test]
fn h3b_win_com1_refused() {
    let (_a, d, w) = verbatim_worktree("COM1");
    unverified(bind_additional_checkout(&d, &id(), &w));
}

#[cfg(windows)]
#[test]
fn h3b_win_trailing_dot_refused() {
    let (_a, d, w) = verbatim_worktree("w1.");
    unverified(bind_additional_checkout(&d, &id(), &w));
}
