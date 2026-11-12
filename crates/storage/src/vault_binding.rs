//! Per-user binding of a collection id to a collection root, and the
//! per-environment write lock.
//!
//! Keychain accounts are keyed by the committed collection id, so a cloned or
//! copied collection with the same id would otherwise reach the original's
//! secrets. The registry lives in the per-user data dir (never in the
//! collection), maps `id -> canonical root`, holds no secret values and is
//! written `0600`. A corrupt or unreadable registry fails closed.
//!
//! Policy: an unbound id fails closed for reads and deletes. A store write
//! (set, migrate-to-store, import) binds an unbound id only when the
//! environment has no secret names yet (first-time setup); otherwise the user
//! must confirm with [`crate::vault::bind`]. An id bound to a different canonical root fails closed for
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
const VERSION: i64 = 1;

/// Why a binding check failed.
#[derive(Debug)]
pub(crate) enum BindError {
    Storage(StorageError),
    BoundElsewhere {
        bound_root: PathBuf,
    },
    /// No registry row for the id and the caller may not create one.
    Unbound,
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

fn read_registry(path: &Path) -> Result<BTreeMap<String, String>, StorageError> {
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
            v.as_str()
                .map(|s| (k.clone(), s.to_string()))
                .ok_or_else(bad)
        })
        .collect()
}

fn write_registry(path: &Path, map: &BTreeMap<String, String>) -> Result<(), StorageError> {
    let v = serde_json::json!({ "version": VERSION, "bindings": map });
    let text = serde_json::to_string_pretty(&v).map_err(|e| StorageError::Parse {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    fs::atomic_write_force_private(path, text.as_bytes())
}

/// Check `id` against `root`. With `bind`, an unbound id is bound to `root`.
pub(crate) fn check(
    data: &DataDir,
    id: &CollectionId,
    root: &Path,
    bind: bool,
) -> Result<(), BindError> {
    let canon = canonical(root)?;
    let path = registry_path(data);
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    ensure_private_dir(data.root())?;
    let _f = lock_file(&path.with_extension("lock"))?;
    let mut map = read_registry(&path)?;
    match map.get(id.as_str()) {
        Some(b) if *b == key(&canon) => Ok(()),
        Some(b) => Err(BindError::BoundElsewhere {
            bound_root: PathBuf::from(b),
        }),
        None => {
            if bind {
                map.insert(id.as_str().to_string(), key(&canon));
                write_registry(&path, &map)?;
                Ok(())
            } else {
                Err(BindError::Unbound)
            }
        }
    }
}

/// Bind an unbound `id` to `root` after the user confirmed it. Already bound
/// to `root` is fine; bound elsewhere is an error (use [`rebind`] to move).
pub(crate) fn bind_confirmed(
    data: &DataDir,
    id: &CollectionId,
    root: &Path,
) -> Result<(), BindError> {
    check(data, id, root, true)
}

/// The canonical root `id` is bound to, if any.
pub fn bound_root(data: &DataDir, id: &CollectionId) -> Result<Option<PathBuf>, StorageError> {
    let path = registry_path(data);
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    Ok(read_registry(&path)?.get(id.as_str()).map(PathBuf::from))
}

/// Bind `id` to `new_root`, replacing any earlier binding. Call only after
/// the user has confirmed the move.
pub fn rebind(data: &DataDir, id: &CollectionId, new_root: &Path) -> Result<(), StorageError> {
    let canon = canonical(new_root)?;
    let path = registry_path(data);
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    ensure_private_dir(data.root())?;
    let _f = lock_file(&path.with_extension("lock"))?;
    let mut map = read_registry(&path)?;
    map.insert(id.as_str().to_string(), key(&canon));
    write_registry(&path, &map)
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
