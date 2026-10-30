//! Request history log.
//!
//! One JSON line per finished request in `<root>/.wisp/history.jsonl`. Entries
//! hold no header values, auth fields or environment values, and the URL is the
//! unresolved template.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use wisp_core::{ErrorKind, Method};

use crate::{STATE_DIR, StorageError};

/// Maximum size in bytes of a stored response body preview.
pub const PREVIEW_LIMIT: usize = 1024;
/// Size above which the log is rotated before an append.
const ROTATE_AT: u64 = 4 * 1024 * 1024;
const LOG_FILE: &str = "history.jsonl";
const ROTATED_FILE: &str = "history.1.jsonl";

/// How a request ended. Carries no error message, since those embed resolved URLs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Outcome {
    Completed,
    Failed { kind: ErrorKind },
    Cancelled,
}

/// One finished request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub at: SystemTime,
    /// Request file path relative to the collection root.
    pub request: Option<String>,
    pub method: Method,
    /// The URL template before resolution.
    pub url: String,
    pub outcome: Outcome,
    pub status: Option<u16>,
    pub ttfb: Option<Duration>,
    pub total: Option<Duration>,
    pub size: Option<u64>,
    pub preview: String,
}

impl HistoryEntry {
    /// Lossy-UTF-8 preview of `bytes`, cut on a char boundary to `PREVIEW_LIMIT` bytes.
    pub fn preview_from(bytes: &[u8]) -> String {
        let mut s = String::from_utf8_lossy(&bytes[..bytes.len().min(PREVIEW_LIMIT)]).into_owned();
        if s.len() > PREVIEW_LIMIT {
            let mut end = PREVIEW_LIMIT;
            while !s.is_char_boundary(end) {
                end -= 1;
            }
            s.truncate(end);
        }
        s
    }
}

/// True when the file exists, is non-empty and does not end in a newline.
fn ends_mid_line(path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = fs::File::open(path) else {
        return false;
    };
    let mut last = [0u8; 1];
    f.seek(SeekFrom::End(-1)).is_ok() && f.read_exact(&mut last).is_ok() && last[0] != b'\n'
}

/// Append-only history log inside a collection.
#[derive(Debug, Clone)]
pub struct HistoryLog {
    dir: PathBuf,
}

impl HistoryLog {
    /// Does no I/O.
    pub fn open(root: impl AsRef<Path>) -> HistoryLog {
        HistoryLog {
            dir: root.as_ref().join(STATE_DIR),
        }
    }

    fn io(path: &Path, source: std::io::Error) -> StorageError {
        StorageError::Io {
            path: path.to_path_buf(),
            source,
        }
    }

    pub fn append(&self, entry: &HistoryEntry) -> Result<(), StorageError> {
        fs::create_dir_all(&self.dir).map_err(|e| Self::io(&self.dir, e))?;
        let ignore = self.dir.join(".gitignore");
        if !ignore.exists() {
            fs::write(&ignore, "*\n").map_err(|e| Self::io(&ignore, e))?;
        }
        let log = self.dir.join(LOG_FILE);
        if fs::metadata(&log).is_ok_and(|m| m.len() > ROTATE_AT) {
            let rotated = self.dir.join(ROTATED_FILE);
            fs::rename(&log, &rotated).map_err(|e| Self::io(&rotated, e))?;
        }
        let mut line = serde_json::to_string(entry).map_err(|e| StorageError::History {
            message: e.to_string(),
        })?;
        line.push('\n');
        if ends_mid_line(&log) {
            line.insert(0, '\n');
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .map_err(|e| Self::io(&log, e))?;
        f.write_all(line.as_bytes()).map_err(|e| Self::io(&log, e))
    }

    /// Reads the current log, skipping malformed lines.
    pub fn read_all(&self) -> Result<Vec<HistoryEntry>, StorageError> {
        let log = self.dir.join(LOG_FILE);
        let bytes = match fs::read(&log) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(Self::io(&log, e)),
        };
        Ok(bytes
            .split(|&b| b == b'\n')
            .filter_map(|l| serde_json::from_slice(l).ok())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(url: &str) -> HistoryEntry {
        HistoryEntry {
            at: SystemTime::UNIX_EPOCH + Duration::from_secs(10),
            request: Some("a.toml".into()),
            method: Method::Get,
            url: url.into(),
            outcome: Outcome::Completed,
            status: Some(200),
            ttfb: Some(Duration::from_millis(5)),
            total: Some(Duration::from_millis(9)),
            size: Some(3),
            preview: "abc".into(),
        }
    }

    #[test]
    fn appends_entries_in_order() {
        let d = tempfile::tempdir().unwrap();
        let log = HistoryLog::open(d.path());
        log.append(&entry("/1")).unwrap();
        log.append(&entry("/2")).unwrap();
        let all = log.read_all().unwrap();
        assert_eq!(all, vec![entry("/1"), entry("/2")]);
    }

    #[test]
    fn append_writes_a_gitignore_that_hides_state() {
        let d = tempfile::tempdir().unwrap();
        let log = HistoryLog::open(d.path());
        log.append(&entry("/1")).unwrap();
        let g = fs::read_to_string(d.path().join(".wisp/.gitignore")).unwrap();
        assert!(g.contains('*'));
    }

    #[test]
    fn open_does_no_io() {
        let d = tempfile::tempdir().unwrap();
        let log = HistoryLog::open(d.path());
        assert!(log.read_all().unwrap().is_empty());
        assert_eq!(fs::read_dir(d.path()).unwrap().count(), 0);
    }

    #[test]
    fn preview_is_bounded_and_valid() {
        let bytes = "é".repeat(5000).into_bytes();
        let p = HistoryEntry::preview_from(&bytes);
        assert!(p.len() <= PREVIEW_LIMIT);
        assert!(!p.is_empty());
    }

    #[test]
    fn preview_cuts_on_a_char_boundary_for_three_byte_chars() {
        let bytes = "€".repeat(500).into_bytes();
        let p = HistoryEntry::preview_from(&bytes);
        assert_eq!(p.len(), 1023);
    }

    #[test]
    fn read_all_skips_invalid_utf8_lines() {
        let d = tempfile::tempdir().unwrap();
        let log = HistoryLog::open(d.path());
        log.append(&entry("/1")).unwrap();
        let path = d.path().join(".wisp/history.jsonl");
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(b"\xff\xfe\n").unwrap();
        log.append(&entry("/2")).unwrap();
        assert_eq!(log.read_all().unwrap().len(), 2);
    }

    #[test]
    fn append_after_a_torn_line_keeps_the_new_entry() {
        let d = tempfile::tempdir().unwrap();
        let log = HistoryLog::open(d.path());
        log.append(&entry("/1")).unwrap();
        let path = d.path().join(".wisp/history.jsonl");
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(b"{\"at\":").unwrap();
        log.append(&entry("/2")).unwrap();
        assert_eq!(log.read_all().unwrap(), vec![entry("/1"), entry("/2")]);
    }

    #[test]
    fn stores_the_url_template_unresolved() {
        let d = tempfile::tempdir().unwrap();
        let log = HistoryLog::open(d.path());
        let url = "{{baseUrl}}/x?k={{token}}";
        log.append(&entry(url)).unwrap();
        let raw = fs::read_to_string(d.path().join(".wisp/history.jsonl")).unwrap();
        assert!(raw.contains(url));
        assert_eq!(log.read_all().unwrap()[0].url, url);
    }

    #[test]
    fn rotates_the_log_once_it_passes_the_size_limit() {
        let d = tempfile::tempdir().unwrap();
        let state = d.path().join(".wisp");
        fs::create_dir_all(&state).unwrap();
        fs::write(
            state.join("history.jsonl"),
            vec![b'x'; (ROTATE_AT + 1) as usize],
        )
        .unwrap();
        let log = HistoryLog::open(d.path());
        log.append(&entry("/1")).unwrap();
        assert!(state.join("history.1.jsonl").exists());
        let text = fs::read_to_string(state.join("history.jsonl")).unwrap();
        assert_eq!(text.lines().count(), 1);
    }

    #[test]
    fn skips_malformed_lines_when_reading() {
        let d = tempfile::tempdir().unwrap();
        let log = HistoryLog::open(d.path());
        log.append(&entry("/1")).unwrap();
        let path = d.path().join(".wisp/history.jsonl");
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(b"not json\n").unwrap();
        log.append(&entry("/2")).unwrap();
        assert_eq!(log.read_all().unwrap().len(), 2);
    }
}
