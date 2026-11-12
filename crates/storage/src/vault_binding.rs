//! Per-user binding of a collection id to a collection root, and the
//! per-environment write lock.
//!
//! Keychain accounts are keyed by the committed collection id, so a cloned or
//! copied collection with the same id would otherwise reach the original's
//! secrets. The registry lives in the per-user data dir (never in the
//! collection), maps `id -> canonical root`, holds no secret values and is
//! written `0600`. A corrupt or unreadable registry fails closed.
//!
//! A row is the set of roots bound to an id (several checkouts of one
//! collection), each with a random nonce, mirrored in a git-ignored marker file
//! `environments/.wisp-binding` inside that root. The marker holds one entry
//! per registry (each data dir's registry has a random id), so two data dirs
//! bound to one root do not overwrite each other. A binding check needs the
//! marker's entry for this registry to match the root's nonce, so a folder
//! recreated at a bound path (no marker) is `Unbound` and needs the bind
//! prompt again.
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
const VERSION: i64 = 3;

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

/// The registry: a random id for this data dir (its entry key in every
/// collection marker) and, per collection id, the set of bound roots.
struct Registry {
    id: String,
    rows: BTreeMap<String, Vec<Row>>,
}

fn read_registry(path: &Path) -> Result<Registry, StorageError> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Registry {
                id: new_nonce(),
                rows: BTreeMap::new(),
            });
        }
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
    let id = v
        .get("registry_id")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric()))
        .ok_or_else(bad)?
        .to_string();
    let map = v
        .get("bindings")
        .and_then(|b| b.as_object())
        .ok_or_else(bad)?;
    let rows = map
        .iter()
        .map(|(k, v)| {
            let list = v.as_array().ok_or_else(bad)?;
            let rows = list
                .iter()
                .map(|r| {
                    let root = r.get("root").and_then(|x| x.as_str()).ok_or_else(bad)?;
                    let nonce = r.get("nonce").and_then(|x| x.as_str()).ok_or_else(bad)?;
                    Ok(Row {
                        root: root.to_string(),
                        nonce: nonce.to_string(),
                    })
                })
                .collect::<Result<Vec<_>, StorageError>>()?;
            Ok((k.clone(), rows))
        })
        .collect::<Result<_, StorageError>>()?;
    Ok(Registry { id, rows })
}

fn write_registry(path: &Path, reg: &Registry) -> Result<(), StorageError> {
    let rows: serde_json::Map<String, serde_json::Value> = reg
        .rows
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, rs)| {
            let list: Vec<_> = rs
                .iter()
                .map(|r| serde_json::json!({ "root": r.root, "nonce": r.nonce }))
                .collect();
            (k.clone(), serde_json::Value::Array(list))
        })
        .collect();
    let v = serde_json::json!({ "version": VERSION, "registry_id": reg.id, "bindings": rows });
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

/// The marker path if it is safe to touch (see [`fs::confined_regular_file`]).
/// `Ok(None)` when absent; `Invalid` on any refusal.
fn safe_marker(root: &Path) -> Result<Option<PathBuf>, StorageError> {
    let p = marker_path(root);
    Ok(fs::confined_regular_file(root, &p, "binding marker")?.then_some(p))
}

/// The marker's `registry id -> nonce` entries, one `<registry id> <nonce>`
/// line each. Empty when the marker is missing or refused (fail closed: a
/// refused marker reads as no marker, so the id is `Unbound`).
fn read_marker_entries(root: &Path) -> Result<BTreeMap<String, String>, StorageError> {
    let p = match safe_marker(root) {
        Ok(Some(p)) => p,
        Ok(None) | Err(StorageError::Invalid { .. }) => return Ok(BTreeMap::new()),
        Err(e) => return Err(e),
    };
    let text = match std::fs::read_to_string(&p) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(io(&p, e)),
    };
    Ok(text
        .lines()
        .filter_map(|l| l.split_once(' '))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect())
}

fn read_marker(root: &Path, registry_id: &str) -> Result<Option<String>, StorageError> {
    Ok(read_marker_entries(root)?.remove(registry_id))
}

/// Write a fresh nonce for `registry_id` into the marker (mode `0600`,
/// git-ignored), keeping other registries' entries, and return it. Refuses,
/// writing nothing, when the directory or marker is a symlink.
fn write_marker(root: &Path, registry_id: &str) -> Result<String, StorageError> {
    safe_marker(root)?;
    let dir = root.join(crate::environment::DIR);
    std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
    safe_marker(root)?;
    crate::environment::ensure_marker_ignored(&dir)?;
    let mut entries = read_marker_entries(root)?;
    let nonce = new_nonce();
    entries.insert(registry_id.to_string(), nonce.clone());
    let text: String = entries.iter().map(|(k, v)| format!("{k} {v}\n")).collect();
    fs::atomic_write_force_private(&marker_path(root), text.as_bytes())?;
    Ok(nonce)
}

/// Check `id` against `root`. An unbound id, or a root whose marker entry for
/// this registry does not hold the row's nonce, is an error; only
/// [`bind_confirmed`] and [`bind_additional_confirmed`] create a row.
pub(crate) fn check(data: &DataDir, id: &CollectionId, root: &Path) -> Result<(), BindError> {
    let canon = canonical(root)?;
    let path = registry_path(data);
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    ensure_private_dir(data.root())?;
    let _f = lock_file(&path.with_extension("lock"))?;
    let reg = read_registry(&path)?;
    let rows = reg.rows.get(id.as_str()).map(Vec::as_slice).unwrap_or(&[]);
    match rows.iter().find(|r| r.root == key(&canon)) {
        Some(r) => match read_marker(&canon, &reg.id)? {
            Some(n) if n == r.nonce => Ok(()),
            _ => Err(BindError::Unbound),
        },
        None => match rows.first() {
            Some(r) => Err(BindError::BoundElsewhere {
                bound_root: PathBuf::from(&r.root),
            }),
            None => Err(BindError::Unbound),
        },
    }
}

fn bind_inner(
    data: &DataDir,
    id: &CollectionId,
    root: &Path,
    additional: bool,
) -> Result<(), BindError> {
    let canon = canonical(root)?;
    let path = registry_path(data);
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    ensure_private_dir(data.root())?;
    let _f = lock_file(&path.with_extension("lock"))?;
    let mut reg = read_registry(&path)?;
    let reg_id = reg.id.clone();
    let rows = reg.rows.entry(id.as_str().to_string()).or_default();
    let existing = rows.iter().position(|r| r.root == key(&canon));
    if let Some(i) = existing {
        if read_marker(&canon, &reg_id)?.as_deref() == Some(&rows[i].nonce) {
            return Ok(());
        }
    } else if !additional && let Some(r) = rows.first() {
        return Err(BindError::BoundElsewhere {
            bound_root: PathBuf::from(&r.root),
        });
    }
    let nonce = write_marker(&canon, &reg_id)?;
    match existing {
        Some(i) => rows[i].nonce = nonce,
        None => rows.push(Row {
            root: key(&canon),
            nonce,
        }),
    }
    write_registry(&path, &reg)?;
    Ok(())
}

/// Bind `id` to `root` after the user confirmed it: new nonce into the row and
/// this registry's marker entry. Already bound to `root` with a matching marker
/// is fine; bound there with a missing or stale marker is re-bound; bound only
/// elsewhere is an error (use [`bind_additional_confirmed`] to add a checkout
/// or [`rebind_confirmed`] to move).
pub(crate) fn bind_confirmed(
    data: &DataDir,
    id: &CollectionId,
    root: &Path,
) -> Result<(), BindError> {
    bind_inner(data, id, root, false)
}

/// Add `root` to the roots `id` is bound to (another checkout of the same
/// collection), after the user confirmed it. Existing roots stay bound.
pub(crate) fn bind_additional_confirmed(
    data: &DataDir,
    id: &CollectionId,
    root: &Path,
) -> Result<(), BindError> {
    bind_inner(data, id, root, true)
}

const GIT_FILE_CAP: u64 = 4096;

fn git_refuse(path: &Path, message: &str) -> StorageError {
    StorageError::Invalid {
        path: path.to_path_buf(),
        key: "path".into(),
        message: message.into(),
    }
}

/// Read a small git metadata file (a regular file, never a symlink, capped).
fn read_git_file(path: &Path) -> Result<Option<String>, StorageError> {
    use std::io::Read;
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() || !m.is_file() || m.len() > GIT_FILE_CAP => {
            Err(git_refuse(
                path,
                "git metadata must be a small regular file, not a link",
            ))
        }
        Ok(_) => {
            let mut text = String::new();
            std::fs::File::open(path)
                .and_then(|f| f.take(GIT_FILE_CAP).read_to_string(&mut text))
                .map_err(|e| io(path, e))?;
            Ok(Some(text.trim().to_string()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(path, e)),
    }
}

/// True when `text` (a path read from a `.git` file) could make the OS open a
/// network or device path: a leading double separator (UNC, `//server`), or, on
/// Windows, any UNC/device/verbatim-UNC prefix. Checked before any I/O on it.
fn is_remote_or_device(text: &str) -> bool {
    if text.starts_with("//") || text.starts_with("\\\\") {
        return true;
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        if let Some(Component::Prefix(p)) = Path::new(text).components().next() {
            return !matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_));
        }
    }
    false
}

/// Resolve `.` and `..` without touching the filesystem.
fn lexical(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}

/// `\\?\C:\x` (VerbatimDisk) -> `C:\x` (Disk), so canonical paths compare with
/// the `C:/...` form git writes. Everything else, notably `\\?\UNC\`, is
/// returned unchanged. Identity off Windows.
fn simplify_verbatim(p: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut it = p.components();
        if let Some(Component::Prefix(pre)) = it.next() {
            if let Prefix::VerbatimDisk(d) = pre.kind() {
                let mut out = PathBuf::from(format!("{}:", d as char));
                out.push(std::path::MAIN_SEPARATOR_STR);
                for c in it {
                    if !matches!(c, Component::RootDir) {
                        out.push(c.as_os_str());
                    }
                }
                return out;
            }
        }
    }
    p.to_path_buf()
}

/// Lexical, no I/O: is `gitdir` exactly `<trusted>/worktrees/<name>`, with
/// `<trusted>` a canonical trusted common dir and `<name>` one normal component?
fn exact_worktree_gitdir(gitdir: &Path, trusted: &[PathBuf]) -> bool {
    use std::path::Component;
    let Some(parent) = gitdir.parent() else {
        return false;
    };
    let name_ok = matches!(gitdir.components().next_back(), Some(Component::Normal(_)));
    name_ok
        && parent
            .parent()
            .zip(parent.file_name())
            .is_some_and(|(common, w)| {
                w == "worktrees" && trusted.iter().any(|t| simplify_verbatim(t) == common)
            })
}

/// The canonical git common directory of the checkout at `root`, read from
/// files only (no git binary). A normal checkout has a `.git` directory, which
/// is the common dir. A linked worktree has a `.git` file `gitdir: <path>`
/// naming `<common>/worktrees/<name>`, whose `commondir` file gives the common
/// dir and whose `gitdir` file must point back at this `.git`. `Ok(None)` when
/// `root` is not a verifiable checkout; a symlinked `.git` is `Invalid`.
///
/// `trusted` (the canonical common dirs of already-bound main checkouts, from
/// local paths; see [`anchor_common_dir`]) is required: the gitdir text is joined to the canonical `root` and
/// normalised lexically; it must equal `<trusted>/worktrees/<name>` exactly
/// before anything derived from it is statted or read. UNC/device paths are
/// refused untouched.
fn common_dir(root: &Path, trusted: &[PathBuf]) -> Result<Option<PathBuf>, StorageError> {
    // Canonicalise `root` once; `<canon_root>/.git` is then the identity and is
    // never followed again after the lstat below.
    let Ok(canon_root) = std::fs::canonicalize(root).map(|p| simplify_verbatim(&p)) else {
        return Ok(None);
    };
    let dot_git = canon_root.join(".git");
    let meta = match std::fs::symlink_metadata(&dot_git) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io(&dot_git, e)),
    };
    if meta.file_type().is_symlink() {
        return Err(git_refuse(&dot_git, ".git must not be a symbolic link"));
    }
    let canon = |p: &Path| std::fs::canonicalize(p).ok().map(|c| simplify_verbatim(&c));
    if meta.is_dir() {
        return Ok(Some(dot_git));
    }
    let Some(text) = read_git_file(&dot_git)? else {
        return Ok(None);
    };
    let Some(target) = text.strip_prefix("gitdir:").map(str::trim) else {
        return Ok(None);
    };
    if is_remote_or_device(target) {
        return Ok(None);
    }
    let joined = simplify_verbatim(&lexical(&canon_root.join(target)));
    if !exact_worktree_gitdir(&joined, trusted) {
        return Ok(None);
    }
    // Matched exactly: now it may be touched, but only if no symlink lies on
    // the path.
    match std::fs::symlink_metadata(&joined) {
        Ok(m) if m.is_dir() => {}
        _ => return Ok(None),
    }
    let gitdir = match canon(&joined) {
        Some(g) if g == joined => g,
        _ => return Ok(None),
    };
    // A linked worktree always has a `commondir`; without one the gitdir is a
    // whole repository chosen by this folder, which proves nothing.
    let Some(common) = read_git_file(&gitdir.join("commondir"))? else {
        return Ok(None);
    };
    if is_remote_or_device(&common) {
        return Ok(None);
    }
    let Some(common) = canon(&gitdir.join(common)) else {
        return Ok(None);
    };
    // The gitdir must live directly under `<common>/worktrees/`, inside the
    // repository's own `.git`, which this folder's author cannot have written.
    if gitdir.parent() != Some(common.join("worktrees").as_path()) {
        return Ok(None);
    }
    // The worktree's gitdir must name this `.git` back, so a forged `.git`
    // file cannot claim someone else's repository.
    let back = read_git_file(&gitdir.join("gitdir"))?;
    let back_ok = back
        .filter(|b| !is_remote_or_device(b))
        .and_then(|b| canon(&gitdir.join(b)))
        .is_some_and(|b| b == dot_git);
    Ok(back_ok.then_some(common))
}

/// The common dir of a bound root that may anchor verification: only a root
/// whose `.git` is a real directory (lstat, not a symlink), i.e. a main
/// checkout. Its common dir is `<canonical root>/.git`; nothing else is read.
/// A bound worktree (`.git` file) or a symlinked `.git` is never an anchor, so
/// no gitdir/commondir of a bound root is ever followed.
fn anchor_common_dir(root: &Path) -> Option<PathBuf> {
    let canon_root = std::fs::canonicalize(root)
        .map(|p| simplify_verbatim(&p))
        .ok()?;
    let dot_git = canon_root.join(".git");
    let meta = std::fs::symlink_metadata(&dot_git).ok()?;
    (!meta.file_type().is_symlink() && meta.is_dir()).then_some(dot_git)
}

/// The bound roots of `id` (that still pass the binding check) whose git
/// common directory equals `new_root`'s. Empty means `new_root` is unverified.
/// Only bound main checkouts (real `.git` directory) anchor: a new folder is
/// one-click verifiable only as a worktree of a bound main checkout.
pub(crate) fn verified_siblings(
    data: &DataDir,
    id: &CollectionId,
    new_root: &Path,
) -> Result<Vec<PathBuf>, StorageError> {
    // Trusted first: the common dirs of the bound roots, from local paths.
    let mut bound = Vec::new();
    for r in bound_roots(data, id)? {
        if check(data, id, &r).is_err() {
            continue;
        }
        if let Some(c) = anchor_common_dir(&r) {
            bound.push((r, c));
        }
    }
    let trusted: Vec<PathBuf> = bound.iter().map(|(_, c)| c.clone()).collect();
    let Some(want) = common_dir(new_root, &trusted)? else {
        return Ok(Vec::new());
    };
    Ok(bound
        .into_iter()
        .filter(|(_, c)| *c == want)
        .map(|(r, _)| r)
        .collect())
}

/// Every canonical root `id` is bound to (in binding order).
pub fn bound_roots(data: &DataDir, id: &CollectionId) -> Result<Vec<PathBuf>, StorageError> {
    let path = registry_path(data);
    let _g = REGISTRY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    Ok(read_registry(&path)?
        .rows
        .get(id.as_str())
        .map(|rs| rs.iter().map(|r| PathBuf::from(&r.root)).collect())
        .unwrap_or_default())
}

/// The first canonical root `id` is bound to, if any.
pub fn bound_root(data: &DataDir, id: &CollectionId) -> Result<Option<PathBuf>, StorageError> {
    Ok(bound_roots(data, id)?.into_iter().next())
}

/// Bind `id` to `new_root` after the user confirmed a move: the bound roots
/// that were moved away (gone, or no longer carrying this id) are replaced by
/// `new_root`; roots that are still live checkouts stay bound. Refused with
/// [`BindError::LooksLikeCopy`] when every other bound root still exists and
/// still carries the same id: that is a copy, not a move.
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
    let mut reg = read_registry(&path)?;
    let reg_id = reg.id.clone();
    let rows = reg.rows.entry(id.as_str().to_string()).or_default();
    let is_new = !rows.iter().any(|r| r.root == key(&canon));
    let mut live_other = None;
    let mut keep = Vec::new();
    for r in rows.iter() {
        if r.root == key(&canon) {
            continue;
        }
        let old = PathBuf::from(&r.root);
        if old.exists() && crate::collection::read_collection_id(&old)?.as_ref() == Some(id) {
            live_other.get_or_insert(old);
            keep.push(r.clone());
        }
    }
    if is_new
        && keep.len() == rows.len()
        && let Some(old) = live_other
    {
        return Err(BindError::LooksLikeCopy { bound_root: old });
    }
    let nonce = write_marker(&canon, &reg_id)?;
    keep.push(Row {
        root: key(&canon),
        nonce,
    });
    *rows = keep;
    write_registry(&path, &reg)?;
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

#[cfg(test)]
mod tests {
    use super::is_remote_or_device;

    #[cfg(windows)]
    #[test]
    fn simplify_verbatim_only_touches_verbatim_disk() {
        use super::simplify_verbatim;
        use std::path::Path;
        assert_eq!(
            simplify_verbatim(Path::new(r"\\?\C:\x\y")),
            Path::new(r"C:\x\y")
        );
        assert_eq!(simplify_verbatim(Path::new(r"C:\x")), Path::new(r"C:\x"));
        let unc = Path::new(r"\\?\UNC\srv\share\x");
        assert_eq!(simplify_verbatim(unc), unc);
    }

    #[cfg(windows)]
    #[test]
    fn disk_form_gitdir_matches_verbatim_trusted_dirs() {
        use super::{exact_worktree_gitdir, lexical, simplify_verbatim};
        use std::path::{Path, PathBuf};
        let trusted = vec![PathBuf::from(r"\\?\C:\repo\.git")];
        let root = simplify_verbatim(Path::new(r"\\?\C:\wt"));
        let joined = simplify_verbatim(&lexical(&root.join("C:/repo/.git/worktrees/w")));
        assert!(exact_worktree_gitdir(&joined, &trusted));
        let other = simplify_verbatim(&lexical(&root.join("C:/other/.git/worktrees/w")));
        assert!(!exact_worktree_gitdir(&other, &trusted));
    }

    #[test]
    fn remote_and_device_prefixes_are_detected() {
        for t in [
            "//server/share/x",
            "\\\\server\\share\\x",
            "\\\\?\\UNC\\server\\share",
            "//?/UNC/server/share",
            "//?/C:/x",
            "\\\\.\\pipe\\x",
            "//./pipe/x",
        ] {
            assert!(is_remote_or_device(t), "{t}");
        }
        for t in ["sub/.git/worktrees/w", "/home/a/.git", "../x"] {
            assert!(!is_remote_or_device(t), "{t}");
        }
    }
}
