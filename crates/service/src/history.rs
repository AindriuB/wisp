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
    append_to(&store, entry, body, bodies)?;
    Ok(store)
}

/// Append to an open store. With `Skip` the body is dropped and recorded as `off`; with
/// `Store`, a missing body on a completed response is recorded as `unreadable`.
pub fn append_to(
    store: &HistoryStore,
    entry: NewEntry,
    body: Option<Vec<u8>>,
    bodies: HistoryBodies,
) -> Result<(), RecordError> {
    let off = bodies == HistoryBodies::Skip;
    let body = if off { None } else { body };
    store
        .append_with(entry, body.as_deref(), off)
        .map(|_| ())
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

    use wisp_storage::history::{BodyEvidence, Evidence};

    fn recorded(
        outcome: Outcome,
        status: Option<u16>,
        body: Option<Vec<u8>>,
        bodies: HistoryBodies,
    ) -> Evidence {
        let dir = tempfile::tempdir().unwrap();
        let data = DataDir::at(dir.path());
        let id = CollectionId::parse("abcdef0123456789").unwrap();
        let mut e = entry();
        e.outcome = outcome;
        e.status = status;
        let store = record(&data, &id, e, body, bodies).unwrap();
        store.list().unwrap()[0].evidence.unwrap()
    }

    fn failed() -> Outcome {
        Outcome::Failed {
            kind: wisp_core::ErrorKind::Connect,
        }
    }

    #[test]
    fn evidence_setting_off_completed_is_off() {
        let ev = recorded(
            Outcome::Completed,
            Some(200),
            Some(b"hi".to_vec()),
            HistoryBodies::Skip,
        );
        assert_eq!(
            ev,
            Evidence {
                body: BodyEvidence::Off,
                headers: true
            }
        );
    }

    #[test]
    fn evidence_setting_on_readable_body_is_stored() {
        let ev = recorded(
            Outcome::Completed,
            Some(200),
            Some(b"hi".to_vec()),
            HistoryBodies::Store,
        );
        assert_eq!(ev.body, BodyEvidence::Stored);
    }

    #[test]
    fn evidence_setting_on_oversized_body_is_truncated() {
        let big = vec![0u8; wisp_storage::history::BODY_CAP + 1];
        let ev = recorded(
            Outcome::Completed,
            Some(200),
            Some(big),
            HistoryBodies::Store,
        );
        assert_eq!(ev.body, BodyEvidence::Truncated);
    }

    #[test]
    fn evidence_setting_on_unreadable_body_is_unreadable() {
        let ev = recorded(Outcome::Completed, Some(200), None, HistoryBodies::Store);
        assert_eq!(ev.body, BodyEvidence::Unreadable);
    }

    #[test]
    fn evidence_failed_and_cancelled_are_no_response() {
        let ev = recorded(failed(), None, None, HistoryBodies::Store);
        assert_eq!(
            ev,
            Evidence {
                body: BodyEvidence::NoResponse,
                headers: false
            }
        );
        let ev = recorded(Outcome::Cancelled, None, None, HistoryBodies::Store);
        assert_eq!(ev.body, BodyEvidence::NoResponse);
    }

    #[test]
    fn evidence_run_completed_is_off() {
        let ev = recorded(Outcome::Completed, Some(200), None, HistoryBodies::Skip);
        assert_eq!(ev.body, BodyEvidence::Off);
    }

    #[test]
    fn evidence_run_failed_is_no_response() {
        let ev = recorded(failed(), None, None, HistoryBodies::Skip);
        assert_eq!(ev.body, BodyEvidence::NoResponse);
    }
}
