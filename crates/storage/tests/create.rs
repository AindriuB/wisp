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
