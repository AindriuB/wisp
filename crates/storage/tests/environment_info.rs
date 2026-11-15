use std::path::Path;

use wisp_core::request::{Environment, Var};
use wisp_storage::data_dir::DataDir;
use wisp_storage::environment::{
    EnvironmentInfo, SuggestConfirm, environment_info, save_environment,
};
use wisp_storage::vault::{NoStores, StoreKind, VaultCtx, load_environment_with};

fn write(root: &Path, file: &str, text: &str) {
    let dir = root.join("environments");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(file), text).unwrap();
}

fn cx() -> VaultCtx {
    VaultCtx::new(DataDir::at(tempfile::tempdir().unwrap().keep()))
}

fn info(text: &str) -> EnvironmentInfo {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", text);
    environment_info(t.path(), "dev").unwrap()
}

fn loaded(text: &str) -> SuggestConfirm {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", text);
    load_environment_with(t.path(), "dev", None, &NoStores, &cx())
        .unwrap()
        .suggest_confirm
}

#[test]
fn hint_variants_for_info_and_loader() {
    let cases = [
        ("", SuggestConfirm::No),
        ("suggest_confirm = false\n", SuggestConfirm::No),
        ("suggest_confirm = true\n", SuggestConfirm::Yes),
        ("suggest_confirm = \"yes\"\n", SuggestConfirm::Malformed),
        ("suggest_confirm = 1\n", SuggestConfirm::Malformed),
    ];
    for (head, want) in cases {
        let text = format!("{head}[vars]\nhost = \"h\"\n");
        assert_eq!(info(&text).suggest_confirm, want, "{head}");
        assert_eq!(loaded(&text), want, "{head}");
    }
}

#[test]
fn file_mode_names() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", "[vars]\nhost = \"h\"\n");
    write(t.path(), "dev.secrets.toml", "[vars]\ntoken = \"t\"\n");
    let i = environment_info(t.path(), "dev").unwrap();
    assert_eq!(i.store, StoreKind::File);
    assert_eq!(i.var_names, ["host"]);
    assert_eq!(i.secret_names, ["token"]);
}

#[test]
fn store_mode_names_without_store() {
    let i = info("secrets = \"keychain\"\nsecret_names = [\"a\", \"b\"]\n[vars]\nhost = \"h\"\n");
    assert_eq!(i.store, StoreKind::Keychain);
    assert_eq!(i.secret_names, ["a", "b"]);
}

#[test]
fn no_values_in_debug() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", "[vars]\nhost = \"PLAIN-MARKER-1\"\n");
    write(
        t.path(),
        "dev.secrets.toml",
        "[vars]\ntoken = \"SECRET-MARKER-2\"\n",
    );
    let i = environment_info(t.path(), "dev").unwrap();
    for s in [format!("{i:?}"), format!("{i:#?}")] {
        assert!(!s.contains("MARKER"), "{s}");
    }
}

#[test]
fn bad_names_refused() {
    let t = tempfile::tempdir().unwrap();
    for n in ["../x", "a/b", "", ".hidden"] {
        assert!(environment_info(t.path(), n).is_err(), "{n}");
    }
}

#[test]
fn save_preserves_hint() {
    let t = tempfile::tempdir().unwrap();
    write(
        t.path(),
        "dev.toml",
        "suggest_confirm = true\n[vars]\nhost = \"h\"\n",
    );
    let env = Environment {
        name: "dev".into(),
        vars: vec![Var {
            name: "host".into(),
            value: "h2".into(),
        }],
        secrets: vec![],
    };
    save_environment(t.path(), &env).unwrap();
    let i = environment_info(t.path(), "dev").unwrap();
    assert_eq!(i.suggest_confirm, SuggestConfirm::Yes);
    assert_eq!(i.var_names, ["host"]);
}
