use std::path::Path;

use wisp_core::request::{Environment, Var};
use wisp_storage::collection::CollectionId;
use wisp_storage::data_dir::DataDir;
use wisp_storage::environment::{load_environment, save_environment};
use wisp_storage::vault::{
    FailReason, LoadStatus, MemoryStore, NoStores, SecretStore, SecretStoreError, SecretValue,
    SecretsError, StoreKind, UnavailableReason, VaultCtx, bind, delete_secret,
    load_environment_with, migrate_to_file, migrate_to_store, refresh_token_account,
    secret_account, set_secret, store_state_account,
};

const SENTINEL: &str = "s3ntinel-value-xyz";

fn cx() -> VaultCtx {
    VaultCtx::new(DataDir::at(tempfile::tempdir().unwrap().keep()))
}

/// A ctx whose registry already binds `id()` to `root`.
fn bcx(root: &Path) -> VaultCtx {
    let c = cx();
    bind(&c.data, &id(), root).unwrap();
    c
}

fn id() -> CollectionId {
    CollectionId::parse("abcdef0123456789").unwrap()
}

fn write(root: &Path, file: &str, text: &str) {
    let dir = root.join("environments");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(file), text).unwrap();
}

fn read(root: &Path, file: &str) -> String {
    std::fs::read_to_string(root.join("environments").join(file)).unwrap()
}

fn exists(root: &Path, file: &str) -> bool {
    root.join("environments").join(file).exists()
}

const STORE_TOML: &str = "# keep me\nsecrets = \"keychain\"\nsecret_names = [\"token\", \"key\"]\n\n[vars]\nhost = \"h\"\n";

fn acct(var: &str) -> String {
    secret_account(&id(), "dev", var)
}

fn put(store: &MemoryStore, var: &str, value: &str) {
    store.set(&acct(var), &SecretValue::new(value)).unwrap();
}

fn file_env(root: &Path) {
    write(root, "dev.toml", "# keep me\n[vars]\nhost = \"h\"\n");
    write(
        root,
        "dev.secrets.toml",
        "[vars]\ntoken = \"t1\"\nkey = \"k1\"\n",
    );
}

fn load(root: &Path, store: &MemoryStore) -> wisp_storage::vault::LoadedEnvironment {
    load_environment_with(root, "dev", Some(&id()), store, &bcx(root)).unwrap()
}

fn var(n: &str, v: &str) -> Var {
    Var {
        name: n.into(),
        value: v.into(),
    }
}

// ---- load table ----

#[test]
fn file_mode_does_not_probe() {
    let t = tempfile::tempdir().unwrap();
    file_env(t.path());
    let s = MemoryStore::new();
    s.fail_probe(Some(SecretStoreError::Denied));
    let l = load(t.path(), &s);
    assert_eq!(l.status, LoadStatus::File);
    assert_eq!(l.env.secrets.len(), 2);
}

#[test]
fn unknown_kind_fails_without_reading_file() {
    let t = tempfile::tempdir().unwrap();
    write(
        t.path(),
        "dev.toml",
        "secrets = \"keepassxc\"\nsecret_names = [\"token\"]\n[vars]\n",
    );
    write(
        t.path(),
        "dev.secrets.toml",
        &format!("[vars]\ntoken = \"{SENTINEL}\"\n"),
    );
    let l = load(t.path(), &MemoryStore::new());
    assert_eq!(
        l.status,
        LoadStatus::Failed {
            reason: FailReason::UnknownStoreKind("keepassxc".into())
        }
    );
    assert!(l.env.secrets.is_empty());
    assert_eq!(l.unresolved, vec!["token".to_string()]);
}

#[test]
fn absent_store_falls_back_to_file() {
    for reason in [
        UnavailableReason::Portable,
        UnavailableReason::NoService,
        UnavailableReason::NotConfigured,
    ] {
        let t = tempfile::tempdir().unwrap();
        write(t.path(), "dev.toml", STORE_TOML);
        write(t.path(), "dev.secrets.toml", "[vars]\ntoken = \"t1\"\n");
        let s = MemoryStore::new();
        s.fail_probe(Some(SecretStoreError::Unavailable { reason }));
        let l = load(t.path(), &s);
        assert_eq!(l.status, LoadStatus::FellBackToFile { reason });
        assert_eq!(l.env.secrets, vec![var("token", "t1")]);
    }
}

#[test]
fn no_store_for_kind_falls_back_as_not_configured() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", STORE_TOML);
    write(t.path(), "dev.secrets.toml", "[vars]\ntoken = \"t1\"\n");
    let l = load_environment_with(t.path(), "dev", Some(&id()), &NoStores, &cx()).unwrap();
    assert_eq!(
        l.status,
        LoadStatus::FellBackToFile {
            reason: UnavailableReason::NotConfigured
        }
    );
    assert_eq!(l.env.secrets, vec![var("token", "t1")]);
}

#[test]
fn timed_out_probe_fails_closed() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", STORE_TOML);
    write(
        t.path(),
        "dev.secrets.toml",
        &format!("[vars]\ntoken = \"{SENTINEL}\"\n"),
    );
    let s = MemoryStore::new();
    let e = SecretStoreError::Unavailable {
        reason: UnavailableReason::TimedOut,
    };
    s.fail_probe(Some(e.clone()));
    let l = load(t.path(), &s);
    assert_eq!(
        l.status,
        LoadStatus::Failed {
            reason: FailReason::Store(e)
        }
    );
    assert!(l.env.secrets.is_empty());
}

#[test]
fn locked_denied_other_probe_fail_closed() {
    for e in [
        SecretStoreError::Locked,
        SecretStoreError::Denied,
        SecretStoreError::Other { code: Some(7) },
    ] {
        let t = tempfile::tempdir().unwrap();
        write(t.path(), "dev.toml", STORE_TOML);
        write(
            t.path(),
            "dev.secrets.toml",
            &format!("[vars]\ntoken = \"{SENTINEL}\"\n"),
        );
        let s = MemoryStore::new();
        s.fail_probe(Some(e.clone()));
        let l = load(t.path(), &s);
        assert_eq!(
            l.status,
            LoadStatus::Failed {
                reason: FailReason::Store(e)
            }
        );
        assert!(l.env.secrets.is_empty());
    }
}

#[test]
fn store_ok_without_collection_id_fails() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", STORE_TOML);
    let l = load_environment_with(t.path(), "dev", None, &MemoryStore::new(), &cx()).unwrap();
    assert_eq!(
        l.status,
        LoadStatus::Failed {
            reason: FailReason::NoCollectionId
        }
    );
    assert_eq!(l.unresolved.len(), 2);
}

#[test]
fn store_ok_resolves_and_marks_missing_unresolved() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", STORE_TOML);
    let s = MemoryStore::new();
    put(&s, "token", "t1");
    let l = load(t.path(), &s);
    assert_eq!(
        l.status,
        LoadStatus::Store {
            kind: StoreKind::Keychain,
            file_ignored: false
        }
    );
    assert_eq!(l.env.secrets, vec![var("token", "t1")]);
    assert_eq!(l.unresolved, vec!["key".to_string()]);
    assert_eq!(l.env.vars, vec![var("host", "h")]);
}

#[test]
fn store_mode_ignores_present_secrets_file() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", STORE_TOML);
    write(
        t.path(),
        "dev.secrets.toml",
        &format!("[vars]\ntoken = \"{SENTINEL}\"\n"),
    );
    let s = MemoryStore::new();
    put(&s, "token", "t1");
    let l = load(t.path(), &s);
    assert_eq!(
        l.status,
        LoadStatus::Store {
            kind: StoreKind::Keychain,
            file_ignored: true
        }
    );
    assert_eq!(l.env.secrets, vec![var("token", "t1")]);
}

#[test]
fn get_error_fails_closed_for_every_error() {
    for e in [
        SecretStoreError::Unavailable {
            reason: UnavailableReason::NoService,
        },
        SecretStoreError::Unavailable {
            reason: UnavailableReason::TimedOut,
        },
        SecretStoreError::Locked,
        SecretStoreError::Denied,
        SecretStoreError::TooLarge,
        SecretStoreError::Other { code: None },
    ] {
        let t = tempfile::tempdir().unwrap();
        write(t.path(), "dev.toml", STORE_TOML);
        write(
            t.path(),
            "dev.secrets.toml",
            &format!("[vars]\ntoken = \"{SENTINEL}\"\n"),
        );
        let s = MemoryStore::new();
        put(&s, "token", "t1");
        put(&s, "key", "k1");
        s.fail_nth(1, e.clone());
        let l = load(t.path(), &s);
        assert_eq!(
            l.status,
            LoadStatus::Failed {
                reason: FailReason::Store(e)
            }
        );
        assert!(l.env.secrets.is_empty());
    }
}

#[test]
fn store_kind_round_trips() {
    for k in [StoreKind::File, StoreKind::Keychain] {
        assert_eq!(StoreKind::parse(k.as_str()), Some(k));
    }
    assert_eq!(StoreKind::parse("keepassxc"), None);
    assert_eq!(StoreKind::parse(""), None);
}

// ---- migrate_to_store ----

#[test]
fn migrate_to_store_success() {
    let t = tempfile::tempdir().unwrap();
    file_env(t.path());
    let s = MemoryStore::new();
    migrate_to_store(
        t.path(),
        "dev",
        &id(),
        StoreKind::Keychain,
        &s,
        &bcx(t.path()),
    )
    .unwrap();
    assert!(!exists(t.path(), "dev.secrets.toml"));
    let plain = read(t.path(), "dev.toml");
    assert!(plain.contains("# keep me"));
    assert!(plain.contains("secrets = \"keychain\""));
    assert!(plain.contains("secret_names"));
    assert!(!plain.contains("t1"));
    assert_eq!(s.accounts(), {
        let mut v = vec![acct("key"), acct("token")];
        v.sort();
        v
    });
    let l = load(t.path(), &s);
    assert_eq!(l.env.secrets.len(), 2);
}

#[test]
fn migrate_to_store_probe_failure_changes_nothing() {
    let t = tempfile::tempdir().unwrap();
    file_env(t.path());
    let before = read(t.path(), "dev.toml");
    let s = MemoryStore::new();
    s.fail_probe(Some(SecretStoreError::Locked));
    assert!(
        migrate_to_store(
            t.path(),
            "dev",
            &id(),
            StoreKind::Keychain,
            &s,
            &bcx(t.path())
        )
        .is_err()
    );
    assert_eq!(read(t.path(), "dev.toml"), before);
    assert!(exists(t.path(), "dev.secrets.toml"));
    assert!(s.accounts().is_empty());
}

#[test]
fn migrate_to_store_set_failure_rolls_back_created_entries() {
    // Calls: get(token) set(token) get(token) get(key) set(key) ...
    for n in [1, 4] {
        let t = tempfile::tempdir().unwrap();
        file_env(t.path());
        let before = read(t.path(), "dev.toml");
        let s = MemoryStore::new();
        s.fail_nth(n, SecretStoreError::Denied);
        assert!(
            migrate_to_store(
                t.path(),
                "dev",
                &id(),
                StoreKind::Keychain,
                &s,
                &bcx(t.path())
            )
            .is_err()
        );
        assert_eq!(read(t.path(), "dev.toml"), before);
        assert!(exists(t.path(), "dev.secrets.toml"));
        assert!(s.accounts().is_empty(), "n={n}: {:?}", s.accounts());
    }
}

#[test]
fn migrate_to_store_read_back_failure_rolls_back() {
    let t = tempfile::tempdir().unwrap();
    file_env(t.path());
    let s = MemoryStore::new();
    s.fail_nth(2, SecretStoreError::Other { code: Some(1) });
    assert!(
        migrate_to_store(
            t.path(),
            "dev",
            &id(),
            StoreKind::Keychain,
            &s,
            &bcx(t.path())
        )
        .is_err()
    );
    assert!(exists(t.path(), "dev.secrets.toml"));
    assert!(s.accounts().is_empty());
}

#[test]
fn migrate_to_store_rollback_keeps_preexisting_entries() {
    let t = tempfile::tempdir().unwrap();
    file_env(t.path());
    let s = MemoryStore::new();
    put(&s, "token", "old");
    // token: get, set, get; key: get, set(fail)
    s.fail_nth(4, SecretStoreError::Denied);
    assert!(
        migrate_to_store(
            t.path(),
            "dev",
            &id(),
            StoreKind::Keychain,
            &s,
            &bcx(t.path())
        )
        .is_err()
    );
    assert_eq!(s.accounts(), vec![acct("token")]);
    assert_eq!(
        s.get(&acct("token")).unwrap(),
        Some(SecretValue::new("old"))
    );
    assert!(exists(t.path(), "dev.secrets.toml"));
}

/// A store that returns a different value on every read.
struct LyingStore(MemoryStore);

impl SecretStore for LyingStore {
    fn probe(&self) -> Result<(), SecretStoreError> {
        self.0.probe()
    }
    fn get(&self, account: &str) -> Result<Option<SecretValue>, SecretStoreError> {
        Ok(self.0.get(account)?.map(|_| SecretValue::new("different")))
    }
    fn set(&self, account: &str, value: &SecretValue) -> Result<(), SecretStoreError> {
        self.0.set(account, value)
    }
    fn delete(&self, account: &str) -> Result<(), SecretStoreError> {
        self.0.delete(account)
    }
}

#[test]
fn migrate_to_store_read_back_mismatch_rolls_back() {
    let t = tempfile::tempdir().unwrap();
    file_env(t.path());
    let before = read(t.path(), "dev.toml");
    let s = LyingStore(MemoryStore::new());
    assert!(
        migrate_to_store(
            t.path(),
            "dev",
            &id(),
            StoreKind::Keychain,
            &s,
            &bcx(t.path())
        )
        .is_err()
    );
    assert_eq!(read(t.path(), "dev.toml"), before);
    assert!(exists(t.path(), "dev.secrets.toml"));
    assert!(s.0.accounts().is_empty());
}

/// Runs `hook` on the `nth` (1-based) call to `get`.
struct HookStore {
    inner: MemoryStore,
    nth: usize,
    calls: std::sync::atomic::AtomicUsize,
    hook: Box<dyn Fn() + Send + Sync>,
}

impl SecretStore for HookStore {
    fn probe(&self) -> Result<(), SecretStoreError> {
        self.inner.probe()
    }
    fn get(&self, a: &str) -> Result<Option<SecretValue>, SecretStoreError> {
        let r = self.inner.get(a);
        if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1 == self.nth {
            (self.hook)();
        }
        r
    }
    fn set(&self, a: &str, v: &SecretValue) -> Result<(), SecretStoreError> {
        self.inner.set(a, v)
    }
    fn delete(&self, a: &str) -> Result<(), SecretStoreError> {
        self.inner.delete(a)
    }
}

#[test]
fn migrate_to_store_final_delete_failure_is_a_warning() {
    let t = tempfile::tempdir().unwrap();
    file_env(t.path());
    let sp = t.path().join("environments").join("dev.secrets.toml");
    let hook_sp = sp.clone();
    // After the last read-back, swap the secrets file for a directory so the
    // final delete fails while the earlier steps succeed.
    let s = HookStore {
        inner: MemoryStore::new(),
        nth: 4,
        calls: Default::default(),
        hook: Box::new(move || {
            std::fs::remove_file(&hook_sp).unwrap();
            std::fs::create_dir(&hook_sp).unwrap();
            std::fs::write(hook_sp.join("x"), "x").unwrap();
        }),
    };
    let w = migrate_to_store(
        t.path(),
        "dev",
        &id(),
        StoreKind::Keychain,
        &s,
        &bcx(t.path()),
    )
    .unwrap()
    .expect("a warning naming the leftover file");
    assert_eq!(w.path, sp);
    assert!(read(t.path(), "dev.toml").contains("secrets = \"keychain\""));
    assert_eq!(s.inner.accounts().len(), 2);
}

#[test]
fn migrate_to_store_success_has_no_warning() {
    let t = tempfile::tempdir().unwrap();
    file_env(t.path());
    let s = MemoryStore::new();
    let w = migrate_to_store(
        t.path(),
        "dev",
        &id(),
        StoreKind::Keychain,
        &s,
        &bcx(t.path()),
    )
    .unwrap();
    assert!(w.is_none());
}

#[test]
fn migrate_to_file_plain_write_failure_restores_existing_secrets_file() {
    let t = tempfile::tempdir().unwrap();
    let s0 = MemoryStore::new();
    store_env(t.path(), &s0);
    write(t.path(), "dev.secrets.toml", "[vars]\nold = \"o\"\n");
    let pp = t.path().join("environments").join("dev.toml");
    let hook_pp = pp.clone();
    // After the last read, replace dev.toml with a directory so writing the
    // plain file fails after the secrets file was already replaced.
    let s = HookStore {
        inner: s0,
        nth: 2,
        calls: Default::default(),
        hook: Box::new(move || {
            std::fs::remove_file(&hook_pp).unwrap();
            std::fs::create_dir(&hook_pp).unwrap();
        }),
    };
    assert!(migrate_to_file(t.path(), "dev", &id(), &s, false, &bcx(t.path())).is_err());
    assert_eq!(read(t.path(), "dev.secrets.toml"), "[vars]\nold = \"o\"\n");
    assert_eq!(s.inner.accounts().len(), 2);
}

#[cfg(unix)]
#[test]
fn migrate_to_store_write_failure_rolls_back() {
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir().unwrap();
    file_env(t.path());
    let dir = t.path().join("environments");
    let s = MemoryStore::new();
    let c = bcx(t.path());
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    let r = migrate_to_store(t.path(), "dev", &id(), StoreKind::Keychain, &s, &c);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    if r.is_ok() {
        return; // running with privileges that ignore the mode
    }
    assert!(!read(t.path(), "dev.toml").contains("keychain"));
    assert!(exists(t.path(), "dev.secrets.toml"));
    assert!(s.accounts().is_empty());
}

// ---- migrate_to_file ----

fn store_env(root: &Path, s: &MemoryStore) {
    write(root, "dev.toml", STORE_TOML);
    put(s, "token", "t1");
    put(s, "key", "k1");
}

#[test]
fn migrate_to_file_success() {
    let t = tempfile::tempdir().unwrap();
    let s = MemoryStore::new();
    store_env(t.path(), &s);
    let warnings = migrate_to_file(t.path(), "dev", &id(), &s, false, &bcx(t.path())).unwrap();
    assert!(warnings.is_empty());
    let plain = read(t.path(), "dev.toml");
    assert!(plain.contains("# keep me"));
    assert!(plain.contains("secrets = \"file\""));
    assert!(!plain.contains("secret_names"));
    assert!(s.accounts().is_empty());
    let env = load_environment(t.path(), "dev").unwrap();
    assert_eq!(env.secrets, vec![var("token", "t1"), var("key", "k1")]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let m = std::fs::metadata(t.path().join("environments/dev.secrets.toml"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(m & 0o777, 0o600);
    }
}

#[test]
fn migrate_to_file_read_failure_changes_nothing() {
    let t = tempfile::tempdir().unwrap();
    let s = MemoryStore::new();
    store_env(t.path(), &s);
    s.fail_nth(1, SecretStoreError::Locked);
    assert!(migrate_to_file(t.path(), "dev", &id(), &s, false, &bcx(t.path())).is_err());
    assert_eq!(read(t.path(), "dev.toml"), STORE_TOML);
    assert!(!exists(t.path(), "dev.secrets.toml"));
    assert_eq!(s.accounts().len(), 2);
}

#[test]
fn migrate_to_file_delete_failure_is_a_warning() {
    let t = tempfile::tempdir().unwrap();
    let s = MemoryStore::new();
    store_env(t.path(), &s);
    // two gets succeed, first delete fails
    s.fail_nth(2, SecretStoreError::Denied);
    let warnings = migrate_to_file(t.path(), "dev", &id(), &s, false, &bcx(t.path())).unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(exists(t.path(), "dev.secrets.toml"));
    assert!(read(t.path(), "dev.toml").contains("secrets = \"file\""));
}

// ---- set / delete ----

#[test]
fn set_secret_writes_store_then_names() {
    let t = tempfile::tempdir().unwrap();
    write(
        t.path(),
        "dev.toml",
        "secrets = \"keychain\"\nsecret_names = []\n[vars]\n",
    );
    let s = MemoryStore::new();
    set_secret(
        t.path(),
        "dev",
        &id(),
        &s,
        "token",
        &SecretValue::new("v"),
        &bcx(t.path()),
    )
    .unwrap();
    assert!(read(t.path(), "dev.toml").contains("\"token\""));
    assert_eq!(s.accounts(), vec![acct("token")]);
    assert!(!exists(t.path(), "dev.secrets.toml"));
}

#[test]
fn set_secret_store_failure_leaves_names() {
    let t = tempfile::tempdir().unwrap();
    write(
        t.path(),
        "dev.toml",
        "secrets = \"keychain\"\nsecret_names = []\n[vars]\n",
    );
    let s = MemoryStore::new();
    s.fail_next(SecretStoreError::TooLarge);
    assert!(
        set_secret(
            t.path(),
            "dev",
            &id(),
            &s,
            "token",
            &SecretValue::new("v"),
            &cx()
        )
        .is_err()
    );
    assert!(!read(t.path(), "dev.toml").contains("token"));
    assert!(!exists(t.path(), "dev.secrets.toml"));
}

#[test]
fn delete_secret_removes_store_then_name() {
    let t = tempfile::tempdir().unwrap();
    let s = MemoryStore::new();
    store_env(t.path(), &s);
    delete_secret(t.path(), "dev", &id(), &s, "token", &bcx(t.path())).unwrap();
    let plain = read(t.path(), "dev.toml");
    assert!(!plain.contains("\"token\""));
    assert!(plain.contains("\"key\""));
    assert_eq!(s.accounts(), vec![acct("key")]);
}

#[test]
fn delete_secret_store_failure_leaves_names() {
    let t = tempfile::tempdir().unwrap();
    let s = MemoryStore::new();
    store_env(t.path(), &s);
    s.fail_next(SecretStoreError::Locked);
    assert!(delete_secret(t.path(), "dev", &id(), &s, "token", &bcx(t.path())).is_err());
    assert_eq!(read(t.path(), "dev.toml"), STORE_TOML);
}

// ---- accounts ----

#[test]
fn accounts_differ_on_slash_and_percent() {
    let i = id();
    assert_eq!(secret_account(&i, "dev", "a"), "abcdef0123456789/dev/a");
    assert_ne!(
        secret_account(&i, "dev", "a/b"),
        secret_account(&i, "dev/a", "b")
    );
    assert_ne!(
        secret_account(&i, "dev", "a%2Fb"),
        secret_account(&i, "dev", "a/b")
    );
    assert_ne!(
        refresh_token_account(&i, "dev", "g", "u/v", "c", "s"),
        refresh_token_account(&i, "dev", "g", "u", "v/c", "s")
    );
    assert_ne!(
        refresh_token_account(&i, "dev", "g", "u%2Fv", "c", "s"),
        refresh_token_account(&i, "dev", "g", "u/v", "c", "s")
    );
}

#[test]
fn store_state_account_is_reserved_prefix() {
    let a = store_state_account(StoreKind::Keychain, "assoc");
    assert!(a.starts_with(".store/"));
    for (e, v) in [("dev", "x"), (".store", "x"), ("a/b", "c/d")] {
        assert!(!secret_account(&id(), e, v).starts_with(".store/"));
    }
    assert!(!refresh_token_account(&id(), "e", "g", "u", "c", "s").starts_with(".store/"));
}

// ---- redaction and leaks ----

#[test]
fn debug_redacts_secret_value() {
    let v = SecretValue::new(SENTINEL);
    assert!(!format!("{v:?}").contains(SENTINEL));
    assert!(!format!("{v:#?}").contains(SENTINEL));
    assert!(format!("{v:?}").contains("<redacted>"));
}

#[test]
fn debug_redacts_memory_store() {
    let s = MemoryStore::new();
    s.set("a/b/c", &SecretValue::new(SENTINEL)).unwrap();
    assert!(!format!("{s:?}").contains(SENTINEL));
    assert!(!format!("{s:#?}").contains(SENTINEL));
}

#[test]
fn no_secret_in_error_messages() {
    let s = MemoryStore::new();
    s.set("a", &SecretValue::new(SENTINEL)).unwrap();
    for e in [
        SecretStoreError::Unavailable {
            reason: UnavailableReason::Portable,
        },
        SecretStoreError::Unavailable {
            reason: UnavailableReason::NoService,
        },
        SecretStoreError::Unavailable {
            reason: UnavailableReason::NotConfigured,
        },
        SecretStoreError::Unavailable {
            reason: UnavailableReason::TimedOut,
        },
        SecretStoreError::Locked,
        SecretStoreError::Denied,
        SecretStoreError::TooLarge,
        SecretStoreError::Other { code: Some(3) },
    ] {
        s.fail_next(e.clone());
        let got = s.get("a").unwrap_err();
        assert_eq!(got, e);
        assert!(!got.to_string().contains(SENTINEL));
        assert!(!format!("{got:?}").contains(SENTINEL));
    }
}

#[test]
fn migrate_error_messages_hold_no_secret() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", "[vars]\n");
    write(
        t.path(),
        "dev.secrets.toml",
        &format!("[vars]\ntoken = \"{SENTINEL}\"\n"),
    );
    let s = MemoryStore::new();
    s.fail_nth(1, SecretStoreError::Denied);
    let e = migrate_to_store(
        t.path(),
        "dev",
        &id(),
        StoreKind::Keychain,
        &s,
        &bcx(t.path()),
    )
    .unwrap_err();
    assert!(!e.to_string().contains(SENTINEL));
    assert!(!format!("{e:?}").contains(SENTINEL));
}

// ---- save_environment on non-file modes ----

#[test]
fn save_environment_store_mode_never_writes_secrets_file() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", STORE_TOML);
    let env = Environment {
        name: "dev".into(),
        vars: vec![var("host", "h2")],
        secrets: vec![var("token", SENTINEL)],
    };
    assert!(matches!(
        save_environment(t.path(), &env),
        Err(wisp_storage::StorageError::SecretsNeedStore { .. })
    ));
    assert!(!exists(t.path(), "dev.secrets.toml"));
    // Without secrets the plain vars are saved and the mode kept.
    let env = Environment {
        secrets: vec![],
        ..env
    };
    save_environment(t.path(), &env).unwrap();
    let plain = read(t.path(), "dev.toml");
    assert!(plain.contains("h2"));
    assert!(plain.contains("secrets = \"keychain\""));
    assert!(plain.contains("secret_names = [\"token\", \"key\"]"));
    assert!(!plain.contains(SENTINEL));
}

#[test]
fn save_environment_unknown_kind_never_writes_secrets_file() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", "secrets = \"keepassxc\"\n[vars]\n");
    let env = Environment {
        name: "dev".into(),
        vars: vec![var("host", "h")],
        secrets: vec![var("token", SENTINEL)],
    };
    assert!(matches!(
        save_environment(t.path(), &env),
        Err(wisp_storage::StorageError::SecretsNeedStore { .. })
    ));
    assert!(!exists(t.path(), "dev.secrets.toml"));
    assert!(read(t.path(), "dev.toml").contains("secrets = \"keepassxc\""));
}

#[test]
fn save_environment_non_string_secrets_never_writes_secrets_file() {
    let t = tempfile::tempdir().unwrap();
    write(
        t.path(),
        "dev.toml",
        "secrets = { kind = \"keepassxc\" }\n[vars]\n",
    );
    let env = Environment {
        name: "dev".into(),
        vars: vec![var("host", "h")],
        secrets: vec![var("token", SENTINEL)],
    };
    let _ = save_environment(t.path(), &env);
    assert!(!exists(t.path(), "dev.secrets.toml"));
    assert!(!read(t.path(), "dev.toml").contains(SENTINEL));
}

#[test]
fn load_environment_ignores_secrets_file_in_non_file_modes() {
    for toml in [STORE_TOML, "secrets = \"keepassxc\"\n[vars]\n"] {
        let t = tempfile::tempdir().unwrap();
        write(t.path(), "dev.toml", toml);
        write(
            t.path(),
            "dev.secrets.toml",
            &format!("[vars]\ntoken = \"{SENTINEL}\"\n"),
        );
        assert!(
            load_environment(t.path(), "dev")
                .unwrap()
                .secrets
                .is_empty()
        );
    }
}

// ---- review round 1 ----

use std::time::{Duration, Instant};
use wisp_storage::vault::{
    bind_additional_checkout, bound_root, bound_roots, import_secrets_file, rebind,
    share_with_folder,
};

fn bound_err(e: &SecretsError) -> bool {
    matches!(e, SecretsError::IdBoundElsewhere { .. })
}

#[test]
fn migrate_to_file_keeps_other_keys_in_existing_secrets_file() {
    let t = tempfile::tempdir().unwrap();
    let s = MemoryStore::new();
    store_env(t.path(), &s);
    write(
        t.path(),
        "dev.secrets.toml",
        "[vars]\nextra = \"keep\"\ntoken = \"old\"\n",
    );
    migrate_to_file(t.path(), "dev", &id(), &s, false, &bcx(t.path())).unwrap();
    let env = load_environment(t.path(), "dev").unwrap();
    assert!(env.secrets.contains(&var("extra", "keep")));
    assert!(env.secrets.contains(&var("token", "t1")));
    assert!(env.secrets.contains(&var("key", "k1")));
}

#[test]
fn migrate_to_file_empty_store_keeps_existing_file() {
    let t = tempfile::tempdir().unwrap();
    let s = MemoryStore::new();
    write(t.path(), "dev.toml", STORE_TOML);
    write(t.path(), "dev.secrets.toml", "[vars]\nextra = \"keep\"\n");
    migrate_to_file(t.path(), "dev", &id(), &s, true, &bcx(t.path())).unwrap();
    let env = load_environment(t.path(), "dev").unwrap();
    assert_eq!(env.secrets, vec![var("extra", "keep")]);
}

#[test]
fn migrate_to_file_unresolved_needs_confirmation() {
    let t = tempfile::tempdir().unwrap();
    let s = MemoryStore::new();
    write(t.path(), "dev.toml", STORE_TOML);
    put(&s, "token", "t1");
    let before = read(t.path(), "dev.toml");
    let e = migrate_to_file(t.path(), "dev", &id(), &s, false, &bcx(t.path())).unwrap_err();
    match e {
        SecretsError::UnresolvedNames { names } => assert_eq!(names, ["key"]),
        other => panic!("{other:?}"),
    }
    assert_eq!(read(t.path(), "dev.toml"), before);
    assert!(!exists(t.path(), "dev.secrets.toml"));
    assert_eq!(s.accounts(), vec![acct("token")]);
    migrate_to_file(t.path(), "dev", &id(), &s, true, &bcx(t.path())).unwrap();
    let env = load_environment(t.path(), "dev").unwrap();
    assert_eq!(env.secrets, vec![var("token", "t1")]);
}

#[test]
fn bound_to_other_root_fails_closed_everywhere() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let cx = bcx(a.path());
    let s = MemoryStore::new();
    store_env(a.path(), &s);
    write(b.path(), "dev.toml", STORE_TOML);
    set_secret(
        a.path(),
        "dev",
        &id(),
        &s,
        "token",
        &SecretValue::new("t2"),
        &cx,
    )
    .unwrap();
    // Same root still works.
    set_secret(
        a.path(),
        "dev",
        &id(),
        &s,
        "key",
        &SecretValue::new("k2"),
        &cx,
    )
    .unwrap();
    let before = s.accounts();
    let lb = load_environment_with(b.path(), "dev", Some(&id()), &s, &cx).unwrap();
    assert!(matches!(
        lb.status,
        LoadStatus::Failed {
            reason: FailReason::IdBoundElsewhere { .. }
        }
    ));
    assert!(lb.env.secrets.is_empty());
    let v = SecretValue::new("x");
    assert!(bound_err(
        &set_secret(b.path(), "dev", &id(), &s, "token", &v, &cx).unwrap_err()
    ));
    assert!(bound_err(
        &delete_secret(b.path(), "dev", &id(), &s, "token", &cx).unwrap_err()
    ));
    assert!(bound_err(
        &migrate_to_file(b.path(), "dev", &id(), &s, true, &cx).unwrap_err()
    ));
    write(b.path(), "dev.toml", "[vars]\n");
    write(b.path(), "dev.secrets.toml", "[vars]\ntoken = \"t\"\n");
    assert!(bound_err(
        &migrate_to_store(b.path(), "dev", &id(), StoreKind::Keychain, &s, &cx).unwrap_err()
    ));
    write(b.path(), "dev.toml", STORE_TOML);
    assert!(bound_err(
        &import_secrets_file(b.path(), "dev", &id(), &s, &cx).unwrap_err()
    ));
    assert_eq!(s.accounts(), before);
    assert_eq!(
        load_environment_with(a.path(), "dev", Some(&id()), &s, &cx)
            .unwrap()
            .env
            .secrets,
        vec![var("token", "t2"), var("key", "k2")]
    );

    // Explicit rebind moves it.
    rebind(&cx.data, &id(), b.path()).unwrap();
    assert_eq!(
        bound_root(&cx.data, &id()).unwrap(),
        Some(std::fs::canonicalize(b.path()).unwrap())
    );
    set_secret(b.path(), "dev", &id(), &s, "token", &v, &cx).unwrap();
    assert!(bound_err(
        &set_secret(a.path(), "dev", &id(), &s, "token", &v, &cx).unwrap_err()
    ));
}

#[test]
fn registry_holds_no_secret_and_is_private() {
    let t = tempfile::tempdir().unwrap();
    let cx = bcx(t.path());
    let s = MemoryStore::new();
    write(t.path(), "dev.toml", "secrets = \"keychain\"\n");
    let v = SecretValue::new(SENTINEL);
    set_secret(t.path(), "dev", &id(), &s, "token", &v, &cx).unwrap();
    let p = cx.data.root().join("vault-bindings.json");
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(!text.contains(SENTINEL));
    assert!(text.contains(id().as_str()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let m = std::fs::metadata(&p).unwrap().permissions().mode();
        assert_eq!(m & 0o777, 0o600);
    }
}

#[test]
fn corrupt_registry_fails_closed() {
    let cx = cx();
    std::fs::write(cx.data.root().join("vault-bindings.json"), "{not json").unwrap();
    let t = tempfile::tempdir().unwrap();
    let s = MemoryStore::new();
    store_env(t.path(), &s);
    assert!(load_environment_with(t.path(), "dev", Some(&id()), &s, &cx).is_err());
    let v = SecretValue::new("x");
    assert!(set_secret(t.path(), "dev", &id(), &s, "token", &v, &cx).is_err());
    assert!(bind(&cx.data, &id(), t.path()).is_err());
    assert!(delete_secret(t.path(), "dev", &id(), &s, "token", &cx).is_err());
    assert!(rebind(&cx.data, &id(), t.path()).is_err());
    assert_eq!(s.get(&acct("token")).unwrap().unwrap().expose(), "t1");
    assert_eq!(s.accounts().len(), 2);
    assert_eq!(
        std::fs::read_to_string(cx.data.root().join("vault-bindings.json")).unwrap(),
        "{not json"
    );
}

struct SlowStore(MemoryStore, Duration);

impl SecretStore for SlowStore {
    fn probe(&self) -> Result<(), SecretStoreError> {
        Ok(())
    }
    fn get(&self, a: &str) -> Result<Option<SecretValue>, SecretStoreError> {
        std::thread::sleep(self.1);
        self.0.get(a)
    }
    fn set(&self, a: &str, v: &SecretValue) -> Result<(), SecretStoreError> {
        std::thread::sleep(self.1);
        self.0.set(a, v)
    }
    fn delete(&self, a: &str) -> Result<(), SecretStoreError> {
        self.0.delete(a)
    }
}

impl wisp_storage::vault::SecretStores for SlowStore {
    fn get(&self, _kind: StoreKind) -> Option<&dyn SecretStore> {
        Some(self)
    }
}

const TIMED_OUT: SecretStoreError = SecretStoreError::Unavailable {
    reason: UnavailableReason::TimedOut,
};

fn many_names_toml() -> String {
    let names: Vec<String> = (0..50).map(|i| format!("\"n{i}\"")).collect();
    format!(
        "secrets = \"keychain\"\nsecret_names = [{}]\n",
        names.join(",")
    )
}

#[test]
fn load_has_a_whole_call_deadline() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", &many_names_toml());
    let s = SlowStore(MemoryStore::new(), Duration::from_millis(20));
    let cx = bcx(t.path()).with_deadline(Duration::from_millis(100));
    let t0 = Instant::now();
    let l = load_environment_with(t.path(), "dev", Some(&id()), &s, &cx).unwrap();
    assert!(t0.elapsed() < Duration::from_millis(600));
    assert_eq!(
        l.status,
        LoadStatus::Failed {
            reason: FailReason::Store(TIMED_OUT)
        }
    );
}

#[test]
fn migrations_have_a_whole_call_deadline() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", &many_names_toml());
    let s = SlowStore(MemoryStore::new(), Duration::from_millis(20));
    let cx = bcx(t.path()).with_deadline(Duration::from_millis(100));
    let t0 = Instant::now();
    let e = migrate_to_file(t.path(), "dev", &id(), &s, true, &cx).unwrap_err();
    assert!(t0.elapsed() < Duration::from_millis(600));
    assert!(matches!(e, SecretsError::Store(ref x) if *x == TIMED_OUT));

    let t = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", "[vars]\n");
    let vars: String = (0..50).map(|i| format!("n{i} = \"v\"\n")).collect();
    write(t.path(), "dev.secrets.toml", &format!("[vars]\n{vars}"));
    let s = SlowStore(MemoryStore::new(), Duration::from_millis(20));
    let cx = bcx(t.path()).with_deadline(Duration::from_millis(100));
    let e = migrate_to_store(t.path(), "dev", &id(), StoreKind::Keychain, &s, &cx).unwrap_err();
    assert!(matches!(e, SecretsError::Store(ref x) if *x == TIMED_OUT));
    // Rolled back: the file stays and nothing is left in the store.
    assert!(exists(t.path(), "dev.secrets.toml"));
    assert!(s.0.accounts().is_empty());
}

fn import_env(root: &Path) {
    write(root, "dev.toml", STORE_TOML);
    write(
        root,
        "dev.secrets.toml",
        "[vars]\ntoken = \"t9\"\nnewone = \"n1\"\n",
    );
}

#[test]
fn import_secrets_file_success() {
    let t = tempfile::tempdir().unwrap();
    let s = MemoryStore::new();
    import_env(t.path());
    let w = import_secrets_file(t.path(), "dev", &id(), &s, &bcx(t.path())).unwrap();
    assert!(w.is_none());
    assert!(!exists(t.path(), "dev.secrets.toml"));
    assert!(read(t.path(), "dev.toml").contains("\"newone\""));
    let l = load(t.path(), &s);
    assert_eq!(l.env.secrets, vec![var("token", "t9"), var("newone", "n1")]);
    assert_eq!(l.unresolved, ["key"]);
}

#[test]
fn import_secrets_file_failure_rolls_back() {
    let t = tempfile::tempdir().unwrap();
    let s = MemoryStore::new();
    import_env(t.path());
    put(&s, "token", "pre");
    let before = read(t.path(), "dev.toml");
    // get, set, get (verify) for token succeed; the fourth call fails.
    s.fail_nth(3, SecretStoreError::Denied);
    assert!(import_secrets_file(t.path(), "dev", &id(), &s, &bcx(t.path())).is_err());
    assert!(exists(t.path(), "dev.secrets.toml"));
    assert_eq!(read(t.path(), "dev.toml"), before);
    assert_eq!(s.get(&acct("token")).unwrap().unwrap().expose(), "pre");
    assert_eq!(s.accounts(), vec![acct("token")]);
}

#[test]
fn concurrent_set_secret_keeps_all_names() {
    let t = tempfile::tempdir().unwrap();
    let cx = bcx(t.path());
    write(t.path(), "dev.toml", "secrets = \"keychain\"\n[vars]\n");
    let s = MemoryStore::new();
    std::thread::scope(|sc| {
        for n in 0..8 {
            let (cx, s, root) = (&cx, &s, t.path());
            sc.spawn(move || {
                let v = SecretValue::new("x");
                set_secret(root, "dev", &id(), s, &format!("v{n}"), &v, cx).unwrap();
            });
        }
    });
    let plain = read(t.path(), "dev.toml");
    for n in 0..8 {
        assert!(plain.contains(&format!("\"v{n}\"")), "{plain}");
    }
}

// ---- unbound id fails closed ----

fn unbound(e: &SecretsError) -> bool {
    matches!(e, SecretsError::Unbound)
}

/// Store-mode env with existing names, valid store entries, no registry row.
fn unbound_env() -> (tempfile::TempDir, MemoryStore, VaultCtx) {
    let t = tempfile::tempdir().unwrap();
    let s = MemoryStore::new();
    store_env(t.path(), &s);
    (t, s, cx())
}

#[test]
fn unbound_id_with_existing_names_fails_closed() {
    let (t, s, cx) = unbound_env();
    // A get on the store would fail loudly; none must be reached.
    s.fail_nth(1, SecretStoreError::Denied);
    let l = load_environment_with(t.path(), "dev", Some(&id()), &s, &cx).unwrap();
    assert_eq!(
        l.status,
        LoadStatus::Failed {
            reason: FailReason::Unbound
        }
    );
    assert!(l.env.secrets.is_empty());
    s.clear_failures();
    let v = SecretValue::new("x");
    assert!(unbound(
        &delete_secret(t.path(), "dev", &id(), &s, "token", &cx).unwrap_err()
    ));
    assert!(unbound(
        &migrate_to_file(t.path(), "dev", &id(), &s, true, &cx).unwrap_err()
    ));
    assert!(unbound(
        &set_secret(t.path(), "dev", &id(), &s, "token", &v, &cx).unwrap_err()
    ));
    write(t.path(), "dev.secrets.toml", "[vars]\ntoken = \"t\"\n");
    assert!(unbound(
        &import_secrets_file(t.path(), "dev", &id(), &s, &cx).unwrap_err()
    ));
    assert_eq!(read(t.path(), "dev.toml"), STORE_TOML);
    assert_eq!(s.accounts().len(), 2);
    assert_eq!(s.get(&acct("token")).unwrap().unwrap().expose(), "t1");
    assert_eq!(bound_root(&cx.data, &id()).unwrap(), None);
}

#[test]
fn bind_unlocks_unbound_id_and_refuses_other_root() {
    let (t, s, cx) = unbound_env();
    bind(&cx.data, &id(), t.path()).unwrap();
    // Idempotent for the same root.
    bind(&cx.data, &id(), t.path()).unwrap();
    let l = load_environment_with(t.path(), "dev", Some(&id()), &s, &cx).unwrap();
    assert!(matches!(l.status, LoadStatus::Store { .. }));
    assert_eq!(l.env.secrets.len(), 2);
    delete_secret(t.path(), "dev", &id(), &s, "token", &cx).unwrap();
    let other = tempfile::tempdir().unwrap();
    assert!(bound_err(&bind(&cx.data, &id(), other.path()).unwrap_err()));
    migrate_to_file(t.path(), "dev", &id(), &s, true, &cx).unwrap();
}

#[test]
fn fresh_env_set_secret_is_unbound_until_bind() {
    let cx = cx();
    let t = tempfile::tempdir().unwrap();
    let s = MemoryStore::new();
    write(t.path(), "dev.toml", "secrets = \"keychain\"\n");
    let v = SecretValue::new("a");
    let e = set_secret(t.path(), "dev", &id(), &s, "token", &v, &cx).unwrap_err();
    assert!(unbound(&e));
    assert!(s.accounts().is_empty());
    assert_eq!(bound_root(&cx.data, &id()).unwrap(), None);
    bind(&cx.data, &id(), t.path()).unwrap();
    set_secret(t.path(), "dev", &id(), &s, "token", &v, &cx).unwrap();
    assert_eq!(
        bound_root(&cx.data, &id()).unwrap(),
        Some(std::fs::canonicalize(t.path()).unwrap())
    );
}

#[test]
fn sibling_environment_never_auto_binds() {
    let t = tempfile::tempdir().unwrap();
    let s = MemoryStore::new();
    let cx = cx();
    // prod: store mode with names and entries. dev: fresh store mode (empty
    // names), a file-mode env and an import candidate.
    write(t.path(), "prod.toml", STORE_TOML);
    put(&s, "token", "t1");
    write(t.path(), "dev.toml", "secrets = \"keychain\"\n");
    write(t.path(), "filedev.toml", "[vars]\n");
    write(t.path(), "filedev.secrets.toml", "[vars]\nk = \"v\"\n");
    write(t.path(), "imp.toml", "secrets = \"keychain\"\n");
    write(t.path(), "imp.secrets.toml", "[vars]\nk = \"v\"\n");
    let snap = |t: &Path| {
        let mut v: Vec<(String, String)> = std::fs::read_dir(t.join("environments"))
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (
                    e.file_name().to_string_lossy().into_owned(),
                    std::fs::read_to_string(e.path()).unwrap(),
                )
            })
            .collect();
        v.sort();
        v
    };
    let reg = cx.data.root().join("vault-bindings.json");
    let before = (snap(t.path()), s.accounts());
    let v = SecretValue::new("x");
    assert!(unbound(
        &set_secret(t.path(), "dev", &id(), &s, "token", &v, &cx).unwrap_err()
    ));
    assert!(unbound(
        &migrate_to_store(t.path(), "filedev", &id(), StoreKind::Keychain, &s, &cx).unwrap_err()
    ));
    assert!(unbound(
        &import_secrets_file(t.path(), "imp", &id(), &s, &cx).unwrap_err()
    ));
    assert_eq!((snap(t.path()), s.accounts()), before);
    assert!(!reg.exists());
    assert_eq!(bound_root(&cx.data, &id()).unwrap(), None);
    // prod still loads Unbound.
    let l = load_environment_with(t.path(), "prod", Some(&id()), &s, &cx).unwrap();
    assert_eq!(
        l.status,
        LoadStatus::Failed {
            reason: FailReason::Unbound
        }
    );
    // After an explicit bind everything works.
    bind(&cx.data, &id(), t.path()).unwrap();
    let l = load_environment_with(t.path(), "prod", Some(&id()), &s, &cx).unwrap();
    assert!(matches!(l.status, LoadStatus::Store { .. }));
    set_secret(t.path(), "dev", &id(), &s, "token", &v, &cx).unwrap();
    migrate_to_store(t.path(), "filedev", &id(), StoreKind::Keychain, &s, &cx).unwrap();
    import_secrets_file(t.path(), "imp", &id(), &s, &cx).unwrap();
}

#[test]
fn migrate_to_file_delete_loop_has_the_deadline() {
    struct SlowDelete(MemoryStore);
    impl SecretStore for SlowDelete {
        fn probe(&self) -> Result<(), SecretStoreError> {
            Ok(())
        }
        fn get(&self, a: &str) -> Result<Option<SecretValue>, SecretStoreError> {
            self.0.get(a)
        }
        fn set(&self, a: &str, v: &SecretValue) -> Result<(), SecretStoreError> {
            self.0.set(a, v)
        }
        fn delete(&self, a: &str) -> Result<(), SecretStoreError> {
            std::thread::sleep(Duration::from_millis(40));
            self.0.delete(a)
        }
    }
    let t = tempfile::tempdir().unwrap();
    let s = SlowDelete(MemoryStore::new());
    write(t.path(), "dev.toml", &many_names_toml());
    for i in 0..50 {
        s.0.set(
            &secret_account(&id(), "dev", &format!("n{i}")),
            &SecretValue::new("v"),
        )
        .unwrap();
    }
    let cx = bcx(t.path()).with_deadline(Duration::from_millis(150));
    let t0 = Instant::now();
    let w = migrate_to_file(t.path(), "dev", &id(), &s, false, &cx).unwrap();
    assert!(t0.elapsed() < Duration::from_millis(1000));
    assert!(read(t.path(), "dev.toml").contains("secrets = \"file\""));
    assert!(!w.is_empty());
    assert!(w.iter().any(|x| x.error == TIMED_OUT));
    assert!(!s.0.accounts().is_empty());
}

// ---- PR #52 round 2 ----

use wisp_storage::environment::save_environment_locked;

fn write_id(root: &Path, id: &str) {
    std::fs::write(
        root.join("collection.toml"),
        format!("format = 1\nid = \"{id}\"\n"),
    )
    .unwrap();
}

fn marker(root: &Path) -> std::path::PathBuf {
    root.join("environments").join(".wisp-binding")
}

#[test]
fn case_variant_id_maps_to_the_same_row() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let cx = bcx(a.path());
    let upper = CollectionId::parse("ABCDEF0123456789").unwrap();
    assert_eq!(upper, id());
    assert_eq!(
        secret_account(&upper, "dev", "t"),
        secret_account(&id(), "dev", "t")
    );
    write(b.path(), "dev.toml", "secrets = \"keychain\"\n");
    let s = MemoryStore::new();
    let e = set_secret(
        b.path(),
        "dev",
        &upper,
        &s,
        "t",
        &SecretValue::new("x"),
        &cx,
    )
    .unwrap_err();
    assert!(matches!(e, SecretsError::IdBoundElsewhere { .. }), "{e:?}");
    let e = bind(&cx.data, &upper, b.path()).unwrap_err();
    assert!(matches!(e, SecretsError::IdBoundElsewhere { .. }), "{e:?}");
}

#[test]
fn locked_save_races_set_secret_and_keeps_names() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().to_path_buf();
    write(
        &root,
        "dev.toml",
        "secrets = \"keychain\"\n\n[vars]\nhost = \"h\"\n",
    );
    let cx = bcx(&root);
    let s = MemoryStore::new();
    std::thread::scope(|sc| {
        sc.spawn(|| {
            for i in 0..40 {
                let v = SecretValue::new("x");
                set_secret(&root, "dev", &id(), &s, &format!("n{i}"), &v, &cx).unwrap();
            }
        });
        sc.spawn(|| {
            for i in 0..40 {
                let env = Environment {
                    name: "dev".into(),
                    vars: vec![var("host", &format!("h{i}"))],
                    secrets: vec![],
                };
                save_environment_locked(&root, &env, &cx).unwrap();
            }
        });
    });
    let text = read(&root, "dev.toml");
    for i in 0..40 {
        assert!(text.contains(&format!("\"n{i}\"")), "lost n{i}");
    }
    assert!(text.contains("secrets = \"keychain\""));
    assert!(!exists(&root, "dev.secrets.toml"));
}

#[test]
fn locked_save_in_store_mode_refuses_secrets_and_writes_no_file() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", STORE_TOML);
    let cx = bcx(t.path());
    let env = Environment {
        name: "dev".into(),
        vars: vec![],
        secrets: vec![var("a", "b")],
    };
    assert!(save_environment_locked(t.path(), &env, &cx).is_err());
    assert!(!exists(t.path(), "dev.secrets.toml"));
}

#[test]
fn rebind_refuses_a_copy_and_allows_a_move() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    write_id(a.path(), id().as_str());
    write_id(b.path(), id().as_str());
    let cx = bcx(a.path());
    let e = rebind(&cx.data, &id(), b.path()).unwrap_err();
    assert!(matches!(e, SecretsError::LooksLikeCopy { .. }), "{e:?}");
    assert_eq!(
        bound_root(&cx.data, &id()).unwrap(),
        Some(std::fs::canonicalize(a.path()).unwrap())
    );
    // Old root now has a different id: a move.
    write_id(a.path(), "fedcba9876543210");
    rebind(&cx.data, &id(), b.path()).unwrap();
    assert_eq!(
        bound_root(&cx.data, &id()).unwrap(),
        Some(std::fs::canonicalize(b.path()).unwrap())
    );
}

#[test]
fn rebind_succeeds_when_old_root_is_gone() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    write_id(a.path(), id().as_str());
    write_id(b.path(), id().as_str());
    let cx = bcx(a.path());
    drop(a);
    rebind(&cx.data, &id(), b.path()).unwrap();
    let s = MemoryStore::new();
    write(b.path(), "dev.toml", "secrets = \"keychain\"\n");
    set_secret(b.path(), "dev", &id(), &s, "t", &SecretValue::new("x"), &cx).unwrap();
}

#[test]
fn new_id_for_a_copy_starts_without_secrets() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    write_id(a.path(), id().as_str());
    write_id(b.path(), id().as_str());
    let cx = bcx(a.path());
    let new = wisp_storage::collection::assign_new_collection_id(b.path()).unwrap();
    assert_ne!(new, id());
    assert_eq!(
        wisp_storage::collection::read_collection_id(b.path()).unwrap(),
        Some(new.clone())
    );
    let s = MemoryStore::new();
    write(b.path(), "dev.toml", "secrets = \"keychain\"\n");
    let e = set_secret(b.path(), "dev", &new, &s, "t", &SecretValue::new("x"), &cx).unwrap_err();
    assert!(matches!(e, SecretsError::Unbound));
    assert!(s.accounts().is_empty());
}

#[test]
fn bind_writes_private_marker_and_ignores_it() {
    let t = tempfile::tempdir().unwrap();
    let _cx = bcx(t.path());
    let m = marker(t.path());
    let nonce = std::fs::read_to_string(&m).unwrap();
    assert!(!nonce.trim().is_empty());
    assert!(
        read(t.path(), ".gitignore")
            .lines()
            .any(|l| l == ".wisp-binding")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&m).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let reg = std::fs::read_to_string(_cx.data.root().join("vault-bindings.json")).unwrap();
    let entry = nonce.trim().split_once(' ').unwrap();
    assert!(reg.contains(entry.0) && reg.contains(entry.1));
}

fn load_status(root: &Path, cx: &VaultCtx) -> LoadStatus {
    let s = MemoryStore::new();
    load_environment_with(root, "dev", Some(&id()), &s, cx)
        .unwrap()
        .status
}

#[test]
fn missing_marker_is_unbound_until_rebound() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", STORE_TOML);
    let cx = bcx(t.path());
    assert!(matches!(
        load_status(t.path(), &cx),
        LoadStatus::Store { .. }
    ));
    std::fs::remove_file(marker(t.path())).unwrap();
    assert_eq!(
        load_status(t.path(), &cx),
        LoadStatus::Failed {
            reason: FailReason::Unbound
        }
    );
    let s = MemoryStore::new();
    let e = set_secret(t.path(), "dev", &id(), &s, "x", &SecretValue::new("v"), &cx).unwrap_err();
    assert!(matches!(e, SecretsError::Unbound));
    // The bind prompt restores it.
    bind(&cx.data, &id(), t.path()).unwrap();
    assert!(matches!(
        load_status(t.path(), &cx),
        LoadStatus::Store { .. }
    ));
}

#[test]
fn wrong_nonce_is_unbound() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", STORE_TOML);
    let cx = bcx(t.path());
    std::fs::write(marker(t.path()), "not-the-nonce").unwrap();
    assert_eq!(
        load_status(t.path(), &cx),
        LoadStatus::Failed {
            reason: FailReason::Unbound
        }
    );
}

#[cfg(unix)]
#[test]
fn migrate_to_file_refuses_a_symlinked_secrets_file() {
    let t = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let s = MemoryStore::new();
    store_env(t.path(), &s);
    let target = out.path().join("outside.toml");
    std::fs::write(&target, "[vars]\nkeep = \"1\"\n").unwrap();
    std::os::unix::fs::symlink(&target, t.path().join("environments/dev.secrets.toml")).unwrap();
    let cx = bcx(t.path());
    let e = migrate_to_file(t.path(), "dev", &id(), &s, true, &cx).unwrap_err();
    assert!(matches!(e, SecretsError::Storage(_)), "{e:?}");
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "[vars]\nkeep = \"1\"\n"
    );
    assert_eq!(read(t.path(), "dev.toml"), STORE_TOML);
    assert_eq!(s.accounts().len(), 2);
}

#[cfg(unix)]
#[test]
fn migrate_to_file_refuses_a_symlinked_environments_dir() {
    let t = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let s = MemoryStore::new();
    store_env(t.path(), &s);
    let cx = bcx(t.path());
    let moved = out.path().join("environments");
    std::fs::rename(t.path().join("environments"), &moved).unwrap();
    std::os::unix::fs::symlink(&moved, t.path().join("environments")).unwrap();
    assert!(migrate_to_file(t.path(), "dev", &id(), &s, true, &cx).is_err());
    assert!(!moved.join("dev.secrets.toml").exists());
}

#[cfg(unix)]
#[test]
fn import_refuses_a_symlinked_secrets_file() {
    let t = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", STORE_TOML);
    let target = out.path().join("outside.toml");
    std::fs::write(&target, "[vars]\nstolen = \"1\"\n").unwrap();
    std::os::unix::fs::symlink(&target, t.path().join("environments/dev.secrets.toml")).unwrap();
    let cx = bcx(t.path());
    let s = MemoryStore::new();
    assert!(import_secrets_file(t.path(), "dev", &id(), &s, &cx).is_err());
    assert!(s.accounts().is_empty());
}

#[cfg(unix)]
#[test]
fn migrate_to_file_forces_0600_on_an_existing_file() {
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir().unwrap();
    let s = MemoryStore::new();
    store_env(t.path(), &s);
    let sp = t.path().join("environments/dev.secrets.toml");
    std::fs::write(&sp, "[vars]\nold = \"1\"\n").unwrap();
    std::fs::set_permissions(&sp, std::fs::Permissions::from_mode(0o644)).unwrap();
    let cx = bcx(t.path());
    migrate_to_file(t.path(), "dev", &id(), &s, false, &cx).unwrap();
    assert_eq!(
        std::fs::metadata(&sp).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

// ---- PR #52: symlinked binding marker ----

#[cfg(unix)]
fn unbound_load(root: &Path, c: &VaultCtx) -> bool {
    let l = load_environment_with(root, "dev", Some(&id()), &MemoryStore::new(), c).unwrap();
    l.status
        == LoadStatus::Failed {
            reason: FailReason::Unbound,
        }
}

#[cfg(unix)]
#[test]
fn symlinked_environments_dir_is_unbound() {
    let t = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", STORE_TOML);
    let c = bcx(t.path());
    assert!(!unbound_load(t.path(), &c));
    let moved = out.path().join("environments");
    std::fs::rename(t.path().join("environments"), &moved).unwrap();
    std::os::unix::fs::symlink(&moved, t.path().join("environments")).unwrap();
    assert!(unbound_load(t.path(), &c));
}

#[cfg(unix)]
#[test]
fn symlinked_marker_is_unbound() {
    let t = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", STORE_TOML);
    let c = bcx(t.path());
    let marker = t.path().join("environments/.wisp-binding");
    let copy = out.path().join("marker");
    std::fs::copy(&marker, &copy).unwrap();
    std::fs::remove_file(&marker).unwrap();
    std::os::unix::fs::symlink(&copy, &marker).unwrap();
    assert!(unbound_load(t.path(), &c));
}

#[cfg(unix)]
#[test]
fn bind_refuses_a_symlinked_marker() {
    let t = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", STORE_TOML);
    let target = out.path().join("marker");
    std::fs::write(&target, "original").unwrap();
    std::os::unix::fs::symlink(&target, t.path().join("environments/.wisp-binding")).unwrap();
    assert!(bind(&cx().data, &id(), t.path()).is_err());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "original");
}

#[cfg(unix)]
#[test]
fn bind_refuses_a_symlinked_environments_dir() {
    let t = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(out.path(), t.path().join("environments")).unwrap();
    assert!(bind(&cx().data, &id(), t.path()).is_err());
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
}

// ---- PR #52 round 3 ----

#[cfg(unix)]
#[test]
fn symlinked_collection_toml_is_refused_for_id_writes() {
    let t = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let target = out.path().join("collection.toml");
    std::fs::write(&target, "format = 1\nid = \"otherrepo12345678\"\n").unwrap();
    std::os::unix::fs::symlink(&target, t.path().join("collection.toml")).unwrap();
    let before = std::fs::read_to_string(&target).unwrap();
    let e = wisp_storage::collection::assign_new_collection_id(t.path()).unwrap_err();
    assert!(
        matches!(e, wisp_storage::StorageError::Invalid { .. }),
        "{e:?}"
    );
    let e = wisp_storage::collection::ensure_collection_id(t.path()).unwrap_err();
    assert!(
        matches!(e, wisp_storage::StorageError::Invalid { .. }),
        "{e:?}"
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap(), before);
}

#[test]
fn non_regular_collection_toml_is_refused() {
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir(t.path().join("collection.toml")).unwrap();
    assert!(wisp_storage::collection::assign_new_collection_id(t.path()).is_err());
    assert!(wisp_storage::collection::ensure_collection_id(t.path()).is_err());
}

#[cfg(unix)]
#[test]
fn save_environment_locked_refuses_symlinked_secrets_file() {
    let t = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", "[vars]\nhost = \"h\"\n");
    let target = out.path().join("loot.toml");
    std::fs::write(&target, "orig").unwrap();
    std::os::unix::fs::symlink(&target, t.path().join("environments/dev.secrets.toml")).unwrap();
    let c = cx();
    let env = Environment {
        name: "dev".into(),
        vars: vec![var("host", "h")],
        secrets: vec![var("token", "t")],
    };
    assert!(wisp_storage::environment::save_environment_locked(t.path(), &env, &c).is_err());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "orig");
}

#[cfg(unix)]
#[test]
fn save_environment_locked_makes_secrets_file_private() {
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", "[vars]\nhost = \"h\"\n");
    write(t.path(), "dev.secrets.toml", "[vars]\ntoken = \"a\"\n");
    let p = t.path().join("environments/dev.secrets.toml");
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
    let env = Environment {
        name: "dev".into(),
        vars: vec![var("host", "h")],
        secrets: vec![var("token", "b")],
    };
    wisp_storage::environment::save_environment_locked(t.path(), &env, &cx()).unwrap();
    assert_eq!(
        std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn additional_checkout_binds_a_second_root() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let third = tempfile::tempdir().unwrap();
    for r in [&a, &b, &third] {
        write_id(r.path(), id().as_str());
        write(r.path(), "dev.toml", STORE_TOML);
    }
    let c = bcx(a.path());
    // Plain bind of another root still needs the explicit action.
    assert!(bound_err(&bind(&c.data, &id(), b.path()).unwrap_err()));
    share_with_folder(&c.data, &id(), a.path(), b.path()).unwrap();
    assert_eq!(bound_roots(&c.data, &id()).unwrap().len(), 2);
    let s = MemoryStore::new();
    for r in [&a, &b] {
        let l = load_environment_with(r.path(), "dev", Some(&id()), &s, &c).unwrap();
        assert!(
            matches!(l.status, LoadStatus::Store { .. }),
            "{:?}",
            l.status
        );
    }
    let l = load_environment_with(third.path(), "dev", Some(&id()), &s, &c).unwrap();
    assert!(matches!(
        l.status,
        LoadStatus::Failed {
            reason: FailReason::IdBoundElsewhere { .. }
        }
    ));
}

#[test]
fn rebind_of_one_root_leaves_the_other_bound() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let n = tempfile::tempdir().unwrap();
    for r in [&a, &b, &n] {
        write_id(r.path(), id().as_str());
        write(r.path(), "dev.toml", STORE_TOML);
    }
    let c = bcx(a.path());
    share_with_folder(&c.data, &id(), a.path(), b.path()).unwrap();
    // Both live: moving to n is a copy.
    assert!(matches!(
        rebind(&c.data, &id(), n.path()).unwrap_err(),
        SecretsError::LooksLikeCopy { .. }
    ));
    // b was moved away (now another id): n replaces b, a stays.
    write_id(b.path(), "fedcba9876543210");
    rebind(&c.data, &id(), n.path()).unwrap();
    let roots = bound_roots(&c.data, &id()).unwrap();
    assert_eq!(roots.len(), 2);
    assert!(roots.contains(&std::fs::canonicalize(a.path()).unwrap()));
    assert!(roots.contains(&std::fs::canonicalize(n.path()).unwrap()));
    let s = MemoryStore::new();
    for r in [&a, &n] {
        let l = load_environment_with(r.path(), "dev", Some(&id()), &s, &c).unwrap();
        assert!(matches!(l.status, LoadStatus::Store { .. }));
    }
}

#[test]
fn two_data_dirs_bound_to_one_root_both_stay_bound() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", STORE_TOML);
    let c1 = cx();
    let c2 = cx();
    bind(&c1.data, &id(), t.path()).unwrap();
    bind(&c2.data, &id(), t.path()).unwrap();
    assert!(!unbound_status(t.path(), &c1));
    assert!(!unbound_status(t.path(), &c2));
    // Re-binding the first keeps the second.
    std::fs::remove_file(c1.data.root().join("vault-bindings.json")).unwrap();
    bind(&c1.data, &id(), t.path()).unwrap();
    assert!(!unbound_status(t.path(), &c1));
    assert!(!unbound_status(t.path(), &c2));
}

// ---- PR #52 round 4 ----

/// A normal checkout: `.git` is a directory holding the common dir.
fn make_repo(root: &Path) {
    std::fs::create_dir_all(root.join(".git")).unwrap();
}

/// On Windows `canonicalize` yields `\\?\C:\..`; git writes `C:/...`.
fn git_style(p: std::path::PathBuf) -> std::path::PathBuf {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy().into_owned();
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            if !rest.starts_with("UNC") {
                return std::path::PathBuf::from(rest.replace('\\', "/"));
            }
        }
    }
    p
}

/// A linked worktree of `main`: `.git` file -> `<main>/.git/worktrees/<name>`
/// with `commondir` and a back-pointing `gitdir`.
fn make_worktree(main: &Path, wt: &Path, name: &str) {
    // Like git, write canonical (realpath) paths.
    let main = &git_style(std::fs::canonicalize(main).unwrap());
    let wt = &git_style(std::fs::canonicalize(wt).unwrap());
    let gd = main.join(".git/worktrees").join(name);
    std::fs::create_dir_all(&gd).unwrap();
    std::fs::write(gd.join("commondir"), "../..\n").unwrap();
    std::fs::write(
        gd.join("gitdir"),
        format!("{}\n", wt.join(".git").display()),
    )
    .unwrap();
    std::fs::write(wt.join(".git"), format!("gitdir: {}\n", gd.display())).unwrap();
}

fn id_roots(roots: &[&Path]) {
    for r in roots {
        write_id(r, id().as_str());
    }
}

#[test]
fn git_worktree_of_bound_repo_is_a_one_click_checkout() {
    let a = tempfile::tempdir().unwrap();
    let w = tempfile::tempdir().unwrap();
    id_roots(&[a.path(), w.path()]);
    make_repo(a.path());
    make_worktree(a.path(), w.path(), "w1");
    let c = bcx(a.path());
    bind_additional_checkout(&c.data, &id(), w.path()).unwrap();
    assert_eq!(bound_roots(&c.data, &id()).unwrap().len(), 2);
}

#[test]
fn separate_repo_with_copied_id_is_unverified() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    id_roots(&[a.path(), b.path()]);
    make_repo(a.path());
    make_repo(b.path());
    let c = bcx(a.path());
    assert!(matches!(
        bind_additional_checkout(&c.data, &id(), b.path()).unwrap_err(),
        SecretsError::UnverifiedCheckout { .. }
    ));
    assert_eq!(bound_roots(&c.data, &id()).unwrap().len(), 1);
}

#[test]
fn non_git_folder_is_unverified() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    id_roots(&[a.path(), b.path()]);
    make_repo(a.path());
    let c = bcx(a.path());
    assert!(matches!(
        bind_additional_checkout(&c.data, &id(), b.path()).unwrap_err(),
        SecretsError::UnverifiedCheckout { .. }
    ));
}

#[test]
fn forged_git_file_without_back_pointer_is_unverified() {
    let a = tempfile::tempdir().unwrap();
    let w = tempfile::tempdir().unwrap();
    id_roots(&[a.path(), w.path()]);
    make_repo(a.path());
    let gd = a.path().join(".git/worktrees/other");
    std::fs::create_dir_all(&gd).unwrap();
    std::fs::write(gd.join("commondir"), "../..\n").unwrap();
    std::fs::write(w.path().join(".git"), format!("gitdir: {}\n", gd.display())).unwrap();
    let c = bcx(a.path());
    assert!(matches!(
        bind_additional_checkout(&c.data, &id(), w.path()).unwrap_err(),
        SecretsError::UnverifiedCheckout { .. }
    ));
}

fn unverified(c: &VaultCtx, root: &Path) {
    assert!(matches!(
        bind_additional_checkout(&c.data, &id(), root).unwrap_err(),
        SecretsError::UnverifiedCheckout { .. }
    ));
    assert_eq!(bound_roots(&c.data, &id()).unwrap().len(), 1);
}

#[test]
fn attack_a_gitdir_naming_the_bound_repo_without_commondir_is_unverified() {
    let a = tempfile::tempdir().unwrap();
    let e = tempfile::tempdir().unwrap();
    id_roots(&[a.path(), e.path()]);
    make_repo(a.path());
    std::fs::write(
        e.path().join(".git"),
        format!("gitdir: {}\n", a.path().join(".git").display()),
    )
    .unwrap();
    let c = bcx(a.path());
    unverified(&c, e.path());
    // Relative form.
    let rel = format!(
        "../{}/.git",
        a.path().file_name().unwrap().to_string_lossy()
    );
    std::fs::write(e.path().join(".git"), format!("gitdir: {rel}\n")).unwrap();
    unverified(&c, e.path());
}

#[test]
fn attack_b_attacker_written_gitdir_and_back_pointer_is_unverified() {
    let a = tempfile::tempdir().unwrap();
    let e = tempfile::tempdir().unwrap();
    id_roots(&[a.path(), e.path()]);
    make_repo(a.path());
    let fake = e.path().join("fake");
    std::fs::create_dir_all(&fake).unwrap();
    std::fs::write(
        fake.join("commondir"),
        format!("{}\n", a.path().join(".git").display()),
    )
    .unwrap();
    std::fs::write(
        fake.join("gitdir"),
        format!("{}\n", e.path().join(".git").display()),
    )
    .unwrap();
    std::fs::write(e.path().join(".git"), "gitdir: fake\n").unwrap();
    let c = bcx(a.path());
    unverified(&c, e.path());
}

#[test]
fn victim_worktree_entry_with_back_pointer_to_another_folder_is_unverified() {
    let a = tempfile::tempdir().unwrap();
    let victim = tempfile::tempdir().unwrap();
    let e = tempfile::tempdir().unwrap();
    id_roots(&[a.path(), victim.path(), e.path()]);
    make_repo(a.path());
    make_worktree(a.path(), victim.path(), "w1");
    std::fs::write(
        e.path().join(".git"),
        format!("gitdir: {}\n", a.path().join(".git/worktrees/w1").display()),
    )
    .unwrap();
    let c = bcx(a.path());
    unverified(&c, e.path());
}

#[test]
fn unc_style_gitdir_is_refused_without_io() {
    let a = tempfile::tempdir().unwrap();
    let e = tempfile::tempdir().unwrap();
    id_roots(&[a.path(), e.path()]);
    make_repo(a.path());
    let c = bcx(a.path());
    std::fs::write(e.path().join(".git"), "gitdir: //server/share/x\n").unwrap();
    unverified(&c, e.path());
    std::fs::write(e.path().join(".git"), "gitdir: \\\\server\\share\\x\n").unwrap();
    unverified(&c, e.path());
}

#[test]
fn lexically_plausible_unc_gitdir_is_refused() {
    let a = tempfile::tempdir().unwrap();
    let e = tempfile::tempdir().unwrap();
    id_roots(&[a.path(), e.path()]);
    make_repo(a.path());
    let c = bcx(a.path());
    let name = a.path().join(".git").file_name().unwrap().to_owned();
    let text = format!(
        "gitdir: //server/share/{}/worktrees/w\n",
        name.to_string_lossy()
    );
    std::fs::write(e.path().join(".git"), text).unwrap();
    unverified(&c, e.path());
}

#[cfg(unix)]
#[test]
fn symlink_inside_a_plausible_gitdir_is_never_followed() {
    let a = tempfile::tempdir().unwrap();
    let e = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    id_roots(&[a.path(), e.path()]);
    make_repo(a.path());
    let c = bcx(a.path());
    // Outside the tempdir, with a commondir that is a directory: reading it
    // would be an error, so Unverified proves it was never opened.
    std::fs::create_dir_all(out.path().join(".git/worktrees/w/commondir")).unwrap();
    std::os::unix::fs::symlink(out.path(), e.path().join("sub")).unwrap();
    std::fs::write(e.path().join(".git"), "gitdir: sub/.git/worktrees/w\n").unwrap();
    unverified(&c, e.path());
}

/// Legitimate worktree layout of bound repo A, reached through a symlink:
/// `E/sub -> A`, `E/.git` = `gitdir: sub/.git/worktrees/w`, back-pointer names
/// `E/.git`. The old code canonicalised the joined path, so it resolved to
/// `A/.git/worktrees/w` and verified; the exact lexical match must refuse it
/// (`E/sub/.git` is not a trusted common dir). Not observable here: that the
/// match does no I/O before deciding (e.g. a macOS /net automount).
#[cfg(unix)]
#[test]
fn symlink_reaching_a_real_worktree_layout_is_unverified() {
    let a = tempfile::tempdir().unwrap();
    let e = tempfile::tempdir().unwrap();
    id_roots(&[a.path(), e.path()]);
    make_repo(a.path());
    let c = bcx(a.path());
    let a_canon = std::fs::canonicalize(a.path()).unwrap();
    let e_canon = std::fs::canonicalize(e.path()).unwrap();
    let gd = a_canon.join(".git/worktrees/w");
    std::fs::create_dir_all(&gd).unwrap();
    std::fs::write(gd.join("commondir"), "../..\n").unwrap();
    std::fs::write(
        gd.join("gitdir"),
        format!("{}\n", e_canon.join(".git").display()),
    )
    .unwrap();
    std::os::unix::fs::symlink(&a_canon, e.path().join("sub")).unwrap();
    std::fs::write(e.path().join(".git"), "gitdir: sub/.git/worktrees/w\n").unwrap();
    unverified(&c, e.path());
}

#[test]
fn gitdir_outside_a_bound_common_dir_is_not_read() {
    let a = tempfile::tempdir().unwrap();
    let e = tempfile::tempdir().unwrap();
    id_roots(&[a.path(), e.path()]);
    make_repo(a.path());
    let c = bcx(a.path());
    // A forged worktree whose commondir is unreadable (a directory): reading it
    // would be an error, so Unverified proves it was never opened.
    let fake = e.path().join("elsewhere/worktrees/w");
    std::fs::create_dir_all(fake.join("commondir")).unwrap();
    std::fs::write(
        e.path().join(".git"),
        format!("gitdir: {}\n", fake.display()),
    )
    .unwrap();
    unverified(&c, e.path());
}

#[cfg(unix)]
#[test]
fn symlinked_dot_git_is_refused() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    id_roots(&[a.path(), b.path()]);
    make_repo(a.path());
    std::os::unix::fs::symlink(a.path().join(".git"), b.path().join(".git")).unwrap();
    let c = bcx(a.path());
    assert!(bind_additional_checkout(&c.data, &id(), b.path()).is_err());
    assert_eq!(bound_roots(&c.data, &id()).unwrap().len(), 1);
}

/// Bind `w` (a verified worktree of `a`) so it is a bound root, then return.
fn bound_worktree(c: &VaultCtx, a: &Path, w: &Path) {
    make_worktree(a, w, "w1");
    bind_additional_checkout(&c.data, &id(), w).unwrap();
}

#[cfg(unix)]
#[test]
fn bound_worktree_with_symlinked_gitdir_is_never_read() {
    let a = tempfile::tempdir().unwrap();
    let w = tempfile::tempdir().unwrap();
    let n = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    id_roots(&[a.path(), w.path(), n.path()]);
    make_repo(a.path());
    let c = bcx(a.path());
    bound_worktree(&c, a.path(), w.path());
    // The bound worktree's .git now routes through a symlink to an outside dir
    // whose commondir is a directory (unreadable if opened).
    std::fs::create_dir_all(out.path().join(".git/worktrees/w/commondir")).unwrap();
    std::os::unix::fs::symlink(out.path(), w.path().join("sub")).unwrap();
    std::fs::write(w.path().join(".git"), "gitdir: sub/.git/worktrees/w\n").unwrap();
    let out_canon = std::fs::canonicalize(out.path()).unwrap();
    std::fs::write(
        n.path().join(".git"),
        format!("gitdir: {}\n", out_canon.join(".git/worktrees/w").display()),
    )
    .unwrap();
    assert!(matches!(
        bind_additional_checkout(&c.data, &id(), n.path()).unwrap_err(),
        SecretsError::UnverifiedCheckout { .. }
    ));
}

#[test]
fn new_worktree_of_a_bound_main_checkout_still_verifies_with_a_bound_worktree() {
    let a = tempfile::tempdir().unwrap();
    let w = tempfile::tempdir().unwrap();
    let n = tempfile::tempdir().unwrap();
    id_roots(&[a.path(), w.path(), n.path()]);
    make_repo(a.path());
    let c = bcx(a.path());
    bound_worktree(&c, a.path(), w.path());
    make_worktree(a.path(), n.path(), "w2");
    bind_additional_checkout(&c.data, &id(), n.path()).unwrap();
    assert_eq!(bound_roots(&c.data, &id()).unwrap().len(), 3);
}

#[cfg(unix)]
#[test]
fn bound_root_with_symlinked_dot_git_is_not_an_anchor() {
    let a = tempfile::tempdir().unwrap();
    let real = tempfile::tempdir().unwrap();
    let n = tempfile::tempdir().unwrap();
    id_roots(&[a.path(), n.path()]);
    make_repo(real.path());
    make_repo(a.path());
    let c = bcx(a.path());
    // Bound root `a` swaps its .git for a symlink to a real repo that `n`
    // is a worktree of.
    std::fs::remove_dir_all(a.path().join(".git")).unwrap();
    std::os::unix::fs::symlink(real.path().join(".git"), a.path().join(".git")).unwrap();
    make_worktree(real.path(), n.path(), "w1");
    unverified(&c, n.path());
}

#[test]
fn share_with_folder_from_a_bound_root_works() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    id_roots(&[a.path(), b.path()]);
    let c = bcx(a.path());
    share_with_folder(&c.data, &id(), a.path(), b.path()).unwrap();
    assert_eq!(bound_roots(&c.data, &id()).unwrap().len(), 2);
}

#[test]
fn share_with_folder_from_unbound_or_stale_root_fails() {
    let a = tempfile::tempdir().unwrap();
    let u = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    id_roots(&[a.path(), u.path(), b.path()]);
    let c = bcx(a.path());
    assert!(share_with_folder(&c.data, &id(), u.path(), b.path()).is_err());
    // Marker mismatch: the bound root's marker is gone.
    std::fs::remove_file(marker(a.path())).unwrap();
    assert!(share_with_folder(&c.data, &id(), a.path(), b.path()).is_err());
    assert_eq!(bound_roots(&c.data, &id()).unwrap().len(), 1);
}

#[test]
fn share_with_folder_needs_the_same_id() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    id_roots(&[a.path()]);
    write_id(b.path(), "fedcba9876543210");
    let c = bcx(a.path());
    assert!(share_with_folder(&c.data, &id(), a.path(), b.path()).is_err());
}

fn file_mode_env(vars: Vec<Var>, secrets: Vec<Var>) -> Environment {
    Environment {
        name: "dev".into(),
        vars,
        secrets,
    }
}

#[cfg(unix)]
#[test]
fn symlinked_environments_dir_vars_only_save_names_the_directory() {
    let t = tempfile::tempdir().unwrap();
    let real = tempfile::tempdir().unwrap();
    std::fs::write(real.path().join("dev.toml"), "secrets = \"file\"\n").unwrap();
    std::os::unix::fs::symlink(real.path(), t.path().join("environments")).unwrap();
    let e = save_environment_locked(t.path(), &file_mode_env(vec![var("h", "1")], vec![]), &cx())
        .unwrap_err();
    let m = e.to_string();
    assert!(m.contains("environments directory"), "{m}");
    assert!(!m.contains("secrets"), "{m}");
}

#[cfg(unix)]
#[test]
fn vars_only_save_ignores_a_dangling_secrets_link() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "dev.toml", "secrets = \"file\"\n");
    std::os::unix::fs::symlink(
        t.path().join("nowhere"),
        t.path().join("environments/dev.secrets.toml"),
    )
    .unwrap();
    save_environment_locked(t.path(), &file_mode_env(vec![var("h", "1")], vec![]), &cx()).unwrap();
    // Writing a secret through that link is still refused.
    assert!(
        save_environment_locked(
            t.path(),
            &file_mode_env(vec![var("h", "1")], vec![var("k", "v")]),
            &cx()
        )
        .is_err()
    );
}

fn unbound_status(root: &Path, c: &VaultCtx) -> bool {
    load_status(root, c)
        == LoadStatus::Failed {
            reason: FailReason::Unbound,
        }
}

// ---- symlink confinement (task 120) ----

#[cfg(unix)]
#[test]
fn symlinked_gitignore_refuses_save_and_leaves_target() {
    let t = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let target = out.path().join("outside");
    std::fs::write(&target, "ORIGINAL\n").unwrap();
    let dir = t.path().join("environments");
    std::fs::create_dir_all(&dir).unwrap();
    std::os::unix::fs::symlink(&target, dir.join(".gitignore")).unwrap();
    let env = Environment {
        name: "dev".into(),
        vars: vec![],
        secrets: vec![Var {
            name: "token".into(),
            value: "SECRETVAL".into(),
        }],
    };
    assert!(save_environment(t.path(), &env).is_err());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "ORIGINAL\n");
}

#[cfg(unix)]
#[test]
fn symlinked_secrets_file_fails_load_and_migrate() {
    let t = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let target = out.path().join("outside.toml");
    std::fs::write(&target, "[vars]\ntoken = \"SECRETVAL\"\n").unwrap();
    write(t.path(), "dev.toml", "[vars]\nhost = \"h\"\n");
    std::os::unix::fs::symlink(&target, t.path().join("environments/dev.secrets.toml")).unwrap();
    assert!(load_environment(t.path(), "dev").is_err());
    let c = bcx(t.path());
    assert!(load_environment_with(t.path(), "dev", Some(&id()), &NoStores, &c).is_err());
    let s = MemoryStore::new();
    let r = migrate_to_store(t.path(), "dev", &id(), StoreKind::Keychain, &s, &c);
    assert!(r.is_err());
    assert!(s.accounts().is_empty());
}

#[cfg(unix)]
#[test]
fn symlinked_collection_toml_yields_no_id() {
    let t = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let target = out.path().join("other.toml");
    std::fs::write(&target, "format = 1\nid = \"abcdef0123456789\"\n").unwrap();
    std::os::unix::fs::symlink(&target, t.path().join("collection.toml")).unwrap();
    assert!(wisp_storage::collection::read_collection_id(t.path()).is_err());
    let m = tempfile::tempdir().unwrap();
    assert!(
        wisp_storage::collection::read_collection_id(m.path())
            .unwrap()
            .is_none()
    );
}

// ---- PR #63: symlinked plain env file ----

#[cfg(unix)]
#[test]
fn symlinked_plain_env_file_is_refused_not_loaded_as_vars() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("environments");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("prod.secrets.toml"), "[vars]\nk = \"SECRETVAL\"\n").unwrap();
    std::os::unix::fs::symlink(dir.join("prod.secrets.toml"), dir.join("dev.toml")).unwrap();
    let e = load_environment_with(t.path(), "dev", None, &NoStores, &cx());
    assert!(
        matches!(e, Err(wisp_storage::StorageError::Invalid { .. })),
        "{e:?}"
    );
    let e = load_environment(t.path(), "dev");
    assert!(
        matches!(e, Err(wisp_storage::StorageError::Invalid { .. })),
        "{e:?}"
    );
}
