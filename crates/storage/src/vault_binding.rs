//! Per-user binding of a collection id to a collection root, and the
//! per-environment write lock.
//!
//! Keychain accounts are keyed by the committed collection id, so a cloned or
//! copied collection with the same id would otherwise reach the original's
//! secrets. The registry lives in the per-user data dir (never in the
//! collection), maps `id -> canonical root`, holds no secret values and is
//! written `0600`. A corrupt or unreadable registry fails closed.
//!
//! Each row also holds a random nonce, mirrored in a git-ignored marker file
//! `environments/.wisp-binding` inside the collection. A binding check needs
//! the marker's nonce to match the row, so a folder recreated at a bound path
//! (no marker) is `Unbound` and needs the bind prompt again.
//!
//! Policy: an unbound id fails closed for every operation; nothing binds
//! implicitly. Only the user's explicit confirmation, [`crate::vault::bind`],
//! creates a row. An id bound to a different canonical root fails closed for
//! every operation until [`rebind`] is called.

use std::collections::{BTreeMap, HashMap};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::StorageError;
use crate::collection::CollectionId;
use crate::data_dir::{DataDir, ensure_private_dir};
use crate::fs;

const REGISTRY_FILE: &str = "vault-bindings.json";
const VERSION: i64 = 2;

/// Why a binding check failed.
#[derive(Debug)]
pub(crate) enum BindError {
    Storage(StorageError),
    BoundElsewhere {
        bound_root: PathBuf,
    },
    /// No registry row for the id, or the collection's marker does not match.
    Unbound,
    /// The bound root still exists and still carries this id.
    LooksLikeCopy {
        bound_root: PathBuf,
    },
}

/// One registry row: the bound canonical root and the nonce its marker holds.
#[derive(Clone)]
struct Row {
    root: String,
    nonce: String,
}

impl From<StorageError> for BindError {
    fn from(e: StorageError) -> Self {
        BindError::Storage(e)
    }
}

fn io(path: &Path, source: std::io::Error) -> StorageError {
    StorageError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn canonical(root: &Path) -> Result<PathBuf, StorageError> {
    std::fs::canonicalize(root).map_err(|e| io(root, e))
}

fn key(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Advisory cross-process lock, released when dropped.
pub(crate) struct FileLock(std::fs::File);

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

fn lock_file(path: &Path) -> Result<FileLock, StorageError> {
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    let mut opts = OpenOptions::new();
    // Windows `File::lock` fails on append-only handles.
    opts.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let f = opts.open(path).map_err(|e| io(path, e))?;
    f.lock().map_err(|e| io(path, e))?;
    Ok(FileLock(f))
}

static REGISTRY_LOCK: Mutex<()> = Mutex::new(());

fn registry_path(data: &DataDir) -> PathBuf {
    data.root().join(REGISTRY_FILE)
}

fn read_registry(path: &Path) -> Result<BTreeMap<String, Row>, StorageError> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(io(path, e)),
    };
    let bad = || StorageError::Parse {
        path: path.to_path_buf(),
        message: "unreadable vault binding registry".into(),
    };
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| bad())?;
    if v.get("version").and_then(|x| x.as_i64()) != Some(VERSION) {
        return Err(bad());
    }
    let map = v
        .get("bindings")
        .and_then(|b| b.as_object())
        .ok_or_else(bad)?;
    map.iter()
        .map(|(k, v)| {
            let root = v.get("root").and_then(|x| x.as_str()).ok_or_else(bad)?;
            let nonce = v.get("nonce").and_then(|x| x.as_str()).ok_or_else(bad)?;
            Ok((
                k.clone(),
                Row {
                    root: root.to_string(),
                    nonce: nonce.to_string(),
                },
            ))
        })
        .collect()
}

fn write_registry(path: &Path, map: &BTreeMap<String, Row>) -> Result<(), StorageError> {
    let rows: serde_json::Map<String, serde_json::Value> = map
        .iter()
        .map(|(k, r)| {
            (
                k.clone(),
                serde_json::json!({ "root": r.root, "nonce": r.nonce }),
            )
        })
        .collect();
    let v = serde_json::json!({ "version": VERSION, "bindings": rows });
    let text = serde_json::to_string_pretty(&v).map_err(|e| StorageError::Parse {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    fs::atomic_write_force_private(path, text.as_bytes())
}

fn marker_path(root: &Path) -> PathBuf {
    root.join(crate::environment::DIR)
        .join(crate::environment::MARKER_FILE)
}

fn new_nonce() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// The marker's nonce; `None` when the marker is missing.
fn read_marker(root: &Path) -> Result<Option<String>, StorageError> {
    let p = marker_path(root);
    match std::fs::read_to_string(&p) {
        Ok(t) => Ok(Some(t.trim().to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(&p, e)),
    }
}

/// Write a fresh nonce to the marker (mode `0600`, git-ignored) and return it.
fn write_marker(root: &Path) -> Result<String, StorageError> {
    let dir = root.join(crate::environment::DIR);
    std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
    crate::environment::ensure_marker_ignored(&dir)?;
    let nonce = new_nonce();
    fs::atomic_write_force_private(&marker_path(root), nonce.as_bytes())?;
    Ok(nonce)
}

/// Check `id` against `root`. An unbound id, or a row whose nonce the
/// collection's marker does not hold, is an error; only [`bind_confirmed`]
/// creates a row.
pub(crate) fn check(data: &DataDir, id: &CollectionId, root: &Path) -> Result<(), BindError> {
    let canon = canonical(root)?;
    let path = registry_path(data);
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    ensure_private_dir(data.root())?;
    let _f = lock_file(&path.with_extension("lock"))?;
    let map = read_registry(&path)?;
    match map.get(id.as_str()) {
        Some(r) if r.root == key(&canon) => match read_marker(&canon)? {
            Some(n) if n == r.nonce => Ok(()),
            _ => Err(BindError::Unbound),
        },
        Some(r) => Err(BindError::BoundElsewhere {
            bound_root: PathBuf::from(&r.root),
        }),
        None => Err(BindError::Unbound),
    }
}

/// Bind an unbound `id` to `root` after the user confirmed it: new nonce into
/// the row and the marker. Already bound to `root` with a matching marker is
/// fine; bound there with a missing or stale marker is re-bound; bound
/// elsewhere is an error (use [`rebind_confirmed`] to move).
pub(crate) fn bind_confirmed(
    data: &DataDir,
    id: &CollectionId,
    root: &Path,
) -> Result<(), BindError> {
    let canon = canonical(root)?;
    let path = registry_path(data);
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    ensure_private_dir(data.root())?;
    let _f = lock_file(&path.with_extension("lock"))?;
    let mut map = read_registry(&path)?;
    match map.get(id.as_str()) {
        Some(r) if r.root == key(&canon) && read_marker(&canon)?.as_deref() == Some(&r.nonce) => {
            Ok(())
        }
        Some(r) if r.root != key(&canon) => Err(BindError::BoundElsewhere {
            bound_root: PathBuf::from(&r.root),
        }),
        _ => {
            let nonce = write_marker(&canon)?;
            map.insert(
                id.as_str().to_string(),
                Row {
                    root: key(&canon),
                    nonce,
                },
            );
            write_registry(&path, &map)?;
            Ok(())
        }
    }
}

/// The canonical root `id` is bound to, if any.
pub fn bound_root(data: &DataDir, id: &CollectionId) -> Result<Option<PathBuf>, StorageError> {
    let path = registry_path(data);
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    Ok(read_registry(&path)?
        .get(id.as_str())
        .map(|r| PathBuf::from(&r.root)))
}

/// Bind `id` to `new_root`, replacing an earlier binding, after the user
/// confirmed a move. Refused with [`BindError::LooksLikeCopy`] while the
/// currently bound root still exists and still carries the same id: that is a
/// copy, not a move.
pub(crate) fn rebind_confirmed(
    data: &DataDir,
    id: &CollectionId,
    new_root: &Path,
) -> Result<(), BindError> {
    let canon = canonical(new_root)?;
    let path = registry_path(data);
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    ensure_private_dir(data.root())?;
    let _f = lock_file(&path.with_extension("lock"))?;
    let mut map = read_registry(&path)?;
    if let Some(r) = map.get(id.as_str())
        && r.root != key(&canon)
    {
        let old = PathBuf::from(&r.root);
        if old.exists() && crate::collection::read_collection_id(&old)?.as_ref() == Some(id) {
            return Err(BindError::LooksLikeCopy { bound_root: old });
        }
    }
    let nonce = write_marker(&canon)?;
    map.insert(
        id.as_str().to_string(),
        Row {
            root: key(&canon),
            nonce,
        },
    );
    write_registry(&path, &map)?;
    Ok(())
}

static ENV_LOCKS: Mutex<Option<HashMap<PathBuf, &'static Mutex<()>>>> = Mutex::new(None);

/// Both locks of one environment file, released on drop.
pub(crate) struct EnvLock {
    _mutex: std::sync::MutexGuard<'static, ()>,
    _file: FileLock,
}

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Serialise read-modify-write of one `<env>.toml`: an in-process mutex keyed
/// by path, then a lock file in the data dir (not in the collection).
pub(crate) fn env_lock(data: &DataDir, plain: &Path) -> Result<EnvLock, StorageError> {
    let canon = plain
        .parent()
        .and_then(|p| std::fs::canonicalize(p).ok())
        .map(|p| p.join(plain.file_name().unwrap_or_default()))
        .unwrap_or_else(|| plain.to_path_buf());
    let m: &'static Mutex<()> = {
        let mut g = ENV_LOCKS.lock().unwrap_or_else(|e| e.into_inner());
        let map = g.get_or_insert_with(HashMap::new);
        map.entry(canon.clone())
            .or_insert_with(|| Box::leak(Box::new(Mutex::new(()))))
    };
    let guard = m.lock().unwrap_or_else(|e| e.into_inner());
    let lock_path = data
        .root()
        .join("locks")
        .join(format!("env-{:016x}.lock", fnv(&key(&canon))));
    let f = lock_file(&lock_path)?;
    Ok(EnvLock {
        _mutex: guard,
        _file: f,
    })
}
