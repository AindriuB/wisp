use std::path::Path;

use wisp_storage::ops::{create_collection, create_environment};
use wisp_storage::{StorageError, collection::open_collection, environment};

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn collection_created_and_opens() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("c");
    let p = create_collection(&dir).unwrap();
    assert_eq!(p, dir.join("collection.toml"));
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(text.contains("format = 1") && !text.contains("id"));
    assert_eq!(names(&dir), ["collection.toml"]);
    open_collection(&dir).unwrap();
}

#[test]
fn collection_refuses_existing_file_dir_symlink() {
    let t = tempfile::tempdir().unwrap();
    let a = t.path().join("a");
    std::fs::create_dir(&a).unwrap();
    std::fs::write(a.join("collection.toml"), "x").unwrap();
    assert!(matches!(
        create_collection(&a),
        Err(StorageError::Exists { .. })
    ));
    assert_eq!(
        std::fs::read_to_string(a.join("collection.toml")).unwrap(),
        "x"
    );

    let b = t.path().join("b");
    std::fs::create_dir_all(b.join("collection.toml")).unwrap();
    assert!(matches!(
        create_collection(&b),
        Err(StorageError::Exists { .. })
    ));
    assert_eq!(names(&b), ["collection.toml"]);

    #[cfg(unix)]
    {
        let c = t.path().join("c");
        std::fs::create_dir(&c).unwrap();
        std::os::unix::fs::symlink(t.path().join("nowhere"), c.join("collection.toml")).unwrap();
        assert!(matches!(
            create_collection(&c),
            Err(StorageError::Exists { .. })
        ));
        assert!(!t.path().join("nowhere").exists());
    }
}

#[test]
fn environment_created() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    let p = create_environment(root, "dev").unwrap();
    assert_eq!(
        p,
        root.canonicalize().unwrap().join("environments/dev.toml")
    );
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "");
    let e = environment::load_environment(root, "dev").unwrap();
    assert_eq!(e.name, "dev");
    assert!(e.vars.is_empty() && e.secrets.is_empty());
    assert!(
        environment::list_environments(root)
            .unwrap()
            .contains(&"dev".to_string())
    );
    assert_eq!(names(root), ["environments"]);
    assert_eq!(names(&root.join("environments")), ["dev.toml"]);
}

#[test]
fn environment_refuses() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    for bad in ["..", "a/b", "a\\b", "CON", ""] {
        assert!(create_environment(root, bad).is_err(), "{bad}");
    }
    assert!(names(root).is_empty());
    create_environment(root, "dev").unwrap();
    assert!(matches!(
        create_environment(root, "dev"),
        Err(StorageError::Exists { .. })
    ));
    let envs = root.join("environments");
    std::fs::write(envs.join("x.secrets.toml"), "s").unwrap();
    assert!(matches!(
        create_environment(root, "x"),
        Err(StorageError::Exists { .. })
    ));
    assert_eq!(names(&envs), ["dev.toml", "x.secrets.toml"]);
}

#[cfg(unix)]
#[test]
fn environment_refuses_symlinked_dir() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("root");
    let out = t.path().join("out");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&out).unwrap();
    std::os::unix::fs::symlink(&out, root.join("environments")).unwrap();
    assert!(create_environment(&root, "dev").is_err());
    assert!(names(&out).is_empty());
}

fn h5_barrier_ids(n: usize) {
    use std::sync::{Arc, Barrier};
    for _ in 0..n {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().to_path_buf();
        let b = Arc::new(Barrier::new(2));
        let hs: Vec<_> = (0..2)
            .map(|_| {
                let (b, r) = (b.clone(), root.clone());
                std::thread::spawn(move || {
                    b.wait();
                    wisp_storage::collection::ensure_collection_id(&r).unwrap()
                })
            })
            .collect();
        let ids: Vec<_> = hs.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(ids[0], ids[1]);
        let on_disk = wisp_storage::collection::read_collection_id(&root).unwrap();
        assert_eq!(on_disk, Some(ids[0].clone()));
    }
}

#[test]
fn h5_concurrent_ensure_id_agrees() {
    h5_barrier_ids(50);
}

#[test]
fn h5_ensure_id_and_save_defaults_keep_both() {
    use std::sync::{Arc, Barrier};
    use wisp_core::request::CollectionDefaults;
    for _ in 0..50 {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().to_path_buf();
        let b = Arc::new(Barrier::new(2));
        let (b1, r1) = (b.clone(), root.clone());
        let h1 = std::thread::spawn(move || {
            b1.wait();
            wisp_storage::collection::ensure_collection_id(&r1).unwrap()
        });
        let h2 = std::thread::spawn(move || {
            let mut d = CollectionDefaults::default();
            d.vars.push(wisp_core::request::Var {
                name: "k".into(),
                value: "v".into(),
            });
            b.wait();
            wisp_storage::collection::save_collection_defaults(&root, &d).unwrap();
            root
        });
        let id = h1.join().unwrap();
        let root = h2.join().unwrap();
        assert_eq!(
            wisp_storage::collection::read_collection_id(&root).unwrap(),
            Some(id)
        );
        let d = wisp_storage::collection::load_collection_defaults(&root).unwrap();
        assert_eq!(d.vars.len(), 1);
    }
}

#[cfg(unix)]
#[test]
fn h5_trailing_slash_symlink_refused() {
    use wisp_storage::data_dir::ensure_private_dir;
    let t = tempfile::tempdir().unwrap();
    let real = t.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let link = t.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    assert!(ensure_private_dir(&link).is_err());
    let mut with_slash = link.into_os_string();
    with_slash.push("/");
    assert!(ensure_private_dir(Path::new(&with_slash)).is_err());
}

#[cfg(unix)]
#[test]
fn h5_wide_dir_tightened_and_file_refused() {
    use std::os::unix::fs::PermissionsExt;
    use wisp_storage::data_dir::ensure_private_dir;
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("d");
    std::fs::create_dir(&d).unwrap();
    std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
    ensure_private_dir(&d).unwrap();
    let mode = std::fs::metadata(&d).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    let f = t.path().join("f");
    std::fs::write(&f, b"x").unwrap();
    assert!(ensure_private_dir(&f).is_err());
}
