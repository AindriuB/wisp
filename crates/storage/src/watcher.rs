//! Filesystem watcher with debounced change events.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use notify::event::{CreateKind, RemoveKind};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

use crate::{STATE_DIR, StorageError, TEMP_PREFIX};

/// Repeated events for one path inside this window collapse into one `Changed`.
const COALESCE: Duration = Duration::from_millis(100);

/// A relevant file under the collection changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    /// Relative to the collection root.
    pub path: PathBuf,
}

/// Watches a collection folder; dropping it stops watching and closes the channel.
pub struct Watcher {
    _inner: RecommendedWatcher,
}

impl Watcher {
    pub fn start(root: &Path) -> Result<(Watcher, async_channel::Receiver<Changed>), StorageError> {
        let root = root.canonicalize().map_err(|source| StorageError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        let (raw_tx, raw_rx) = mpsc::channel::<notify::Event>();
        let mut inner =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| match res {
                Ok(event) => {
                    let _ = raw_tx.send(event);
                }
                Err(e) => tracing::warn!(error = %e, paths = ?e.paths, "watcher error"),
            })
            .map_err(watch_err)?;
        inner
            .watch(&root, RecursiveMode::Recursive)
            .map_err(watch_err)?;

        // Scan after watching starts so no directory is missed; a rename
        // source is no longer on disk at emit time, so it must be known here.
        let mut dirs = HashSet::new();
        scan_dirs(&root, Path::new(""), &mut dirs);

        let (tx, rx) = async_channel::unbounded();
        std::thread::Builder::new()
            .name("wisp-watcher".into())
            .spawn(move || run(&root, raw_rx, tx, dirs))
            .map_err(|e| StorageError::Watch {
                message: e.to_string(),
            })?;
        Ok((Watcher { _inner: inner }, rx))
    }
}

fn watch_err(e: notify::Error) -> StorageError {
    StorageError::Watch {
        message: e.to_string(),
    }
}

/// Returns the root-relative path if the event path is relevant.
fn relevant(root: &Path, path: &Path) -> Option<PathBuf> {
    let rel = path.strip_prefix(root).ok()?;
    let first = rel.components().next()?.as_os_str();
    if first == ".git" || first == STATE_DIR {
        return None;
    }
    let name = rel.file_name()?.to_string_lossy();
    if name.starts_with(TEMP_PREFIX) {
        return None;
    }
    Some(rel.to_path_buf())
}

/// Records every relevant directory under `root/rel` in `dirs`.
fn scan_dirs(root: &Path, rel: &Path, dirs: &mut HashSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root.join(rel)) else {
        return;
    };
    for entry in entries.flatten() {
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        if !is_dir {
            continue;
        }
        if let Some(sub) = relevant(root, &entry.path()) {
            scan_dirs(root, &sub, dirs);
            dirs.insert(sub);
        }
    }
}

/// True if `rel` is a directory, or a known directory that is now gone
/// (removed or renamed away), so it must not be reported as a file change.
fn skip_directory(root: &Path, rel: &Path, dirs: &mut HashSet<PathBuf>) -> bool {
    let path = root.join(rel);
    // `symlink_metadata` does not follow links: a symlinked directory is never walked.
    let is_real_dir = std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir());
    if is_real_dir {
        // Also learn directories inside it (moved in, or renamed from a parent).
        scan_dirs(root, rel, dirs);
        dirs.insert(rel.to_path_buf());
        return true;
    }
    let known = dirs.remove(rel);
    if known {
        // Forget everything that lived under the old path.
        dirs.retain(|d| !d.starts_with(rel));
    }
    known && !path.exists()
}

fn run(
    root: &Path,
    raw: mpsc::Receiver<notify::Event>,
    tx: async_channel::Sender<Changed>,
    mut dirs: HashSet<PathBuf>,
) {
    let mut pending: HashMap<PathBuf, Instant> = HashMap::new();
    loop {
        let wait = pending
            .values()
            .map(|t| (*t + COALESCE).saturating_duration_since(Instant::now()))
            .min();
        let got = match wait {
            Some(d) => raw.recv_timeout(d),
            None => raw.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match got {
            Ok(event) => {
                if matches!(event.kind, EventKind::Access(_) | EventKind::Other) {
                    continue;
                }
                let folder_kind = matches!(
                    event.kind,
                    EventKind::Create(CreateKind::Folder) | EventKind::Remove(RemoveKind::Folder)
                );
                for p in &event.paths {
                    if let Some(rel) = relevant(root, p) {
                        if folder_kind || p.is_dir() {
                            dirs.insert(rel.clone());
                        }
                        pending.insert(rel, Instant::now());
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let now = Instant::now();
        let mut due: Vec<(Instant, PathBuf)> = pending
            .iter()
            .filter(|(_, t)| now.duration_since(**t) >= COALESCE)
            .map(|(p, t)| (*t, p.clone()))
            .collect();
        due.sort();
        for (_, rel) in due {
            pending.remove(&rel);
            if skip_directory(root, &rel, &mut dirs) {
                continue;
            }
            if tx.send_blocking(Changed { path: rel }).is_err() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const DEADLINE: Duration = Duration::from_secs(5);

    fn next(rx: &async_channel::Receiver<Changed>) -> Option<Changed> {
        let end = Instant::now() + DEADLINE;
        loop {
            match rx.try_recv() {
                Ok(c) => return Some(c),
                Err(async_channel::TryRecvError::Closed) => return None,
                Err(async_channel::TryRecvError::Empty) => {
                    if Instant::now() > end {
                        return None;
                    }
                    std::thread::yield_now();
                }
            }
        }
    }

    fn setup() -> (tempfile::TempDir, Watcher, async_channel::Receiver<Changed>) {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("users")).unwrap();
        let (w, rx) = Watcher::start(dir.path()).unwrap();
        (dir, w, rx)
    }

    fn changed(p: &str) -> Changed {
        Changed { path: p.into() }
    }

    #[test]
    fn file_changes_are_reported_once_per_burst() {
        let (dir, _w, rx) = setup();
        let f = dir.path().join("users/get-user.toml");
        fs::write(&f, "a").unwrap();
        assert_eq!(next(&rx), Some(changed("users/get-user.toml")));

        use std::io::Write;
        let mut h = fs::OpenOptions::new().append(true).open(&f).unwrap();
        for _ in 0..5 {
            h.write_all(b"more").unwrap();
            h.flush().unwrap();
        }
        drop(h);
        assert_eq!(next(&rx), Some(changed("users/get-user.toml")));

        fs::remove_file(&f).unwrap();
        assert_eq!(next(&rx), Some(changed("users/get-user.toml")));
        // sentinel proves no duplicate came between
        fs::write(dir.path().join("sentinel.toml"), "").unwrap();
        assert_eq!(next(&rx), Some(changed("sentinel.toml")));
    }

    #[test]
    fn ignores_state_and_temp_files() {
        let (dir, _w, rx) = setup();
        fs::create_dir(dir.path().join(".wisp")).unwrap();
        fs::write(dir.path().join(".wisp/history.jsonl"), "x").unwrap();
        fs::write(dir.path().join(".wisp-tmp-x"), "x").unwrap();
        fs::write(dir.path().join("sentinel.toml"), "").unwrap();
        assert_eq!(next(&rx), Some(changed("sentinel.toml")));
    }

    #[test]
    fn removed_directories_are_not_reported() {
        let (dir, _w, rx) = setup();
        fs::write(dir.path().join("users/a.toml"), "").unwrap();
        assert_eq!(next(&rx), Some(changed("users/a.toml")));
        fs::remove_dir_all(dir.path().join("users")).unwrap();
        fs::write(dir.path().join("sentinel.toml"), "").unwrap();
        let mut seen = Vec::new();
        while let Some(c) = next(&rx) {
            let done = c == changed("sentinel.toml");
            seen.push(c);
            if done {
                break;
            }
        }
        assert_eq!(
            seen,
            [changed("users/a.toml"), changed("sentinel.toml")],
            "unexpected events"
        );
    }

    #[test]
    fn directory_replaced_by_file_is_reported() {
        let (dir, _w, rx) = setup();
        let old = dir.path().join("old");
        fs::create_dir(&old).unwrap();
        fs::write(dir.path().join("s1.toml"), "").unwrap();
        assert_eq!(next(&rx), Some(changed("s1.toml")));
        // Back-to-back: both events land in one coalescing window.
        fs::remove_dir(&old).unwrap();
        fs::write(&old, "x").unwrap();
        fs::write(dir.path().join("s2.toml"), "").unwrap();
        let mut seen = Vec::new();
        while let Some(c) = next(&rx) {
            let done = c == changed("s2.toml");
            seen.push(c);
            if done {
                break;
            }
        }
        assert!(seen.contains(&changed("old")), "old not reported: {seen:?}");
        assert_eq!(seen.last(), Some(&changed("s2.toml")));
    }

    #[test]
    fn renamed_directories_are_not_reported() {
        let dir = tempfile::tempdir().unwrap();
        let (_w, rx) = Watcher::start(dir.path()).unwrap();
        // Create the directory after start and let its events settle, so the
        // rename below is the only thing said about it.
        fs::create_dir(dir.path().join("users")).unwrap();
        fs::write(dir.path().join("users/a.toml"), "").unwrap();
        fs::write(dir.path().join("s1.toml"), "").unwrap();
        while let Some(c) = next(&rx) {
            if c == changed("s1.toml") {
                break;
            }
        }
        fs::rename(dir.path().join("users"), dir.path().join("people")).unwrap();
        fs::write(dir.path().join("sentinel.toml"), "").unwrap();
        let mut seen = Vec::new();
        while let Some(c) = next(&rx) {
            let done = c == changed("sentinel.toml");
            seen.push(c);
            if done {
                break;
            }
        }
        assert_eq!(seen.last(), Some(&changed("sentinel.toml")), "{seen:?}");
        for d in ["users", "people"] {
            assert!(!seen.contains(&changed(d)), "{d} reported: {seen:?}");
        }
    }

    #[test]
    fn nested_directory_renamed_after_parent_rename_is_not_reported() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("users/deep")).unwrap();
        let (_w, rx) = Watcher::start(dir.path()).unwrap();
        fs::rename(dir.path().join("users"), dir.path().join("people")).unwrap();
        fs::write(dir.path().join("s1.toml"), "").unwrap();
        while let Some(c) = next(&rx) {
            if c == changed("s1.toml") {
                break;
            }
        }
        fs::rename(
            dir.path().join("people/deep"),
            dir.path().join("people/archive"),
        )
        .unwrap();
        fs::write(dir.path().join("s2.toml"), "").unwrap();
        let mut seen = Vec::new();
        while let Some(c) = next(&rx) {
            let done = c == changed("s2.toml");
            seen.push(c);
            if done {
                break;
            }
        }
        assert_eq!(seen, [changed("s2.toml")], "unexpected events");
    }

    #[test]
    fn known_directories_that_vanish_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("users/deep")).unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join("users/a.toml"), "").unwrap();
        let mut dirs = HashSet::new();
        scan_dirs(dir.path(), Path::new(""), &mut dirs);
        let expected: HashSet<PathBuf> = ["users", "users/deep"].map(PathBuf::from).into();
        assert_eq!(dirs, expected);

        // Renamed away: the source is gone but was a known directory.
        fs::rename(dir.path().join("users"), dir.path().join("people")).unwrap();
        assert!(skip_directory(dir.path(), Path::new("users"), &mut dirs));
        assert!(skip_directory(dir.path(), Path::new("people"), &mut dirs));
        // A file is never skipped, and a gone unknown path is not a directory.
        assert!(!skip_directory(
            dir.path(),
            Path::new("people/a.toml"),
            &mut dirs
        ));
        assert!(!skip_directory(dir.path(), Path::new("nothing"), &mut dirs));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_directories_are_not_walked() {
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir_all(outside.path().join("a/b")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("home")).unwrap();
        let mut dirs = HashSet::new();
        scan_dirs(dir.path(), Path::new(""), &mut dirs);
        assert!(dirs.is_empty(), "scan followed a symlink: {dirs:?}");
        // The link is not a directory to skip, and nothing behind it is learnt.
        assert!(!skip_directory(dir.path(), Path::new("home"), &mut dirs));
        assert!(dirs.is_empty(), "skip_directory walked a symlink: {dirs:?}");
    }

    #[test]
    fn drop_closes_channel() {
        let (_dir, w, rx) = setup();
        drop(w);
        let end = Instant::now() + DEADLINE;
        while !rx.is_closed() {
            assert!(Instant::now() < end, "channel never closed");
            std::thread::yield_now();
        }
    }
}
