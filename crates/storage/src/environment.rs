//! Environment and secrets files.
//!
//! Plain vars live in `environments/<name>.toml`, secret vars in
//! `environments/<name>.secrets.toml`; both use a `[vars]` table.

use std::path::{Path, PathBuf};

use wisp_core::request::Environment;

use crate::{StorageError, codec, fs};

const DIR: &str = "environments";
const SECRETS_SUFFIX: &str = ".secrets.toml";
const IGNORE_LINE: &str = "*.secrets.toml";

fn plain_path(root: &Path, name: &str) -> PathBuf {
    root.join(DIR).join(format!("{name}.toml"))
}

fn secrets_path(root: &Path, name: &str) -> PathBuf {
    root.join(DIR).join(format!("{name}{SECRETS_SUFFIX}"))
}

fn valid_name(name: &str) -> bool {
    let chars_ok = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if name.is_empty() || !chars_ok || name.starts_with('.') || name.ends_with('.') {
        return false;
    }
    if name.contains("..") || name.to_ascii_lowercase().ends_with(".secrets") {
        return false;
    }
    // Windows reserved device names, with or without an extension.
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    !reserved
}

fn check_name(root: &Path, name: &str) -> Result<(), StorageError> {
    if !valid_name(name) {
        return Err(StorageError::Invalid {
            path: root.join(DIR),
            key: "name".into(),
            message: "invalid environment name".into(),
        });
    }
    Ok(())
}

fn io(path: &Path, source: std::io::Error) -> StorageError {
    StorageError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Names of the environments under `environments/`, sorted.
pub fn list_environments(root: &Path) -> Result<Vec<String>, StorageError> {
    let dir = root.join(DIR);
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io(&dir, e)),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| io(&dir, e))?;
        let file = entry.file_name().to_string_lossy().into_owned();
        if file.ends_with(SECRETS_SUFFIX) || file.starts_with('.') {
            continue;
        }
        let is_file = entry.file_type().map_err(|e| io(&dir, e))?.is_file();
        if let Some(name) = file.strip_suffix(".toml")
            && is_file
            && valid_name(name)
        {
            names.push(name.to_string());
        }
    }
    names.sort();
    Ok(names)
}

fn load_vars(path: &Path) -> Result<Vec<wisp_core::request::Var>, StorageError> {
    let text = fs::read_to_string(path)?;
    let doc = codec::parse(path, &text)?;
    codec::read_vars(doc.as_table(), "vars", path)
}

/// Load an environment: vars from `<name>.toml`, secrets from `<name>.secrets.toml`.
pub fn load_environment(root: &Path, name: &str) -> Result<Environment, StorageError> {
    check_name(root, name)?;
    let vars = load_vars(&plain_path(root, name))?;
    let sp = secrets_path(root, name);
    let secrets = if sp.exists() {
        load_vars(&sp)?
    } else {
        Vec::new()
    };
    tracing::debug!(
        environment = name,
        vars = vars.len(),
        secrets = secrets.len(),
        "loaded environment"
    );
    Ok(Environment {
        name: name.to_string(),
        vars,
        secrets,
    })
}

/// Edit (or create) a file's `[vars]` table in place and return the new text.
fn render(path: &Path, vars: &[wisp_core::request::Var]) -> Result<String, StorageError> {
    let mut doc = if path.exists() {
        codec::parse(path, &fs::read_to_string(path)?)?
    } else {
        toml_edit::DocumentMut::new()
    };
    let table = doc.as_table_mut();
    if table.get("vars").is_none() {
        // Always emit `[vars]` for a new file, even when empty.
        table.insert("vars", toml_edit::Item::Table(toml_edit::Table::new()));
    }
    codec::write_vars(table, "vars", vars);
    Ok(doc.to_string())
}

fn ensure_gitignore(dir: &Path) -> Result<(), StorageError> {
    let path = dir.join(".gitignore");
    if !path.exists() {
        return fs::atomic_write(&path, format!("{IGNORE_LINE}\n").as_bytes());
    }
    let mut text = fs::read_to_string(&path)?;
    if text.lines().any(|l| l.trim() == IGNORE_LINE) {
        return Ok(());
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(IGNORE_LINE);
    text.push('\n');
    fs::atomic_write(&path, text.as_bytes())
}

/// Save an environment, editing both files in place.
pub fn save_environment(root: &Path, env: &Environment) -> Result<(), StorageError> {
    check_name(root, &env.name)?;
    let dir = root.join(DIR);
    std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;

    let pp = plain_path(root, &env.name);
    let sp = secrets_path(root, &env.name);
    let plain_text = render(&pp, &env.vars)?;
    let secrets_text = if !env.secrets.is_empty() || sp.exists() {
        Some(render(&sp, &env.secrets)?)
    } else {
        None
    };

    fs::atomic_write(&pp, plain_text.as_bytes())?;
    if let Some(text) = secrets_text {
        ensure_gitignore(&dir)?;
        fs::atomic_write_private(&sp, text.as_bytes())?;
    }
    let names: Vec<&str> = env
        .vars
        .iter()
        .chain(&env.secrets)
        .map(|v| v.name.as_str())
        .collect();
    tracing::debug!(environment = %env.name, ?names, "saved environment");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_core::request::Var;

    fn var(n: &str, v: &str) -> Var {
        Var {
            name: n.into(),
            value: v.into(),
        }
    }

    fn write(root: &Path, file: &str, text: &str) {
        let d = root.join(DIR);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(file), text).unwrap();
    }

    fn read(root: &Path, file: &str) -> String {
        std::fs::read_to_string(root.join(DIR).join(file)).unwrap()
    }

    #[test]
    fn loads_vars_and_secrets() {
        let t = tempfile::tempdir().unwrap();
        write(t.path(), "dev.toml", "[vars]\nhost = \"a\"\n");
        write(t.path(), "dev.secrets.toml", "[vars]\ntoken = \"s\"\n");
        let env = load_environment(t.path(), "dev").unwrap();
        assert_eq!(env.vars, vec![var("host", "a")]);
        assert_eq!(env.secrets, vec![var("token", "s")]);
    }

    #[test]
    fn lists_sorted_without_secrets() {
        let t = tempfile::tempdir().unwrap();
        assert!(list_environments(t.path()).unwrap().is_empty());
        write(t.path(), "prod.toml", "");
        write(t.path(), "dev.secrets.toml", "");
        write(t.path(), "dev.toml", "");
        assert_eq!(list_environments(t.path()).unwrap(), ["dev", "prod"]);
    }

    #[test]
    fn gitignore_created_and_appended_once() {
        let t = tempfile::tempdir().unwrap();
        write(t.path(), ".gitignore", "foo\n");
        let env = Environment {
            name: "dev".into(),
            vars: vec![],
            secrets: vec![var("token", "s")],
        };
        save_environment(t.path(), &env).unwrap();
        save_environment(t.path(), &env).unwrap();
        assert_eq!(read(t.path(), ".gitignore"), "foo\n*.secrets.toml\n");

        let t2 = tempfile::tempdir().unwrap();
        save_environment(t2.path(), &env).unwrap();
        assert_eq!(read(t2.path(), ".gitignore"), "*.secrets.toml\n");
    }

    #[test]
    fn moving_var_to_secret_removes_from_plain() {
        let t = tempfile::tempdir().unwrap();
        write(
            t.path(),
            "dev.toml",
            "[vars]\nhost = \"a\"\ntoken = \"s\"\n",
        );
        let env = Environment {
            name: "dev".into(),
            vars: vec![var("host", "a")],
            secrets: vec![var("token", "s")],
        };
        save_environment(t.path(), &env).unwrap();
        assert!(!read(t.path(), "dev.toml").contains("token"));
        assert!(read(t.path(), "dev.secrets.toml").contains("token = \"s\""));
        let back = load_environment(t.path(), "dev").unwrap();
        assert_eq!(back, env);
    }

    #[test]
    fn edit_preserves_comments() {
        let t = tempfile::tempdir().unwrap();
        let src = "# top\n[vars]\n# the host\nhost = \"a\"\nport = \"1\" # port\n";
        write(t.path(), "dev.toml", src);
        let env = Environment {
            name: "dev".into(),
            vars: vec![var("host", "b"), var("port", "1")],
            secrets: vec![],
        };
        save_environment(t.path(), &env).unwrap();
        let out = read(t.path(), "dev.toml");
        let diff = src.lines().zip(out.lines()).filter(|(a, b)| a != b).count();
        assert_eq!(src.lines().count(), out.lines().count());
        assert_eq!(diff, 1);
    }

    #[test]
    fn no_secrets_file_when_unneeded() {
        let t = tempfile::tempdir().unwrap();
        let env = Environment {
            name: "dev".into(),
            vars: vec![var("a", "b")],
            secrets: vec![],
        };
        save_environment(t.path(), &env).unwrap();
        assert!(!t.path().join(DIR).join("dev.secrets.toml").exists());
        assert!(!t.path().join(DIR).join(".gitignore").exists());
    }

    #[cfg(unix)]
    #[test]
    fn secrets_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let env = Environment {
            name: "dev".into(),
            vars: vec![],
            secrets: vec![var("k", "v")],
        };
        save_environment(t.path(), &env).unwrap();
        let mode = std::fs::metadata(t.path().join(DIR).join("dev.secrets.toml"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn rejects_secrets_suffix_name_without_clobbering() {
        let t = tempfile::tempdir().unwrap();
        let dev = Environment {
            name: "dev".into(),
            vars: vec![var("host", "a")],
            secrets: vec![var("token", "s3cret")],
        };
        save_environment(t.path(), &dev).unwrap();
        for name in ["dev.secrets", "dev.SECRETS"] {
            let evil = Environment {
                name: name.into(),
                vars: vec![var("x", "y")],
                secrets: vec![],
            };
            assert!(save_environment(t.path(), &evil).is_err());
            assert!(load_environment(t.path(), name).is_err());
        }
        let back = load_environment(t.path(), "dev").unwrap();
        assert_eq!(back.secrets, vec![var("token", "s3cret")]);
    }

    #[test]
    fn rejects_unsafe_names() {
        let t = tempfile::tempdir().unwrap();
        for name in [
            "C:evil", "dev:x", "a\\b", "a/b", "a..b", ".x", "x.", "", "con", "NUL.txt", "com1",
            "a\u{1}b", "a b",
        ] {
            assert!(load_environment(t.path(), name).is_err(), "{name:?}");
        }
        assert!(check_name(t.path(), "dev-1_a.b").is_ok());
    }

    #[test]
    fn list_skips_dirs_and_invalid_names() {
        let t = tempfile::tempdir().unwrap();
        write(t.path(), "ok.toml", "");
        write(t.path(), "a..b.toml", "");
        std::fs::create_dir_all(t.path().join(DIR).join("x.toml")).unwrap();
        assert_eq!(list_environments(t.path()).unwrap(), ["ok"]);
    }
}
