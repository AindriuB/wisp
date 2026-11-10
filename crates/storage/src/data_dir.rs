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

/// Create missing directories (mode 0700 on unix). Refuses a symlinked final component.
pub fn ensure_private_dir(path: &Path) -> Result<(), StorageError> {
    if let Ok(meta) = std::fs::symlink_metadata(path)
        && meta.file_type().is_symlink()
    {
        return Err(StorageError::Invalid {
            path: path.to_path_buf(),
            key: "path".to_string(),
            message: "is a symlink".to_string(),
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
            .map_err(|e| io(path, e))
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(path).map_err(|e| io(path, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> CollectionId {
        CollectionId::parse("abcdef0123456789").unwrap()
    }

    #[test]
    fn location_rules() {
        let exe = Path::new("/app");
        let os = Path::new("/os");
        let ov = Path::new("/ov");
        assert_eq!(location(exe, true, Some(os), Some(ov)), Some("/ov".into()));
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
