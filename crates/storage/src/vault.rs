//! Optional per-environment secret stores.
//!
//! An environment's `<env>.toml` may carry `secrets = "<kind>"` and
//! `secret_names = [...]`. Secrets for such an environment live in a
//! [`SecretStore`] instead of `<env>.secrets.toml`. This module has the trait,
//! an in-memory store, the fail-closed loader and explicit migration. It holds
//! no backend; the app constructs the stores.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use toml_edit::DocumentMut;
use wisp_core::request::{Environment, Var};

use crate::collection::CollectionId;
use crate::data_dir::DataDir;
use crate::environment::{check_name, ensure_gitignore, plain_path, render, secrets_path};
use crate::vault_binding::{self as binding, BindError};
use crate::{StorageError, codec, fs};

pub use crate::error::{SecretStoreError, UnavailableReason};
pub use crate::vault_binding::{bound_root, bound_roots};

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
/// browser-protocol pipe). Callers run them off the UI thread. Implementations
/// must bound each call and map a timeout to
/// `Unavailable { reason: TimedOut }`; the operations in this module add a
/// whole-call deadline ([`OPERATION_DEADLINE`]) on top, so N names cannot
/// take N times the per-call bound. The account is a store-neutral address; a
/// store maps it to its own scheme without changing it.
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

/// Whole-call budget for [`load_environment_with`] and the migrations: N
/// secret names must not take N times a per-call bound.
pub const OPERATION_DEADLINE: Duration = Duration::from_secs(30);

/// Per-user context for vault operations: where the binding registry and the
/// environment locks live, and the whole-call deadline.
#[derive(Debug, Clone)]
pub struct VaultCtx {
    pub data: DataDir,
    pub deadline: Duration,
}

impl VaultCtx {
    pub fn new(data: DataDir) -> Self {
        Self {
            data,
            deadline: OPERATION_DEADLINE,
        }
    }

    /// Replace the whole-call deadline (tests use a short one).
    pub fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

const TIMED_OUT: SecretStoreError = SecretStoreError::Unavailable {
    reason: UnavailableReason::TimedOut,
};

/// A store seen through a whole-call deadline: a call that starts after, or
/// returns after, the deadline is a `TimedOut` failure.
struct Bounded<'a> {
    store: &'a dyn SecretStore,
    end: Instant,
}

impl<'a> Bounded<'a> {
    fn new(store: &'a dyn SecretStore, budget: Duration) -> Self {
        Self {
            store,
            end: Instant::now() + budget,
        }
    }

    fn check(&self) -> Result<(), SecretStoreError> {
        if Instant::now() >= self.end {
            Err(TIMED_OUT)
        } else {
            Ok(())
        }
    }

    fn probe(&self) -> Result<(), SecretStoreError> {
        self.check()?;
        self.store.probe()?;
        self.check()
    }

    fn get(&self, a: &str) -> Result<Option<SecretValue>, SecretStoreError> {
        self.check()?;
        let r = self.store.get(a)?;
        self.check()?;
        Ok(r)
    }

    fn set(&self, a: &str, v: &SecretValue) -> Result<(), SecretStoreError> {
        self.check()?;
        self.store.set(a, v)?;
        self.check()
    }

    fn delete(&self, a: &str) -> Result<(), SecretStoreError> {
        self.check()?;
        self.store.delete(a)?;
        self.check()
    }
}

/// Why a store-mode load produced no secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailReason {
    UnknownStoreKind(String),
    NoCollectionId,
    Store(SecretStoreError),
    /// The collection id is bound to another root; nothing was read.
    IdBoundElsewhere {
        bound_root: PathBuf,
    },
    /// The collection id has no binding on this machine; nothing was read.
    /// See [`bind`].
    Unbound,
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
/// problems are `Err`; a store failure is a `Failed` status. Blocking: run it
/// off the UI thread. The whole call is bounded by `ctx.deadline`.
pub fn load_environment_with(
    root: &Path,
    name: &str,
    id: Option<&CollectionId>,
    stores: &dyn SecretStores,
    ctx: &VaultCtx,
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
    let store = match stores.get(kind) {
        Some(s) => Bounded::new(s, ctx.deadline),
        None => {
            return Ok(finish(
                file_secrets()?,
                LoadStatus::FellBackToFile {
                    reason: UnavailableReason::NotConfigured,
                },
            ));
        }
    };
    match store.probe() {
        Ok(()) => {}
        Err(SecretStoreError::Unavailable { reason }) if is_absent(reason) => {
            return Ok(finish(
                file_secrets()?,
                LoadStatus::FellBackToFile { reason },
            ));
        }
        Err(e) => return Ok(failed(FailReason::Store(e))),
    }
    let Some(id) = id else {
        return Ok(failed(FailReason::NoCollectionId));
    };
    match binding::check(&ctx.data, id, root) {
        Ok(()) => {}
        Err(BindError::BoundElsewhere { bound_root }) => {
            return Ok(failed(FailReason::IdBoundElsewhere { bound_root }));
        }
        // `check` never returns `LooksLikeCopy`; fail closed if it ever did.
        Err(BindError::Unbound | BindError::LooksLikeCopy { .. }) => {
            return Ok(failed(FailReason::Unbound));
        }
        Err(BindError::Storage(e)) => return Err(e),
    }
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
    /// The collection id is bound to a different root; nothing was read or
    /// written. See [`rebind`].
    #[error("collection id is bound to another collection root: {}", bound_root.display())]
    IdBoundElsewhere { bound_root: PathBuf },
    /// The collection id has no binding on this machine; nothing was
    /// read or written. See [`bind`].
    #[error("collection id is not bound to this collection root")]
    Unbound,
    /// `rebind` refused: the bound root still exists and still has this
    /// collection id, so this is a copy, not a move. Give the copy its own id
    /// with `collection::assign_new_collection_id`.
    #[error("collection id is still in use by {}; this looks like a copy", bound_root.display())]
    LooksLikeCopy { bound_root: PathBuf },
    /// `bind_additional_checkout` could not verify that the folder is a checkout
    /// of an already-bound root (same git common directory). `bound_roots` are
    /// the roots the id is bound to.
    #[error("this folder is not a verified checkout of a bound collection root")]
    UnverifiedCheckout { bound_roots: Vec<PathBuf> },
    /// `migrate_to_file` found names with no value in the store; nothing was
    /// changed. Pass `allow_drop_unresolved` to drop them.
    #[error("secret names without a value in the store: {}", names.join(", "))]
    UnresolvedNames { names: Vec<String> },
}

/// A store entry that could not be deleted after `migrate_to_file`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteWarning {
    pub name: String,
    pub error: SecretStoreError,
}

/// The old `<env>.secrets.toml` could not be deleted after a successful
/// `migrate_to_store`; the plaintext file is left behind.
#[derive(Debug)]
pub struct LeftoverFileWarning {
    pub path: PathBuf,
    pub error: StorageError,
}

fn invalid(path: &Path, key: &str, message: &str) -> SecretsError {
    SecretsError::Storage(StorageError::Invalid {
        path: path.to_path_buf(),
        key: key.into(),
        message: message.into(),
    })
}

fn map_bind(e: BindError) -> SecretsError {
    match e {
        BindError::Storage(e) => SecretsError::Storage(e),
        BindError::BoundElsewhere { bound_root } => SecretsError::IdBoundElsewhere { bound_root },
        BindError::Unbound => SecretsError::Unbound,
        BindError::LooksLikeCopy { bound_root } => SecretsError::LooksLikeCopy { bound_root },
    }
}

/// Check the binding; an unbound id is [`SecretsError::Unbound`].
fn check_bind(ctx: &VaultCtx, id: &CollectionId, root: &Path) -> Result<(), SecretsError> {
    binding::check(&ctx.data, id, root).map_err(map_bind)
}

/// Bind `id` to `root` after the user confirmed "use keychain secrets for this
/// collection at <path>". Fine if already bound to `root`; errors with
/// [`SecretsError::IdBoundElsewhere`] if bound to another root (see [`rebind`]).
pub fn bind(data: &DataDir, id: &CollectionId, root: &Path) -> Result<(), SecretsError> {
    binding::bind_confirmed(data, id, root).map_err(map_bind)
}

/// Add `new_root` to the roots `id` is bound to, one click from the new
/// folder. Succeeds only when `new_root` carries `id` and is a verified
/// checkout of an already-bound root: both resolve to the same git common
/// directory, read from `.git` files (never from remote URLs). Otherwise
/// [`SecretsError::UnverifiedCheckout`]; use [`share_with_folder`] from a bound
/// checkout instead.
pub fn bind_additional_checkout(
    data: &DataDir,
    id: &CollectionId,
    new_root: &Path,
) -> Result<(), SecretsError> {
    require_same_id(id, new_root)?;
    if binding::verified_siblings(data, id, new_root)?.is_empty() {
        return Err(SecretsError::UnverifiedCheckout {
            bound_roots: binding::bound_roots(data, id)?,
        });
    }
    binding::bind_additional_confirmed(data, id, new_root).map_err(map_bind)
}

/// "Share keychain secrets with another folder": the user started from
/// `from_root`, which must pass the binding check, and picked `new_root`, which
/// must carry the same id. No git relationship is needed.
pub fn share_with_folder(
    data: &DataDir,
    id: &CollectionId,
    from_root: &Path,
    new_root: &Path,
) -> Result<(), SecretsError> {
    binding::check(data, id, from_root).map_err(map_bind)?;
    require_same_id(id, new_root)?;
    binding::bind_additional_confirmed(data, id, new_root).map_err(map_bind)
}

fn require_same_id(id: &CollectionId, root: &Path) -> Result<(), SecretsError> {
    match crate::collection::read_collection_id(root)? {
        Some(found) if &found == id => Ok(()),
        _ => Err(invalid(
            root,
            "id",
            "the folder does not carry this collection id",
        )),
    }
}

/// Move `id`'s binding to `new_root` after the user confirmed the move. Refused
/// with [`SecretsError::LooksLikeCopy`] while the bound root still exists and
/// still has the same id.
pub fn rebind(data: &DataDir, id: &CollectionId, new_root: &Path) -> Result<(), SecretsError> {
    binding::rebind_confirmed(data, id, new_root).map_err(map_bind)
}

fn read_plain(root: &Path, name: &str) -> Result<(PathBuf, DocumentMut), StorageError> {
    check_name(root, name)?;
    let pp = plain_path(root, name);
    let doc = codec::parse(&pp, &fs::read_to_string(&pp)?)?;
    Ok((pp, doc))
}

type Touched = Vec<(String, Option<SecretValue>)>;

/// Put every touched account back: `None` means the entry did not exist.
fn rollback(store: &dyn SecretStore, touched: &Touched) {
    for (a, old) in touched {
        let r = match old {
            Some(v) => store.set(a, v),
            None => store.delete(a),
        };
        if r.is_err() {
            tracing::warn!("could not restore a secret store entry during rollback");
        }
    }
}

/// Copy `secrets` into the store with read-back verification. On failure the
/// entries this call touched are restored; on success they are returned so a
/// later failure can restore them too.
fn copy_into_store(
    store: &dyn SecretStore,
    bounded: &Bounded<'_>,
    id: &CollectionId,
    name: &str,
    secrets: &[Var],
) -> Result<Touched, SecretStoreError> {
    let mut touched: Touched = Vec::new();
    let mut copy = || -> Result<(), SecretStoreError> {
        for v in secrets {
            let account = secret_account(id, name, &v.name);
            let old = bounded.get(&account)?;
            touched.push((account.clone(), old));
            let value = SecretValue::new(v.value.as_str());
            bounded.set(&account, &value)?;
            if bounded.get(&account)?.as_ref() != Some(&value) {
                return Err(SecretStoreError::Other { code: None });
            }
        }
        Ok(())
    };
    match copy() {
        Ok(()) => Ok(touched),
        Err(e) => {
            rollback(store, &touched);
            Err(e)
        }
    }
}

/// Refuse a secrets path that is, or sits behind, a symlink, or that resolves
/// outside the collection. A missing file is fine.
fn check_secrets_path(root: &Path, sp: &Path) -> Result<(), StorageError> {
    crate::fs::confined_regular_file(root, sp, "secrets file").map(|_| ())
}

fn remove_secrets_file(name: &str, sp: &Path) -> Option<LeftoverFileWarning> {
    if sp.exists()
        && let Err(e) = std::fs::remove_file(sp)
    {
        tracing::warn!(environment = name, "could not delete the old secrets file");
        return Some(LeftoverFileWarning {
            path: sp.to_path_buf(),
            error: StorageError::Io {
                path: sp.to_path_buf(),
                source: e,
            },
        });
    }
    None
}

/// Move an environment's file secrets into `store`. Ordered so that a
/// failure before the file is deleted leaves the file and mode unchanged and
/// restores the entries this call touched. Blocking; bounded by
/// `ctx.deadline`.
pub fn migrate_to_store(
    root: &Path,
    name: &str,
    id: &CollectionId,
    kind: StoreKind,
    store: &dyn SecretStore,
    ctx: &VaultCtx,
) -> Result<Option<LeftoverFileWarning>, SecretsError> {
    check_name(root, name)?;
    let _l = binding::env_lock(&ctx.data, &plain_path(root, name))?;
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
    check_bind(ctx, id, root)?;
    let bounded = Bounded::new(store, ctx.deadline);
    bounded.probe()?;
    let touched = copy_into_store(store, &bounded, id, name, &secrets)?;

    doc["secrets"] = toml_edit::value(kind.as_str());
    let names: Vec<String> = secrets.iter().map(|v| v.name.clone()).collect();
    set_names(&mut doc, &names);
    if let Err(e) = fs::atomic_write(&pp, doc.to_string().as_bytes()) {
        rollback(store, &touched);
        return Err(e.into());
    }
    // Store entries and mode are committed; a failed delete is a warning.
    let leftover = remove_secrets_file(name, &sp);
    tracing::debug!(
        environment = name,
        count = names.len(),
        "migrated secrets to store"
    );
    Ok(leftover)
}

/// For an environment already in store mode that has an ignored
/// `<env>.secrets.toml`: copy the file's values into the store, add its names
/// to `secret_names`, then delete the file. Same rollback rules as
/// [`migrate_to_store`]: a failure before the file is deleted restores
/// pre-existing store values; a failed final delete is a warning.
pub fn import_secrets_file(
    root: &Path,
    name: &str,
    id: &CollectionId,
    store: &dyn SecretStore,
    ctx: &VaultCtx,
) -> Result<Option<LeftoverFileWarning>, SecretsError> {
    check_name(root, name)?;
    let _l = binding::env_lock(&ctx.data, &plain_path(root, name))?;
    let (pp, mut doc) = read_plain(root, name)?;
    if !matches!(read_mode(&doc, &pp)?, Mode::Store(_)) {
        return Err(invalid(&pp, "secrets", "environment is not in store mode"));
    }
    let sp = secrets_path(root, name);
    check_secrets_path(root, &sp)?;
    if !sp.exists() {
        return Err(invalid(&sp, "path", "no secrets file to import"));
    }
    let secrets = crate::environment::load_vars(&sp)?;
    let mut names = read_names(&doc, &pp)?;
    check_bind(ctx, id, root)?;
    let bounded = Bounded::new(store, ctx.deadline);
    bounded.probe()?;
    let touched = copy_into_store(store, &bounded, id, name, &secrets)?;

    for v in &secrets {
        if !names.contains(&v.name) {
            names.push(v.name.clone());
        }
    }
    set_names(&mut doc, &names);
    if let Err(e) = fs::atomic_write(&pp, doc.to_string().as_bytes()) {
        rollback(store, &touched);
        return Err(e.into());
    }
    Ok(remove_secrets_file(name, &sp))
}

/// Move an environment's store secrets back into `<env>.secrets.toml`,
/// keeping every other key already in that file. Any read error aborts with
/// nothing changed. Names with no value in the store abort with
/// [`SecretsError::UnresolvedNames`] and nothing changed, unless
/// `allow_drop_unresolved` is set, which drops only those names. A failure
/// deleting store entries afterwards is returned as warnings; the file stays.
/// Blocking; bounded by `ctx.deadline`.
pub fn migrate_to_file(
    root: &Path,
    name: &str,
    id: &CollectionId,
    store: &dyn SecretStore,
    allow_drop_unresolved: bool,
    ctx: &VaultCtx,
) -> Result<Vec<DeleteWarning>, SecretsError> {
    check_name(root, name)?;
    let _l = binding::env_lock(&ctx.data, &plain_path(root, name))?;
    let (pp, mut doc) = read_plain(root, name)?;
    if !matches!(read_mode(&doc, &pp)?, Mode::Store(_)) {
        return Err(invalid(&pp, "secrets", "environment is not in store mode"));
    }
    let names = read_names(&doc, &pp)?;
    check_bind(ctx, id, root)?;
    let bounded = Bounded::new(store, ctx.deadline);
    bounded.probe()?;
    let mut secrets = Vec::new();
    let mut unresolved = Vec::new();
    for n in &names {
        match bounded.get(&secret_account(id, name, n))? {
            Some(v) => secrets.push(Var {
                name: n.clone(),
                value: v.expose().to_string(),
            }),
            None => unresolved.push(n.clone()),
        }
    }
    if !unresolved.is_empty() && !allow_drop_unresolved {
        return Err(SecretsError::UnresolvedNames { names: unresolved });
    }

    let sp = secrets_path(root, name);
    check_secrets_path(root, &sp)?;
    let old_file = if sp.exists() {
        Some(fs::read_to_string(&sp)?)
    } else {
        None
    };
    // Overlay the store values on what the file already holds; never drop keys.
    let mut merged = if old_file.is_some() {
        crate::environment::load_vars(&sp)?
    } else {
        Vec::new()
    };
    for s in secrets.iter() {
        match merged.iter_mut().find(|m| m.name == s.name) {
            Some(m) => m.value = s.value.clone(),
            None => merged.push(s.clone()),
        }
    }
    let text = render(&sp, &merged)?;
    let dir = root.join(crate::environment::DIR);
    ensure_gitignore(&dir)?;
    fs::atomic_write_force_private(&sp, text.as_bytes())?;

    doc["secrets"] = toml_edit::value(StoreKind::File.as_str());
    doc.remove("secret_names");
    if let Err(e) = fs::atomic_write(&pp, doc.to_string().as_bytes()) {
        match &old_file {
            Some(old) => {
                let _ = fs::atomic_write_force_private(&sp, old.as_bytes());
            }
            None => {
                let _ = std::fs::remove_file(&sp);
            }
        }
        return Err(e.into());
    }

    let mut warnings = Vec::new();
    for n in &names {
        if let Err(error) = bounded.delete(&secret_account(id, name, n)) {
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
) -> Result<(PathBuf, DocumentMut, Vec<String>), SecretsError> {
    let (pp, doc) = read_plain(root, name)?;
    if !matches!(read_mode(&doc, &pp)?, Mode::Store(_)) {
        return Err(invalid(&pp, "secrets", "environment is not in store mode"));
    }
    let names = read_names(&doc, &pp)?;
    Ok((pp, doc, names))
}

/// Set one secret in a store-mode environment: the store first, then
/// `secret_names`. Never writes a secrets file. An unbound collection id is
/// refused. The read-modify-write of `<env>.toml` is locked.
pub fn set_secret(
    root: &Path,
    name: &str,
    id: &CollectionId,
    store: &dyn SecretStore,
    var: &str,
    value: &SecretValue,
    ctx: &VaultCtx,
) -> Result<(), SecretsError> {
    check_name(root, name)?;
    let _l = binding::env_lock(&ctx.data, &plain_path(root, name))?;
    let (pp, mut doc, mut names) = store_mode_doc(root, name)?;
    check_bind(ctx, id, root)?;
    store.set(&secret_account(id, name, var), value)?;
    if !names.iter().any(|n| n == var) {
        names.push(var.to_string());
        set_names(&mut doc, &names);
        fs::atomic_write(&pp, doc.to_string().as_bytes())?;
    }
    Ok(())
}

/// Delete one secret from a store-mode environment: the store first, then
/// `secret_names`. Never writes a secrets file. An unbound id is refused.
pub fn delete_secret(
    root: &Path,
    name: &str,
    id: &CollectionId,
    store: &dyn SecretStore,
    var: &str,
    ctx: &VaultCtx,
) -> Result<(), SecretsError> {
    check_name(root, name)?;
    let _l = binding::env_lock(&ctx.data, &plain_path(root, name))?;
    let (pp, mut doc, mut names) = store_mode_doc(root, name)?;
    check_bind(ctx, id, root)?;
    store.delete(&secret_account(id, name, var))?;
    if names.iter().any(|n| n == var) {
        names.retain(|n| n != var);
        set_names(&mut doc, &names);
        fs::atomic_write(&pp, doc.to_string().as_bytes())?;
    }
    Ok(())
}
