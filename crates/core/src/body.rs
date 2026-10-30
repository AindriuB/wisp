//! Request body store: spooling response bodies to memory or disk.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Bodies larger than this are spilled to a temp file by default.
pub const DEFAULT_MEMORY_LIMIT: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BodyId(pub u64);

/// Plain-data reference to a body held in a `BodyStore`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BodyHandle {
    pub id: BodyId,
    pub len: u64,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum BodyError {
    #[error("body io error: {0}")]
    Io(String),
    #[error("unknown body handle: {0:?}")]
    UnknownHandle(BodyId),
}

impl From<std::io::Error> for BodyError {
    fn from(e: std::io::Error) -> Self {
        BodyError::Io(e.to_string())
    }
}

/// Process-wide so body ids never collide across stores.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

enum Entry {
    Memory(Vec<u8>),
    File(PathBuf),
}

struct Inner {
    memory_limit: u64,
    spill_base: PathBuf,
    entries: Mutex<HashMap<BodyId, Entry>>,
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, HashMap<BodyId, Entry>> {
        self.entries.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        let entries = self.entries.get_mut().unwrap_or_else(|p| p.into_inner());
        for (_, entry) in entries.drain() {
            if let Entry::File(path) = entry {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// Current uid, read from the owner of a file this process just created.
#[cfg(unix)]
fn current_uid() -> Result<u32, BodyError> {
    use std::os::unix::fs::MetadataExt;
    let probe = std::env::temp_dir().join(format!(".wisp-probe-{}", uuid::Uuid::new_v4().simple()));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)?;
    let uid = file.metadata().map(|m| m.uid());
    drop(file);
    let _ = std::fs::remove_file(&probe);
    Ok(uid?)
}

/// Returns the private per-user spill directory under `base`, creating it if absent.
#[cfg(unix)]
fn spill_dir(base: &std::path::Path) -> Result<PathBuf, BodyError> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    let uid = current_uid()?;
    let dir = base.join(format!("wisp-{uid}"));
    let mut created = false;
    loop {
        match std::fs::symlink_metadata(&dir) {
            Ok(md) => {
                if !md.is_dir() {
                    return Err(BodyError::Io(format!(
                        "spill path {} is not a directory",
                        dir.display()
                    )));
                }
                if md.uid() != uid || md.mode() & 0o077 != 0 {
                    return Err(BodyError::Io(format!(
                        "spill directory {} is not private to this user",
                        dir.display()
                    )));
                }
                return Ok(dir);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !created => {
                created = true;
                match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
                    Ok(()) => {}
                    // Lost a creation race: loop and inspect what is there.
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(e.into()),
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
}

#[cfg(not(unix))]
fn spill_dir(base: &std::path::Path) -> Result<PathBuf, BodyError> {
    let dir = base.join("wisp");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Creates a new, uniquely named, owner-only spill file.
fn create_spill(base: &std::path::Path) -> Result<Spill, BodyError> {
    let dir = spill_dir(base)?;
    let path = dir.join(format!(
        "wisp-{}-{}.body",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let file = opts.open(&path)?;
    Ok(Spill { file, path })
}

/// Holds response bodies in memory up to a limit, then on disk.
#[derive(Clone)]
pub struct BodyStore {
    inner: Arc<Inner>,
}

impl BodyStore {
    pub fn new(memory_limit: u64) -> Self {
        Self::with_spill_base(memory_limit, std::env::temp_dir())
    }

    pub(crate) fn with_spill_base(memory_limit: u64, base: PathBuf) -> Self {
        Self {
            inner: Arc::new(Inner {
                memory_limit,
                spill_base: base,
                entries: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn writer(&self) -> BodyWriter {
        let id = BodyId(NEXT_ID.fetch_add(1, Ordering::Relaxed));
        BodyWriter {
            inner: self.inner.clone(),
            id,
            len: 0,
            buf: Vec::new(),
            spill: None,
            failed: false,
        }
    }

    /// Reads up to `len` bytes starting at `offset`; short or empty at the end.
    pub fn read(&self, handle: &BodyHandle, offset: u64, len: usize) -> Result<Vec<u8>, BodyError> {
        let path = {
            let entries = self.inner.lock();
            match entries.get(&handle.id) {
                None => return Err(BodyError::UnknownHandle(handle.id)),
                Some(Entry::Memory(data)) => {
                    let total = data.len() as u64;
                    if offset >= total {
                        return Ok(Vec::new());
                    }
                    let start = offset as usize;
                    let end = start.saturating_add(len).min(data.len());
                    return Ok(data[start..end].to_vec());
                }
                Some(Entry::File(path)) => path.clone(),
            }
        };
        if offset >= handle.len {
            return Ok(Vec::new());
        }
        let mut file = File::open(&path).map_err(|e| match e.kind() {
            // Released between the lookup and the open.
            std::io::ErrorKind::NotFound => BodyError::UnknownHandle(handle.id),
            _ => BodyError::from(e),
        })?;
        file.seek(SeekFrom::Start(offset))?;
        let want = (handle.len - offset).min(len as u64);
        let mut out = Vec::new();
        file.take(want).read_to_end(&mut out)?;
        Ok(out)
    }

    pub fn is_spilled(&self, handle: &BodyHandle) -> bool {
        matches!(self.inner.lock().get(&handle.id), Some(Entry::File(_)))
    }

    /// Frees the body, deleting its temp file if any. Unknown handles are ignored.
    pub fn release(&self, handle: &BodyHandle) {
        let entry = self.inner.lock().remove(&handle.id);
        if let Some(Entry::File(path)) = entry {
            let _ = std::fs::remove_file(path);
        }
    }
}

struct Spill {
    file: File,
    path: PathBuf,
}

/// Accumulates a body. Dropping without `finish` discards the data.
pub struct BodyWriter {
    inner: Arc<Inner>,
    id: BodyId,
    len: u64,
    buf: Vec<u8>,
    spill: Option<Spill>,
    /// Set once a write fails; the body is incomplete so later calls error.
    failed: bool,
}

impl BodyWriter {
    pub fn write(&mut self, data: &[u8]) -> Result<(), BodyError> {
        if self.failed {
            return Err(BodyError::Io("body writer failed earlier".into()));
        }
        let result = self.write_inner(data);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn write_inner(&mut self, data: &[u8]) -> Result<(), BodyError> {
        let new_len = self.len + data.len() as u64;
        if self.spill.is_none() && new_len > self.inner.memory_limit {
            let spill = create_spill(&self.inner.spill_base)?;
            // Registered before any write so Drop removes it on failure.
            let spill = self.spill.insert(spill);
            spill.file.write_all(&self.buf)?;
            self.buf = Vec::new();
        }
        match self.spill.as_mut() {
            Some(spill) => spill.file.write_all(data)?,
            None => self.buf.extend_from_slice(data),
        }
        self.len = new_len;
        Ok(())
    }

    pub fn finish(mut self) -> Result<BodyHandle, BodyError> {
        if self.failed {
            return Err(BodyError::Io("body writer failed earlier".into()));
        }
        let entry = match self.spill.take() {
            Some(mut spill) => {
                if let Err(e) = spill.file.flush() {
                    let _ = std::fs::remove_file(&spill.path);
                    return Err(e.into());
                }
                Entry::File(spill.path)
            }
            None => Entry::Memory(std::mem::take(&mut self.buf)),
        };
        self.inner.lock().insert(self.id, entry);
        Ok(BodyHandle {
            id: self.id,
            len: self.len,
        })
    }
}

impl Drop for BodyWriter {
    fn drop(&mut self) {
        if let Some(spill) = self.spill.take() {
            drop(spill.file);
            let _ = std::fs::remove_file(spill.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i % 251) as u8).collect()
    }

    fn put(store: &BodyStore, data: &[u8], chunk: usize) -> BodyHandle {
        let mut w = store.writer();
        for c in data.chunks(chunk) {
            w.write(c).unwrap();
        }
        w.finish().unwrap()
    }

    /// Test-owned base directory, removed on drop.
    struct TempBase(PathBuf);

    impl TempBase {
        fn new() -> Self {
            let p =
                std::env::temp_dir().join(format!("wisp-test-{}", uuid::Uuid::new_v4().simple()));
            std::fs::create_dir(&p).unwrap();
            Self(p)
        }
    }

    impl Drop for TempBase {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn spill_path(store: &BodyStore, h: &BodyHandle) -> PathBuf {
        match store.inner.lock().get(&h.id) {
            Some(Entry::File(p)) => p.clone(),
            _ => panic!("not spilled"),
        }
    }

    fn files_in(dir: &std::path::Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .map(|r| r.map(|e| e.unwrap().path()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn small_body_stays_in_memory() {
        let store = BodyStore::new(4096);
        let data = bytes(1024);
        let h = put(&store, &data, 1024);
        assert!(!store.is_spilled(&h));
        assert_eq!(h.len, 1024);
        assert_eq!(store.read(&h, 0, 1024).unwrap(), data);
    }

    #[test]
    fn large_body_spills_and_serves_ranges() {
        let store = BodyStore::new(4096);
        let data = bytes(10 * 1024);
        let h = put(&store, &data, 3 * 1024);
        assert!(store.is_spilled(&h));
        assert!(spill_path(&store, &h).exists());
        for off in [0usize, 4095, 9000] {
            assert_eq!(
                store.read(&h, off as u64, 100).unwrap(),
                data[off..off + 100]
            );
        }
    }

    #[test]
    fn read_past_end_returns_tail_or_empty() {
        let store = BodyStore::new(4096);
        for n in [100usize, 6000] {
            let data = bytes(n);
            let h = put(&store, &data, 500);
            assert_eq!(
                store.read(&h, (n - 10) as u64, 100).unwrap(),
                data[n - 10..]
            );
            assert!(store.read(&h, n as u64, 10).unwrap().is_empty());
            assert!(store.read(&h, n as u64 + 50, 10).unwrap().is_empty());
        }
    }

    #[test]
    fn release_removes_file_and_handle() {
        let store = BodyStore::new(10);
        let h = put(&store, &bytes(100), 100);
        let path = spill_path(&store, &h);
        assert!(path.exists());
        store.release(&h);
        assert!(!path.exists());
        assert_eq!(store.read(&h, 0, 1), Err(BodyError::UnknownHandle(h.id)));
    }

    #[test]
    fn dropping_spilled_writer_removes_file() {
        let base = TempBase::new();
        let store = BodyStore::with_spill_base(10, base.0.clone());
        let mut w = store.writer();
        w.write(&bytes(100)).unwrap();
        let path = w.spill.as_ref().unwrap().path.clone();
        assert!(path.exists());
        drop(w);
        assert!(!path.exists());
    }

    #[test]
    fn dropping_last_store_removes_files() {
        let store = BodyStore::new(10);
        let clone = store.clone();
        let h1 = put(&store, &bytes(100), 100);
        let h2 = put(&clone, &bytes(200), 100);
        let (p1, p2) = (spill_path(&store, &h1), spill_path(&store, &h2));
        drop(store);
        assert!(p1.exists() && p2.exists());
        drop(clone);
        assert!(!p1.exists() && !p2.exists());
    }

    #[cfg(unix)]
    #[test]
    fn spill_file_and_dir_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let base = TempBase::new();
        let store = BodyStore::with_spill_base(10, base.0.clone());
        let h = put(&store, &bytes(100), 100);
        let path = spill_path(&store, &h);
        let file_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let dir_mode = std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);
        assert_eq!(dir_mode, 0o700);
    }

    #[test]
    fn independent_stores_use_distinct_names() {
        let base = TempBase::new();
        let a = BodyStore::with_spill_base(10, base.0.clone());
        let b = BodyStore::with_spill_base(10, base.0.clone());
        let (da, db) = (bytes(100), bytes(150));
        let (ha, hb) = (put(&a, &da, 100), put(&b, &db, 100));
        let (pa, pb) = (spill_path(&a, &ha), spill_path(&b, &hb));
        assert_ne!(pa.file_name(), pb.file_name());
        let prefix = format!("wisp-{}-", std::process::id());
        for p in [&pa, &pb] {
            let name = p.file_name().unwrap().to_str().unwrap();
            let hex = name
                .strip_prefix(prefix.as_str())
                .and_then(|n| n.strip_suffix(".body"))
                .unwrap();
            assert_eq!(hex.len(), 32);
            assert!(hex.bytes().all(|c| c.is_ascii_hexdigit()));
        }
        assert_eq!(a.read(&ha, 0, 1000).unwrap(), da);
        assert_eq!(b.read(&hb, 0, 1000).unwrap(), db);
    }

    #[cfg(unix)]
    fn spill_fails_in(base: &TempBase) {
        let store = BodyStore::with_spill_base(10, base.0.clone());
        let mut w = store.writer();
        assert!(matches!(w.write(&bytes(100)), Err(BodyError::Io(_))));
        drop(w);
        let created: Vec<_> = std::fs::read_dir(&base.0)
            .unwrap()
            .flat_map(|e| files_in(&e.unwrap().path()))
            .collect();
        assert!(created.is_empty(), "{created:?}");
    }

    #[cfg(unix)]
    #[test]
    fn spill_rejects_loose_or_non_directory_spill_path() {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        let uid = current_uid().unwrap();

        // Group/other bits set.
        let base = TempBase::new();
        let dir = base.0.join(format!("wisp-{uid}"));
        std::fs::DirBuilder::new().mode(0o755).create(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        spill_fails_in(&base);

        // Regular file.
        let base = TempBase::new();
        std::fs::write(base.0.join(format!("wisp-{uid}")), b"x").unwrap();
        spill_fails_in(&base);

        // Symlink to a private directory.
        let base = TempBase::new();
        let target = base.0.join("target");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&target)
            .unwrap();
        std::os::unix::fs::symlink(&target, base.0.join(format!("wisp-{uid}"))).unwrap();
        spill_fails_in(&base);
    }

    #[cfg(unix)]
    #[test]
    fn failed_spill_does_not_count_bytes_and_poisons_writer() {
        use std::os::unix::fs::PermissionsExt;
        let base = TempBase::new();
        let dir = base.0.join(format!("wisp-{}", current_uid().unwrap()));
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let store = BodyStore::with_spill_base(10, base.0.clone());
        let mut w = store.writer();
        w.write(&[0; 5]).unwrap();
        assert!(w.write(&[0; 10]).is_err());
        assert_eq!(w.len, 5);
        assert!(w.write(&[0; 1]).is_err());
        assert!(w.finish().is_err());
    }
}
