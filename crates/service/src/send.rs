//! The one place outside `wisp-core` that builds `Command::Send`.

use std::path::PathBuf;

use wisp_core::request::Request;
use wisp_core::{Command, Engine, RequestId};

use crate::plan::{SendPlan, recorded_request};

/// What was sent, kept for the history entry written at the terminal event.
pub struct SentRecord {
    /// The unresolved draft; the store redacts it.
    pub draft: Request,
    pub request: Option<String>,
    /// The root of the sending tab's collection; the history entry goes there.
    pub root: Option<PathBuf>,
    /// The environment the request was resolved under, by name.
    pub environment: Option<String>,
}

/// A submitted send: the engine's id and the record to keep for it.
pub struct Submitted {
    pub id: RequestId,
    pub record: SentRecord,
}

/// Submit a planned send under `partition` (the cookie partition, the collection id).
pub fn submit(
    engine: &Engine,
    mut plan: SendPlan,
    draft: Request,
    partition: Option<String>,
    root: Option<PathBuf>,
) -> Submitted {
    plan.prepared.client.collection = partition;
    let scope_path = plan.scope_path;
    let environment = plan.prepared.environment.clone();
    let id = engine.submit(Command::Send(plan.prepared));
    Submitted {
        id,
        record: SentRecord {
            request: recorded_request(scope_path.as_deref()),
            draft,
            root,
            environment,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    use wisp_core::request::{CollectionDefaults, KeyValue};
    use wisp_core::resolve::Recording;
    use wisp_core::{EngineConfig, Event};

    use super::*;
    use crate::plan::{SendInput, prepare};

    #[test]
    fn dynamic_header_value_reaches_the_wire_and_is_recorded() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = s.read(&mut chunk).unwrap();
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            let head = String::from_utf8_lossy(&buf).into_owned();
            let value = head
                .lines()
                .find_map(|l| {
                    l.strip_prefix("x-req: ")
                        .or_else(|| l.strip_prefix("X-Req: "))
                })
                .unwrap_or("")
                .trim()
                .to_string();
            let _ = s.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nX-Echo: {value}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            );
            tx.send(value).unwrap();
        });
        let draft = Request {
            url: format!("http://{addr}/"),
            headers: vec![KeyValue {
                name: "X-Req".into(),
                value: "{{$uuid}}".into(),
                enabled: true,
            }],
            ..Request::default()
        };
        let defaults = CollectionDefaults::default();
        let mut rec = Recording::new();
        let mut plan = prepare(
            &SendInput {
                draft: &draft,
                tab_path: None,
                origin: None,
                tree: None,
                empty_defaults: &defaults,
                env: None,
            },
            &mut rec,
        )
        .unwrap();
        plan.dynamics = rec.into_values();
        assert_eq!(plan.dynamics.len(), 1);
        let (engine, events) = Engine::start(EngineConfig::default());
        let dynamic = plan.dynamics[0].value.clone();
        let sub = submit(&engine, plan, draft, None, None);
        assert_eq!(sub.record.request, None);
        loop {
            match events.recv_blocking().unwrap() {
                Event::Completed { .. } => break,
                Event::Failed { error, .. } => panic!("failed: {error:?}"),
                _ => {}
            }
        }
        let echoed = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(echoed, dynamic);
    }
}
