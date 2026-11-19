//! Data directory resolution for the history store and app state.

use std::path::{Path, PathBuf};

use crate::StorageError;
use crate::collection::CollectionId;

/// Env var that overrides the data dir (absolute paths only). Used by tests and agents.
pub const OVERRIDE_ENV: &str = "WISP_DATA_DIR";

/// Where per-user data lives: the override, else beside the executable when portable,
/// else in the OS data dir.
pub fn location(
    exe_dir: &Path,
    portable_marker: bool,
    os_data_dir: Option<&Path>,
    override_dir: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(o) = override_dir.filter(|o| o.is_absolute()) {
        return Some(o.to_path_buf());
    }
    if portable_marker {
        return Some(exe_dir.join("data"));
    }
    os_data_dir.map(|d| d.join("wisp"))
}

fn os_data_dir() -> Option<PathBuf> {
    let var = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library").join("Application Support"))
    } else if cfg!(windows) {
        var("LOCALAPPDATA")
    } else {
        var("XDG_DATA_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| var("HOME").map(|h| h.join(".local").join("share")))
    }
}

/// The per-user data root. Creates nothing until a caller writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDir {
    root: PathBuf,
}

impl DataDir {
    /// Resolve from the executable location, the `portable` marker and the environment.
    pub fn resolve() -> Option<DataDir> {
        let exe = std::env::current_exe().ok()?;
        let exe_dir = exe.parent()?;
        let portable = exe_dir.join("portable").exists();
        let over = std::env::var_os(OVERRIDE_ENV)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        location(exe_dir, portable, os_data_dir().as_deref(), over.as_deref())
            .map(|root| DataDir { root })
    }

    pub fn at(root: impl Into<PathBuf>) -> DataDir {
        DataDir { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn history_dir(&self, id: &CollectionId) -> PathBuf {
        self.root.join("history").join(id.as_str())
    }

    pub fn cookies_file(&self, id: &CollectionId) -> PathBuf {
        self.root
            .join("cookies")
            .join(format!("{}.json", id.as_str()))
    }
}

fn io(path: &Path, source: std::io::Error) -> StorageError {
    StorageError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Create missing directories (mode 0700 on unix; owner-only protected DACL on Windows).
/// Refuses a symlinked final component, with or without a trailing separator. An existing
/// directory wider than 0700 (unix) or without the owner-only DACL (Windows) is tightened.
pub fn ensure_private_dir(path: &Path) -> Result<(), StorageError> {
    let path = strip_trailing_separators(path);
    let path = path.as_path();
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(StorageError::Invalid {
                path: path.to_path_buf(),
                key: "path".to_string(),
                message: "is a symlink".to_string(),
            });
        }
        Ok(meta) if !meta.is_dir() => {
            return Err(StorageError::Invalid {
                path: path.to_path_buf(),
                key: "path".to_string(),
                message: "is not a directory".to_string(),
            });
        }
        _ => {}
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
            .map_err(|e| io(path, e))?;
        let meta = std::fs::metadata(path).map_err(|e| io(path, e))?;
        if meta.permissions().mode() & 0o777 != 0o700 {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| io(path, e))?;
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        if acl_policy(crate::win_acl::volume_flags(path)) == AclPolicy::Skip {
            warn_no_acl_volume_once();
            return std::fs::create_dir_all(path).map_err(|e| io(path, e));
        }
        // Create each missing level top-down and restrict every directory this call creates,
        // so no new ancestor keeps inherited ACLs. Directories that already existed are left
        // alone (only `path` itself is tightened below). A level another process creates
        // first (AlreadyExists) is not ours, so it is likewise not touched: the conservative
        // choice, which cannot loosen or break anyone else's ACL.
        let mut missing = Vec::new();
        let mut cur = Some(path);
        while let Some(p) = cur {
            match std::fs::symlink_metadata(p) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    missing.push(p);
                    cur = p.parent().filter(|q| !q.as_os_str().is_empty());
                }
                _ => break,
            }
        }
        for p in missing.into_iter().rev() {
            match std::fs::create_dir(p) {
                Ok(()) => crate::win_acl::restrict_to_owner(p).map_err(|e| io(p, e))?,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(io(p, e)),
            }
        }
        // Only rewrite the DACL when it is not already owner-only: setting it propagates to
        // every child, and this runs on hot paths.
        if !crate::win_acl::is_owner_only(path).map_err(|e| io(path, e))? {
            crate::win_acl::restrict_to_owner(path).map_err(|e| io(path, e))?;
        }
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    {
        std::fs::create_dir_all(path).map_err(|e| io(path, e))
    }
}

/// Whether to set an owner-only DACL on a directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) enum AclPolicy {
    Apply,
    Skip,
}

/// `FILE_PERSISTENT_ACLS` from `GetVolumeInformationW`'s flags word.
#[cfg_attr(not(windows), allow(dead_code))]
const FILE_PERSISTENT_ACLS: u32 = 0x8;

/// Skip only on an explicit "no persistent ACLs" answer; a failed query (`None`) applies the
/// DACL, so its failure stays an error.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn acl_policy(volume_flags: Option<u32>) -> AclPolicy {
    match volume_flags {
        Some(f) if f & FILE_PERSISTENT_ACLS == 0 => AclPolicy::Skip,
        _ => AclPolicy::Apply,
    }
}

#[cfg(windows)]
fn warn_no_acl_volume_once() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        tracing::warn!(
            "data directory is on a volume without ACL support (FAT/exFAT); \
             owner-only permissions are not applied"
        );
    });
}

/// `a/b/` -> `a/b`; a root such as `/` is returned unchanged.
fn strip_trailing_separators(path: &Path) -> PathBuf {
    let s = path.as_os_str().to_string_lossy();
    let t = s.trim_end_matches(['/', '\\']);
    if t.is_empty() || t.len() == s.len() || t.ends_with(':') {
        return path.to_path_buf();
    }
    PathBuf::from(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> CollectionId {
        CollectionId::parse("abcdef0123456789").unwrap()
    }

    #[test]
    fn no_acl_volume_flags_without_persistent_acls_skip() {
        assert_eq!(acl_policy(Some(0x0000_0003)), AclPolicy::Skip);
        assert_eq!(acl_policy(Some(0)), AclPolicy::Skip);
    }

    #[test]
    fn no_acl_volume_flags_with_persistent_acls_apply() {
        assert_eq!(acl_policy(Some(0x0000_0008 | 0x3)), AclPolicy::Apply);
    }

    #[test]
    fn no_acl_volume_failed_query_applies() {
        assert_eq!(acl_policy(None), AclPolicy::Apply);
    }

    #[test]
    fn location_rules() {
        let exe = Path::new("/app");
        let os = Path::new("/os");
        // `/ov` has no drive letter, so it is not absolute on Windows.
        let ov = std::env::temp_dir().join("ov");
        assert_eq!(location(exe, true, Some(os), Some(&ov)), Some(ov.clone()));
        assert_eq!(
            location(exe, false, Some(os), Some(Path::new("rel"))),
            Some("/os/wisp".into())
        );
        assert_eq!(
            location(exe, true, Some(os), None),
            Some("/app/data".into())
        );
        assert_eq!(
            location(exe, false, Some(os), None),
            Some("/os/wisp".into())
        );
        assert_eq!(location(exe, false, None, None), None);
    }

    #[test]
    fn paths_for_id() {
        let d = DataDir::at("/r");
        assert_eq!(d.root(), Path::new("/r"));
        assert_eq!(
            d.history_dir(&id()),
            Path::new("/r/history/abcdef0123456789")
        );
        assert_eq!(
            d.cookies_file(&id()),
            Path::new("/r/cookies/abcdef0123456789.json")
        );
    }

    #[cfg(windows)]
    #[test]
    fn private_dir_restricts_every_created_ancestor_on_windows() {
        use crate::win_acl::is_owner_only;
        let t = tempfile::tempdir().unwrap();
        let before = is_owner_only(t.path()).unwrap();
        ensure_private_dir(&t.path().join("a/b/c")).unwrap();
        for d in ["a", "a/b", "a/b/c"] {
            assert!(is_owner_only(&t.path().join(d)).unwrap(), "{d}");
        }
        assert_eq!(is_owner_only(t.path()).unwrap(), before);
    }

    #[cfg(unix)]
    #[test]
    fn private_dir_mode_and_symlink() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("a/b/c");
        ensure_private_dir(&p).unwrap();
        for d in ["a", "a/b", "a/b/c"] {
            let m = std::fs::metadata(t.path().join(d))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(m & 0o777, 0o700, "{d}");
        }
        ensure_private_dir(&p).unwrap();
        let link = t.path().join("link");
        symlink(&p, &link).unwrap();
        assert!(matches!(
            ensure_private_dir(&link),
            Err(StorageError::Invalid { .. })
        ));
    }
}
