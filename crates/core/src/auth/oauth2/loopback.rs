//! The loopback redirect listener of the authorization-code flow (RFC 8252 section 7.3).
//!
//! A hand-written request-line parser on a tokio `TcpListener`; no HTTP server crate.

use std::fmt;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;

use super::token::Secret;

/// Most of a request head read from one connection.
const MAX_HEAD: usize = 8 * 1024;
/// Longest a connection may take to send its head.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// The parameters of a `GET /callback` request.
pub(crate) struct Callback {
    pub(crate) state: Option<Secret>,
    pub(crate) code: Option<Secret>,
    /// The `error` parameter, kept only when it looks like an RFC 6749 error code.
    pub(crate) error: Option<String>,
}

impl fmt::Debug for Callback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Callback")
            .field("state", &format_args!("\"<redacted>\""))
            .field("code", &format_args!("\"<redacted>\""))
            .field("error", &self.error)
            .finish()
    }
}

/// Binds `127.0.0.1:port` (0 picks a free port) and returns the listener and its port.
pub(crate) async fn bind(port: u16) -> std::io::Result<(TcpListener, u16)> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

/// Serves until a `GET /callback` arrives, then returns its parameters. Takes the listener, so
/// the port is closed when this returns or the future is dropped.
pub(crate) async fn wait(listener: TcpListener) -> Callback {
    let mut conns: JoinSet<Option<Callback>> = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => { conns.spawn(serve(stream)); }
                Err(e) => {
                    tracing::debug!(error = %e, "loopback accept failed");
                    // Back off briefly: a persistent error must not spin.
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            },
            Some(done) = conns.join_next() => {
                if let Ok(Some(cb)) = done {
                    return cb;
                }
            }
        }
    }
}

const PAGE: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>Wisp</title></head>\
<body><p>You can close this tab and return to Wisp.</p></body></html>";

async fn serve(mut stream: TcpStream) -> Option<Callback> {
    let head = tokio::time::timeout(READ_TIMEOUT, read_head(&mut stream))
        .await
        .ok()?;
    let target = head.as_deref().and_then(callback_target);
    let (status, body) = if target.is_some() {
        ("200 OK", PAGE)
    } else {
        ("404 Not Found", "Not found")
    };
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    // A failed write only means the client went away.
    let _ = stream.write_all(reply.as_bytes()).await;
    let _ = stream.shutdown().await;
    target.map(|q| parse_query(&q))
}

/// Reads up to `MAX_HEAD` bytes, stopping at the blank line. `None` when the head is too big
/// or the peer closed early.
async fn read_head(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            return Some(buf);
        }
        if buf.len() >= MAX_HEAD {
            return None;
        }
    }
}

/// The query of a `GET /callback` request line (empty when there is none); `None` for any
/// other request.
fn callback_target(head: &[u8]) -> Option<String> {
    let line_end = head.windows(2).position(|w| w == b"\r\n")?;
    let line = std::str::from_utf8(&head[..line_end]).ok()?;
    let mut parts = line.split(' ');
    let (method, target, version) = (parts.next()?, parts.next()?, parts.next()?);
    if method != "GET" || !version.starts_with("HTTP/") || parts.next().is_some() {
        return None;
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    (path == "/callback").then(|| query.to_owned())
}

fn decode(s: &str) -> String {
    let spaced = s.replace('+', " ");
    percent_encoding::percent_decode_str(&spaced)
        .decode_utf8_lossy()
        .into_owned()
}

fn parse_query(q: &str) -> Callback {
    let (mut state, mut code, mut error) = (None, None, None);
    for pair in q.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        match k {
            "state" if state.is_none() => state = Some(Secret::new(decode(v))),
            "code" if code.is_none() => code = Some(Secret::new(decode(v))),
            "error" if error.is_none() => {
                let v = decode(v);
                let plain = !v.is_empty()
                    && v.len() <= 40
                    && v.chars().all(|c| c.is_ascii_lowercase() || c == '_');
                error = Some(if plain { v } else { "unknown".to_owned() });
            }
            _ => {}
        }
    }
    Callback { state, code, error }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_get_callback_is_a_callback() {
        let q = |s: &str| callback_target(s.as_bytes());
        assert_eq!(
            q("GET /callback?a=b HTTP/1.1\r\nHost: x\r\n\r\n").as_deref(),
            Some("a=b")
        );
        assert_eq!(q("GET /callback HTTP/1.1\r\n\r\n").as_deref(), Some(""));
        assert_eq!(q("POST /callback HTTP/1.1\r\n\r\n"), None);
        assert_eq!(q("GET /favicon.ico HTTP/1.1\r\n\r\n"), None);
        assert_eq!(q("GET /callback/x HTTP/1.1\r\n\r\n"), None);
        assert_eq!(q("garbage\r\n\r\n"), None);
    }

    #[test]
    fn query_values_are_decoded_and_errors_sanitized() {
        let cb = parse_query("state=a%20b+c&code=xyz&error=access_denied");
        assert_eq!(cb.state.unwrap().expose(), "a b c");
        assert_eq!(cb.code.unwrap().expose(), "xyz");
        assert_eq!(cb.error.as_deref(), Some("access_denied"));
        let cb = parse_query("error=%3Cscript%3E");
        assert_eq!(cb.error.as_deref(), Some("unknown"));
    }

    #[test]
    fn debug_redacts_callback() {
        let cb = parse_query("state=STATEMARK&code=CODEMARK");
        for text in [format!("{cb:?}"), format!("{cb:#?}")] {
            assert!(text.contains("<redacted>"));
            assert!(
                !text.contains("STATEMARK") && !text.contains("CODEMARK"),
                "{text}"
            );
        }
    }
}
