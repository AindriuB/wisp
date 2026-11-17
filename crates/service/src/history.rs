//! History recording: one append for a collection whose id the caller already holds.

use wisp_storage::collection::CollectionId;
use wisp_storage::data_dir::DataDir;
use wisp_storage::history::{HistoryStore, NewEntry};

/// Whether the response body goes to the history entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryBodies {
    Store,
    Skip,
}

impl HistoryBodies {
    /// From the "store response bodies" setting.
    pub fn from_setting(enabled: bool) -> Self {
        if enabled { Self::Store } else { Self::Skip }
    }
}

/// Why an entry was not recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordError {
    /// The store could not be opened: history is off for the collection.
    Open(String),
    /// The append failed: this one entry is dropped.
    Append(String),
}

/// Append `entry` to the history of collection `id`, returning the store. The id is the one
/// the caller holds: it is never re-read from disk. Blocking; never on the UI thread.
pub fn record(
    data: &DataDir,
    id: &CollectionId,
    entry: NewEntry,
    body: Option<Vec<u8>>,
    bodies: HistoryBodies,
) -> Result<HistoryStore, RecordError> {
    let store = HistoryStore::open(data, id);
    let body = match bodies {
        HistoryBodies::Store => body,
        HistoryBodies::Skip => None,
    };
    store
        .append(entry, body.as_deref())
        .map(|_| store)
        .map_err(|e| RecordError::Append(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_core::request::Request;
    use wisp_storage::history::Outcome;

    fn entry() -> NewEntry {
        NewEntry {
            at: std::time::SystemTime::UNIX_EPOCH,
            request_path: None,
            request: Request::default(),
            outcome: Outcome::Completed,
            status: Some(200),
            ttfb: None,
            total: None,
            dns: None,
            connect: None,
            size: None,
            response_headers: Vec::new(),
            environment: None,
            assertions: Vec::new(),
            delivery: None,
        }
    }

    #[test]
    fn record_appends_under_the_given_id_and_honours_the_body_setting() {
        let dir = tempfile::tempdir().unwrap();
        let data = DataDir::at(dir.path());
        let id = CollectionId::parse("abcdef0123456789").unwrap();
        let store = record(
            &data,
            &id,
            entry(),
            Some(b"hi".to_vec()),
            HistoryBodies::Skip,
        )
        .unwrap();
        let listed = store.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert!(!listed[0].body_stored);
        let store = record(
            &data,
            &id,
            entry(),
            Some(b"hi".to_vec()),
            HistoryBodies::Store,
        )
        .unwrap();
        assert!(store.list().unwrap().iter().any(|e| e.body_stored));
    }
}
