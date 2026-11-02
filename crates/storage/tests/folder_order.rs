//! `order` entries written as file names (`get-user.toml`) are honoured and maintained.

use std::path::Path;

use wisp_storage::collection::{CollectionTree, Node, open_collection};
use wisp_storage::ops::{delete_node, move_node, rename_request};

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

fn users(tree: &CollectionTree) -> Vec<String> {
    for n in &tree.nodes {
        if let Node::Folder { name, children, .. } = n
            && name == "users"
        {
            return children
                .iter()
                .map(|c| match c {
                    Node::Request(m) => m.name.clone(),
                    other => panic!("unexpected {other:?}"),
                })
                .collect();
        }
    }
    panic!("no users folder");
}

fn setup() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/my-api"),
        tmp.path(),
    );
    tmp
}

fn folder_toml(root: &Path) -> String {
    std::fs::read_to_string(root.join("users/folder.toml")).unwrap()
}

#[test]
fn fixture_order_honoured_and_maintained() {
    let tmp = setup();
    let root = tmp.path();
    assert_eq!(
        users(&open_collection(root).unwrap()),
        ["Get user", "Create user"]
    );

    rename_request(root, Path::new("users/get-user.toml"), "fetch-user").unwrap();
    let text = folder_toml(root);
    assert!(text.contains("# Folder defaults for the users folder."));
    assert!(
        text.contains("order = [\"fetch-user.toml\", \"create-user.toml\"]"),
        "{text}"
    );
    assert_eq!(users(&open_collection(root).unwrap())[1], "Create user");

    delete_node(root, Path::new("users/create-user.toml")).unwrap();
    assert!(
        folder_toml(root).contains("order = [\"fetch-user.toml\"]"),
        "{}",
        folder_toml(root)
    );
}

#[test]
fn move_out_removes_entry() {
    let tmp = setup();
    let root = tmp.path();
    move_node(root, Path::new("users/get-user.toml"), Path::new("")).unwrap();
    let text = folder_toml(root);
    assert!(!text.contains("get-user"), "{text}");
    assert!(text.contains("create-user.toml"), "{text}");
}
