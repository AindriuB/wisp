//! Atomic file writes and filesystem helpers.

use std::path::{Path, PathBuf};

use crate::{StorageError, TEMP_PREFIX};

fn io(path: &Path, source: std::io::Error) -> StorageError {
    StorageError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Read a whole file as UTF-8 text.
pub fn read_to_string(path: &Path) -> Result<String, StorageError> {
    std::fs::read_to_string(path).map_err(|e| io(path, e))
}

/// Whether `name` is a Windows reserved device name: `CON PRN AUX NUL`,
/// `COM0-9`, `LPT0-9` and the superscript forms `COM¹²³` / `LPT¹²³`. The stem
/// is the name up to the first `.`, trailing spaces removed; case is ignored
/// (ASCII only). Windows rejects these with or without an extension.
pub fn is_reserved_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    let upper = stem.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    let (head, last) = match upper.char_indices().last() {
        Some((i, c)) => (&upper[..i], c),
        None => return false,
    };
    matches!(head, "COM" | "LPT") && matches!(last, '0'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}')
}

/// `a` and `b` are the same file-system entry (Unix dev/ino, so hard links and
/// symlinks count). Always false elsewhere: std has no stable identity API on
/// Windows.
pub fn same_file_entry(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(x), Ok(y)) = (std::fs::metadata(a), std::fs::metadata(b)) {
            return x.dev() == y.dev() && x.ino() == y.ino();
        }
    }
    let _ = (a, b);
    false
}

/// `target` is the same entry as any `*.toml` file (ignoring case) in `dir`.
pub fn same_file_entry_any_toml(dir: &Path, target: &Path) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return false;
    };
    rd.flatten().any(|e| {
        let p = e.path();
        p.to_string_lossy().to_ascii_lowercase().ends_with(".toml") && same_file_entry(&p, target)
    })
}

/// Maximum symlink hops followed when resolving a target.
const MAX_HOPS: usize = 40;
/// Maximum temp-file name candidates tried before giving up.
const MAX_TEMP_ATTEMPTS: u64 = 100;

/// Resolve `path` through symlinks so the link itself is never replaced.
fn resolve(path: &Path) -> Result<PathBuf, StorageError> {
    if let Ok(p) = std::fs::canonicalize(path) {
        return Ok(p);
    }
    let mut cur = path.to_path_buf();
    for _ in 0..MAX_HOPS {
        match std::fs::symlink_metadata(&cur) {
            Ok(m) if m.file_type().is_symlink() => {
                let link = std::fs::read_link(&cur).map_err(|e| io(&cur, e))?;
                cur = match cur.parent() {
                    Some(parent) => parent.join(link),
                    None => link,
                };
            }
            _ => return Ok(cur),
        }
    }
    Err(io(
        path,
        std::io::Error::other("too many levels of symbolic links"),
    ))
}

/// Create a fresh temp file next to `target`, trying `<n>`, `<n+1>`, ... with `create_new`.
fn create_temp(target: &Path, start: u64) -> Result<(PathBuf, std::fs::File), StorageError> {
    let dir = match target.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut last = None;
    for n in start..start.saturating_add(MAX_TEMP_ATTEMPTS) {
        let tmp = dir.join(format!("{TEMP_PREFIX}{name}.{n}"));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(f) => return Ok((tmp, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some((tmp, e)),
            Err(e) => return Err(io(&tmp, e)),
        }
    }
    match last {
        Some((tmp, e)) => Err(io(&tmp, e)),
        None => Err(io(
            target,
            std::io::Error::other("no temp file name available"),
        )),
    }
}

fn write_impl(
    path: &Path,
    bytes: &[u8],
    private_new: bool,
    force_private: bool,
) -> Result<(), StorageError> {
    use std::io::Write;
    let target = resolve(path)?;
    let existing = std::fs::metadata(&target).ok();
    let (tmp, mut file) = create_temp(&target, 0)?;
    let result = (|| {
        match &existing {
            Some(m) if !force_private => file
                .set_permissions(m.permissions())
                .map_err(|e| io(&tmp, e))?,
            _ => {
                #[cfg(unix)]
                if private_new || force_private {
                    use std::os::unix::fs::PermissionsExt;
                    file.set_permissions(std::fs::Permissions::from_mode(0o600))
                        .map_err(|e| io(&tmp, e))?;
                }
                #[cfg(not(unix))]
                let _ = (private_new, force_private);
            }
        }
        file.write_all(bytes).map_err(|e| io(&tmp, e))?;
        drop(file);
        std::fs::rename(&tmp, &target).map_err(|e| io(&target, e))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Confinement check for a file a collection controls (marker, secrets file,
/// `collection.toml`): every component of `file` below `root` must be a real
/// directory (final component a regular file), none a symlink, and the result
/// must resolve inside `root`. `Ok(false)` when any component is absent,
/// `Ok(true)` when the file exists and is safe, `Invalid` on any refusal.
pub(crate) fn confined_regular_file(
    root: &Path,
    file: &Path,
    what: &str,
) -> Result<bool, StorageError> {
    let refuse = |path: &Path| StorageError::Invalid {
        path: path.to_path_buf(),
        key: "path".into(),
        message: format!(
            "{what} must be a regular file inside the collection, with no symbolic links"
        ),
    };
    let Ok(rel) = file.strip_prefix(root) else {
        return Err(refuse(file));
    };
    let count = rel.components().count();
    let mut cur = root.to_path_buf();
    for (i, comp) in rel.components().enumerate() {
        if !matches!(comp, std::path::Component::Normal(_)) {
            return Err(refuse(file));
        }
        cur.push(comp);
        match std::fs::symlink_metadata(&cur) {
            Ok(m) if m.file_type().is_symlink() => return Err(refuse(&cur)),
            Ok(m) if i + 1 < count && !m.is_dir() => return Err(refuse(&cur)),
            Ok(m) if i + 1 == count && !m.is_file() => return Err(refuse(&cur)),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(io(&cur, e)),
        }
    }
    let canon_root = std::fs::canonicalize(root).map_err(|e| io(root, e))?;
    let canon = std::fs::canonicalize(file).map_err(|e| io(file, e))?;
    if canon.starts_with(&canon_root) {
        Ok(true)
    } else {
        Err(refuse(file))
    }
}

/// Write `bytes` to a temp file beside the (symlink-resolved) target, then rename it over.
/// An existing target keeps its permissions; a new file gets default permissions.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    write_impl(path, bytes, false, false)
}

/// Like [`atomic_write`], but a newly created file gets mode `0600` on unix.
pub fn atomic_write_private(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    write_impl(path, bytes, true, false)
}

/// Like [`atomic_write_private`], but the result is `0600` on unix even when the target
/// already exists with looser permissions.
pub fn atomic_write_force_private(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    write_impl(path, bytes, true, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.toml");
        std::fs::write(&p, "old").unwrap();
        atomic_write(&p, b"new").unwrap();
        assert_eq!(read_to_string(&p).unwrap(), "new");
        let leftovers = std::fs::read_dir(dir.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(TEMP_PREFIX)
            })
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn atomic_write_creates_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("b.toml");
        atomic_write(&p, b"x").unwrap();
        assert_eq!(read_to_string(&p).unwrap(), "x");
    }

    #[test]
    fn missing_file_is_io_with_path() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nope.toml");
        match read_to_string(&p) {
            Err(StorageError::Io { path, .. }) => assert_eq!(path, p),
            other => panic!("unexpected {other:?}"),
        }
    }

    fn temp_count(dir: &Path) -> usize {
        std::fs::read_dir(dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(TEMP_PREFIX)
            })
            .count()
    }

    #[cfg(unix)]
    #[test]
    fn keeps_existing_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("s.toml");
        std::fs::write(&p, "old").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        atomic_write(&p, b"new").unwrap();
        assert_eq!(
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(read_to_string(&p).unwrap(), "new");
    }

    #[cfg(unix)]
    #[test]
    fn writes_through_symlink() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let real = b.path().join("real.toml");
        std::fs::write(&real, "old").unwrap();
        let link = a.path().join("link.toml");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let before = std::fs::read_link(&link).unwrap();
        atomic_write(&link, b"new").unwrap();
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_link(&link).unwrap(), before);
        assert_eq!(read_to_string(&real).unwrap(), "new");
        assert_eq!(temp_count(a.path()), 0);
        assert_eq!(temp_count(b.path()), 0);
    }

    #[cfg(unix)]
    #[test]
    fn writes_through_dangling_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.toml");
        let link = dir.path().join("link.toml");
        std::os::unix::fs::symlink("real.toml", &link).unwrap();
        atomic_write(&link, b"new").unwrap();
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(read_to_string(&real).unwrap(), "new");
    }

    #[cfg(unix)]
    #[test]
    fn private_new_file_is_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.secrets.toml");
        atomic_write_private(&p, b"s").unwrap();
        assert_eq!(
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn temp_collision_picks_next_and_keeps_existing() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("t.toml");
        let planted = dir.path().join(format!("{TEMP_PREFIX}t.toml.3"));
        std::fs::write(&planted, "planted").unwrap();
        let (tmp, _f) = create_temp(&target, 3).unwrap();
        assert_eq!(tmp, dir.path().join(format!("{TEMP_PREFIX}t.toml.4")));
        assert_eq!(std::fs::read_to_string(&planted).unwrap(), "planted");
        atomic_write(&target, b"ok").unwrap();
        assert_eq!(read_to_string(&target).unwrap(), "ok");
        assert_eq!(std::fs::read_to_string(&planted).unwrap(), "planted");
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_does_not_follow_planted_temp_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("t.toml");
        let victim = dir.path().join("victim");
        std::fs::write(&victim, "victim").unwrap();
        let planted = dir.path().join(format!("{TEMP_PREFIX}t.toml.0"));
        std::os::unix::fs::symlink(&victim, &planted).unwrap();
        atomic_write(&target, b"new").unwrap();
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "victim");
        assert!(
            !std::fs::symlink_metadata(&target)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
    }

    #[test]
    fn fs_rs_uses_no_file_create() {
        let src = include_str!("fs.rs");
        let needle = ["File", "::create"].concat();
        let body = src.split("#[cfg(test)]").next().unwrap();
        assert!(!body.contains(&needle));
    }
}
