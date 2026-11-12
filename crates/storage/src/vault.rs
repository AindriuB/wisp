//! Optional per-environment secret stores.
//!
//! An environment's `<env>.toml` may carry `secrets = "<kind>"` and
//! `secret_names = [...]`. Secrets for such an environment live in a
//! [`SecretStore`] instead of `<env>.secrets.toml`. This module has the trait,
//! an in-memory store, the fail-closed loader and explicit migration. It holds
//! no backend; the app constructs the stores.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::sync::Mutex;

use toml_edit::DocumentMut;
use wisp_core::request::{Environment, Var};

use crate::collection::CollectionId;
use crate::environment::{check_name, ensure_gitignore, plain_path, render, secrets_path};
use crate::{StorageError, codec, fs};

pub use crate::error::{SecretStoreError, UnavailableReason};

/// A secret string. Debug prints `<redacted>`; there is no Display.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretValue(String);

impl SecretValue {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Which store holds an environment's secrets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StoreKind {
    File,
    Keychain,
}

impl StoreKind {
    pub fn parse(s: &str) -> Option<StoreKind> {
        match s {
            "file" => Some(StoreKind::File),
            "keychain" => Some(StoreKind::Keychain),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            StoreKind::File => "file",
            StoreKind::Keychain => "keychain",
        }
    }
}

/// A blocking secret store, addressed by an account string.
///
/// Any method may block for seconds or go over IPC (D-Bus, a socket, a
/// browser-protocol pipe); callers run them off the UI thread. The account is
/// a store-neutral address; a store maps it to its own scheme without
/// changing it.
pub trait SecretStore: Send + Sync {
    /// Check that the store can be used at all.
    fn probe(&self) -> Result<(), SecretStoreError>;
    /// `Ok(None)` when the account has no entry.
    fn get(&self, account: &str) -> Result<Option<SecretValue>, SecretStoreError>;
    fn set(&self, account: &str, value: &SecretValue) -> Result<(), SecretStoreError>;
    /// Deleting a missing entry is `Ok`.
    fn delete(&self, account: &str) -> Result<(), SecretStoreError>;
}

/// Finds the store for a kind. The app constructs stores; storage never does.
pub trait SecretStores {
    fn get(&self, kind: StoreKind) -> Option<&dyn SecretStore>;
}

/// A [`SecretStores`] with no stores.
pub struct NoStores;

impl SecretStores for NoStores {
    fn get(&self, _kind: StoreKind) -> Option<&dyn SecretStore> {
        None
    }
}

/// One in-memory store answers every kind.
impl SecretStores for MemoryStore {
    fn get(&self, _kind: StoreKind) -> Option<&dyn SecretStore> {
        Some(self)
    }
}

#[derive(Default)]
struct MemoryInner {
    map: BTreeMap<String, SecretValue>,
    probe_err: Option<SecretStoreError>,
    fail_at: Option<(usize, SecretStoreError)>,
}

/// In-memory store for tests. Can fail its probe, or a chosen later call.
#[derive(Default)]
pub struct MemoryStore {
    inner: Mutex<MemoryInner>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, MemoryInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Make every `probe` return `err` (`None` to clear).
    pub fn fail_probe(&self, err: Option<SecretStoreError>) {
        self.lock().probe_err = err;
    }

    /// Fail the next `get`/`set`/`delete` call with `err`.
    pub fn fail_next(&self, err: SecretStoreError) {
        self.fail_nth(0, err);
    }

    /// Fail the call after `n` more successful `get`/`set`/`delete` calls.
    pub fn fail_nth(&self, n: usize, err: SecretStoreError) {
        self.lock().fail_at = Some((n, err));
    }

    pub fn clear_failures(&self) {
        let mut g = self.lock();
        g.probe_err = None;
        g.fail_at = None;
    }

    /// The accounts held, sorted.
    pub fn accounts(&self) -> Vec<String> {
        self.lock().map.keys().cloned().collect()
    }

    fn tick(g: &mut MemoryInner) -> Result<(), SecretStoreError> {
        if let Some((n, _)) = &mut g.fail_at {
            if *n == 0 {
                if let Some((_, e)) = g.fail_at.take() {
                    return Err(e);
                }
            } else {
                *n -= 1;
            }
        }
        Ok(())
    }
}

impl fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemoryStore")
            .field("entries", &self.lock().map.len())
            .finish()
    }
}

impl SecretStore for MemoryStore {
    fn probe(&self) -> Result<(), SecretStoreError> {
        match self.lock().probe_err.clone() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    fn get(&self, account: &str) -> Result<Option<SecretValue>, SecretStoreError> {
        let mut g = self.lock();
        Self::tick(&mut g)?;
        Ok(g.map.get(account).cloned())
    }

    fn set(&self, account: &str, value: &SecretValue) -> Result<(), SecretStoreError> {
        let mut g = self.lock();
        Self::tick(&mut g)?;
        g.map.insert(account.to_string(), value.clone());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), SecretStoreError> {
        let mut g = self.lock();
        Self::tick(&mut g)?;
        g.map.remove(account);
        Ok(())
    }
}

fn esc(s: &str) -> String {
    s.replace('%', "%25").replace('/', "%2F")
}

/// Account for a secret var: `<id>/<env>/<var>`, components escaped.
pub fn secret_account(id: &CollectionId, env: &str, var: &str) -> String {
    format!("{}/{}/{}", esc(id.as_str()), esc(env), esc(var))
}

/// Account for an OAuth2 refresh token.
pub fn refresh_token_account(
    id: &CollectionId,
    env: &str,
    grant: &str,
    token_url: &str,
    client_id: &str,
    scope: &str,
) -> String {
    format!(
        "{}/{}/oauth2/{}/{}/{}/{}",
        esc(id.as_str()),
        esc(env),
        esc(grant),
        esc(token_url),
        esc(client_id),
        esc(scope)
    )
}

/// Account for per-store state (for example a pairing key), kept in another
/// store. The `.store/` prefix cannot start a [`CollectionId`]. Seam only:
/// no pairing code lives here.
pub fn store_state_account(kind: StoreKind, name: &str) -> String {
    format!(".store/{}/{}", kind.as_str(), esc(name))
}

/// How an environment keeps its secrets, read from `<env>.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Mode {
    File,
    Store(StoreKind),
    Unknown(String),
}

pub(crate) fn read_mode(doc: &DocumentMut, path: &Path) -> Result<Mode, StorageError> {
    let Some(item) = doc.get("secrets") else {
        return Ok(Mode::File);
    };
    let s = item.as_str().ok_or_else(|| StorageError::Invalid {
        path: path.to_path_buf(),
        key: "secrets".into(),
        message: "expected a string".into(),
    })?;
    Ok(match StoreKind::parse(s) {
        Some(StoreKind::File) => Mode::File,
        Some(k) => Mode::Store(k),
        None => Mode::Unknown(s.to_string()),
    })
}

fn read_names(doc: &DocumentMut, path: &Path) -> Result<Vec<String>, StorageError> {
    let Some(item) = doc.get("secret_names") else {
        return Ok(Vec::new());
    };
    let bad = || StorageError::Invalid {
        path: path.to_path_buf(),
        key: "secret_names".into(),
        message: "expected an array of strings".into(),
    };
    let arr = item.as_array().ok_or_else(bad)?;
    arr.iter()
        .map(|v| v.as_str().map(str::to_string).ok_or_else(bad))
        .collect()
}

fn set_names(doc: &mut DocumentMut, names: &[String]) {
    let mut arr = toml_edit::Array::new();
    for n in names {
        arr.push(n.as_str());
    }
    doc.insert("secret_names", toml_edit::value(arr));
}

/// Why a store-mode load produced no secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailReason {
    UnknownStoreKind(String),
    NoCollectionId,
    Store(SecretStoreError),
}

/// Where a loaded environment's secrets came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadStatus {
    File,
    FellBackToFile { reason: UnavailableReason },
    Store { kind: StoreKind, file_ignored: bool },
    Failed { reason: FailReason },
}

/// An environment plus how its secrets were resolved. Debug redacts values
/// through [`Environment`].
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedEnvironment {
    pub env: Environment,
    pub status: LoadStatus,
    /// Names in `secret_names` that have no value.
    pub unresolved: Vec<String>,
}

/// Only these reasons mean the store is absent, so the file may be used.
fn is_absent(reason: UnavailableReason) -> bool {
    matches!(
        reason,
        UnavailableReason::Portable
            | UnavailableReason::NoService
            | UnavailableReason::NotConfigured
    )
}

/// Load an environment, following the fail-closed table. Only TOML and IO
/// problems are `Err`; a store failure is a `Failed` status.
pub fn load_environment_with(
    root: &Path,
    name: &str,
    id: Option<&CollectionId>,
    stores: &dyn SecretStores,
) -> Result<LoadedEnvironment, StorageError> {
    check_name(root, name)?;
    let pp = plain_path(root, name);
    let doc = codec::parse(&pp, &fs::read_to_string(&pp)?)?;
    let mode = read_mode(&doc, &pp)?;
    let names = read_names(&doc, &pp)?;
    let vars = codec::read_vars(doc.as_table(), "vars", &pp)?;
    let sp = secrets_path(root, name);

    let finish = |secrets: Vec<Var>, status: LoadStatus| {
        let unresolved = names
            .iter()
            .filter(|n| !secrets.iter().any(|s| &s.name == *n))
            .cloned()
            .collect();
        tracing::debug!(
            environment = name,
            secrets = secrets.len(),
            ?status,
            "loaded environment"
        );
        LoadedEnvironment {
            env: Environment {
                name: name.to_string(),
                vars: vars.clone(),
                secrets,
            },
            status,
            unresolved,
        }
    };
    let failed = |reason: FailReason| finish(Vec::new(), LoadStatus::Failed { reason });
    let file_secrets = || -> Result<Vec<Var>, StorageError> {
        if sp.exists() {
            crate::environment::load_vars(&sp)
        } else {
            Ok(Vec::new())
        }
    };

    let kind = match mode {
        Mode::File => return Ok(finish(file_secrets()?, LoadStatus::File)),
        Mode::Unknown(s) => return Ok(failed(FailReason::UnknownStoreKind(s))),
        Mode::Store(k) => k,
    };
    let probe = match stores.get(kind) {
        Some(s) => s.probe().map(|()| s),
        None => Err(SecretStoreError::Unavailable {
            reason: UnavailableReason::NotConfigured,
        }),
    };
    let store = match probe {
        Ok(s) => s,
        Err(SecretStoreError::Unavailable { reason }) if is_absent(reason) => {
            return Ok(finish(
                file_secrets()?,
                LoadStatus::FellBackToFile { reason },
            ));
        }
        Err(e) => return Ok(failed(FailReason::Store(e))),
    };
    let Some(id) = id else {
        return Ok(failed(FailReason::NoCollectionId));
    };
    let mut secrets = Vec::new();
    for n in &names {
        match store.get(&secret_account(id, name, n)) {
            Ok(Some(v)) => secrets.push(Var {
                name: n.clone(),
                value: v.expose().to_string(),
            }),
            Ok(None) => {}
            Err(e) => return Ok(failed(FailReason::Store(e))),
        }
    }
    Ok(finish(
        secrets,
        LoadStatus::Store {
            kind,
            file_ignored: sp.exists(),
        },
    ))
}

/// A failure of a secrets operation.
#[derive(Debug, thiserror::Error)]
pub enum SecretsError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Store(#[from] SecretStoreError),
}

/// A store entry that could not be deleted after `migrate_to_file`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteWarning {
    pub name: String,
    pub error: SecretStoreError,
}

fn invalid(path: &Path, key: &str, message: &str) -> SecretsError {
    SecretsError::Storage(StorageError::Invalid {
        path: path.to_path_buf(),
        key: key.into(),
        message: message.into(),
    })
}

fn read_plain(root: &Path, name: &str) -> Result<(std::path::PathBuf, DocumentMut), StorageError> {
    check_name(root, name)?;
    let pp = plain_path(root, name);
    let doc = codec::parse(&pp, &fs::read_to_string(&pp)?)?;
    Ok((pp, doc))
}

/// Move an environment's file secrets into `store`. Ordered so that a
/// failure before the file is deleted leaves the file and mode unchanged and
/// removes the entries this call created.
pub fn migrate_to_store(
    root: &Path,
    name: &str,
    id: &CollectionId,
    kind: StoreKind,
    store: &dyn SecretStore,
) -> Result<(), SecretsError> {
    let (pp, mut doc) = read_plain(root, name)?;
    if kind == StoreKind::File || read_mode(&doc, &pp)? != Mode::File {
        return Err(invalid(&pp, "secrets", "environment is not in file mode"));
    }
    let sp = secrets_path(root, name);
    let secrets = if sp.exists() {
        crate::environment::load_vars(&sp)?
    } else {
        Vec::new()
    };
    store.probe()?;

    let mut created: Vec<String> = Vec::new();
    let rollback = |created: &[String]| {
        for a in created {
            if store.delete(a).is_err() {
                tracing::warn!("could not remove a migrated secret store entry during rollback");
            }
        }
    };
    let mut copy = || -> Result<(), SecretStoreError> {
        for v in &secrets {
            let account = secret_account(id, name, &v.name);
            let existed = store.get(&account)?.is_some();
            let value = SecretValue::new(v.value.as_str());
            if !existed {
                created.push(account.clone());
            }
            store.set(&account, &value)?;
            if store.get(&account)?.as_ref() != Some(&value) {
                return Err(SecretStoreError::Other { code: None });
            }
        }
        Ok(())
    };
    if let Err(e) = copy() {
        rollback(&created);
        return Err(e.into());
    }

    doc["secrets"] = toml_edit::value(kind.as_str());
    let names: Vec<String> = secrets.iter().map(|v| v.name.clone()).collect();
    set_names(&mut doc, &names);
    if let Err(e) = fs::atomic_write(&pp, doc.to_string().as_bytes()) {
        rollback(&created);
        return Err(e.into());
    }
    if sp.exists() {
        std::fs::remove_file(&sp).map_err(|e| StorageError::Io {
            path: sp.clone(),
            source: e,
        })?;
    }
    tracing::debug!(
        environment = name,
        count = names.len(),
        "migrated secrets to store"
    );
    Ok(())
}

/// Move an environment's store secrets back into `<env>.secrets.toml`. Any
/// read error aborts with nothing changed. A failure deleting store entries
/// afterwards is returned as warnings; the file stays.
pub fn migrate_to_file(
    root: &Path,
    name: &str,
    id: &CollectionId,
    store: &dyn SecretStore,
) -> Result<Vec<DeleteWarning>, SecretsError> {
    let (pp, mut doc) = read_plain(root, name)?;
    if !matches!(read_mode(&doc, &pp)?, Mode::Store(_)) {
        return Err(invalid(&pp, "secrets", "environment is not in store mode"));
    }
    let names = read_names(&doc, &pp)?;
    store.probe()?;
    let mut secrets = Vec::new();
    for n in &names {
        if let Some(v) = store.get(&secret_account(id, name, n))? {
            secrets.push(Var {
                name: n.clone(),
                value: v.expose().to_string(),
            });
        }
    }

    let sp = secrets_path(root, name);
    let had_file = sp.exists();
    let text = render(&sp, &secrets)?;
    let dir = root.join("environments");
    ensure_gitignore(&dir)?;
    fs::atomic_write_private(&sp, text.as_bytes())?;

    doc["secrets"] = toml_edit::value(StoreKind::File.as_str());
    doc.remove("secret_names");
    if let Err(e) = fs::atomic_write(&pp, doc.to_string().as_bytes()) {
        if !had_file {
            let _ = std::fs::remove_file(&sp);
        }
        return Err(e.into());
    }

    let mut warnings = Vec::new();
    for n in &names {
        if let Err(error) = store.delete(&secret_account(id, name, n)) {
            warnings.push(DeleteWarning {
                name: n.clone(),
                error,
            });
        }
    }
    tracing::debug!(
        environment = name,
        count = secrets.len(),
        "migrated secrets to file"
    );
    Ok(warnings)
}

fn store_mode_doc(
    root: &Path,
    name: &str,
) -> Result<(std::path::PathBuf, DocumentMut, Vec<String>), SecretsError> {
    let (pp, doc) = read_plain(root, name)?;
    if !matches!(read_mode(&doc, &pp)?, Mode::Store(_)) {
        return Err(invalid(&pp, "secrets", "environment is not in store mode"));
    }
    let names = read_names(&doc, &pp)?;
    Ok((pp, doc, names))
}

/// Set one secret in a store-mode environment: the store first, then
/// `secret_names`. Never writes a secrets file.
pub fn set_secret(
    root: &Path,
    name: &str,
    id: &CollectionId,
    store: &dyn SecretStore,
    var: &str,
    value: &SecretValue,
) -> Result<(), SecretsError> {
    let (pp, mut doc, mut names) = store_mode_doc(root, name)?;
    store.set(&secret_account(id, name, var), value)?;
    if !names.iter().any(|n| n == var) {
        names.push(var.to_string());
        set_names(&mut doc, &names);
        fs::atomic_write(&pp, doc.to_string().as_bytes())?;
    }
    Ok(())
}

/// Delete one secret from a store-mode environment: the store first, then
/// `secret_names`. Never writes a secrets file.
pub fn delete_secret(
    root: &Path,
    name: &str,
    id: &CollectionId,
    store: &dyn SecretStore,
    var: &str,
) -> Result<(), SecretsError> {
    let (pp, mut doc, mut names) = store_mode_doc(root, name)?;
    store.delete(&secret_account(id, name, var))?;
    if names.iter().any(|n| n == var) {
        names.retain(|n| n != var);
        set_names(&mut doc, &names);
        fs::atomic_write(&pp, doc.to_string().as_bytes())?;
    }
    Ok(())
}
