//! The token endpoint: request building, the capped response read and parsing.

use std::fmt;
use std::time::Duration;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use super::json;
use crate::engine::err;
use crate::model::{EngineError, ErrorKind};
use crate::request::ClientAuth;

/// Largest token response read, in bytes.
const MAX_RESPONSE: usize = 1024 * 1024;
/// Most of a failing response body quoted in an error, in bytes.
const MAX_ERROR_BODY: usize = 2048;

/// A string that is a secret: Debug never shows it.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Secret(String);

impl Secret {
    pub(crate) fn new(s: impl Into<String>) -> Self {
        Secret(s.into())
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("\"<redacted>\"")
    }
}

/// A parsed successful token response.
#[derive(Clone)]
pub(crate) struct TokenResponse {
    pub(crate) access_token: Secret,
    pub(crate) refresh_token: Option<Secret>,
    pub(crate) expires_in: Option<Duration>,
}

impl fmt::Debug for TokenResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenResponse")
            .field("access_token", &format_args!("\"<redacted>\""))
            .field(
                "refresh_token",
                &self
                    .refresh_token
                    .as_ref()
                    .map(|_| format_args!("\"<redacted>\"")),
            )
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

/// The client's credentials at the token endpoint.
pub(crate) struct ClientCreds<'a> {
    pub(crate) id: &'a str,
    pub(crate) secret: &'a str,
    pub(crate) mode: ClientAuth,
}

impl fmt::Debug for ClientCreds<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientCreds")
            .field("id", &self.id)
            .field("secret", &format_args!("\"<redacted>\""))
            .field("mode", &self.mode)
            .finish()
    }
}

/// Characters left as they are in `application/x-www-form-urlencoded` (RFC 3986 unreserved).
const FORM: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

fn enc(s: &str) -> String {
    utf8_percent_encode(s, FORM).to_string()
}

/// Rejects a URL the flow may not contact: `https`, or `http` to a loopback host.
pub(crate) fn check_endpoint(raw: &str, what: &str) -> Result<reqwest::Url, EngineError> {
    let url = reqwest::Url::parse(raw)
        .map_err(|_| err(ErrorKind::Auth, format!("the {what} is not a valid URL")))?;
    let loopback = url
        .host_str()
        .is_some_and(|h| matches!(h, "127.0.0.1" | "[::1]") || h.eq_ignore_ascii_case("localhost"));
    match url.scheme() {
        "https" => Ok(url),
        "http" if loopback => Ok(url),
        _ => Err(err(
            ErrorKind::Auth,
            format!("the {what} must use https (http is allowed only for localhost)"),
        )),
    }
}

/// POSTs a grant to the token endpoint and parses the answer. `params` hold the grant fields.
pub(crate) async fn post(
    client: &reqwest::Client,
    url: &reqwest::Url,
    creds: &ClientCreds<'_>,
    params: &[(&str, &str)],
    timeout: Duration,
) -> Result<TokenResponse, EngineError> {
    let public = creds.secret.is_empty();
    let mut fields: Vec<(&str, &str)> = params.to_vec();
    let mut rb = client
        .post(url.clone())
        .header("accept", "application/json")
        .header("content-type", "application/x-www-form-urlencoded")
        .timeout(timeout);
    if public || creds.mode == ClientAuth::Body {
        fields.push(("client_id", creds.id));
        if !public {
            fields.push(("client_secret", creds.secret));
        }
    } else {
        use base64::Engine as _;
        let pair = format!("{}:{}", enc(creds.id), enc(creds.secret));
        let value = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(pair)
        );
        rb = rb.header("authorization", value);
    }
    let body = fields
        .iter()
        .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
        .collect::<Vec<_>>()
        .join("&");
    let mut resp = rb.body(body).send().await.map_err(|e| transport(&e))?;
    let status = resp.status();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| transport(&e))? {
        if buf.len() + chunk.len() > MAX_RESPONSE {
            return Err(err(
                ErrorKind::Auth,
                "the token response is larger than 1 MiB",
            ));
        }
        buf.extend_from_slice(&chunk);
    }
    parse(status.as_u16(), &buf)
}

fn clone_without_url(e: &reqwest::Error) -> String {
    // `Display` of a reqwest error appends the URL; cut it off, keep the description.
    let text = e.to_string();
    match text.split_once(" for url (") {
        Some((head, _)) => head.to_owned(),
        None => text,
    }
}

/// A transport failure. The URL is dropped from the text; the causes are kept.
fn transport(e: &reqwest::Error) -> EngineError {
    use std::error::Error as _;
    let kind = if e.is_timeout() {
        ErrorKind::Timeout
    } else {
        ErrorKind::Auth
    };
    let mut message = format!("token request failed: {}", clone_without_url(e));
    let mut src = e.source();
    while let Some(s) = src {
        message.push_str(": ");
        message.push_str(&s.to_string());
        src = s.source();
    }
    err(kind, message)
}

/// Parses a token response of the given status.
pub(crate) fn parse(status: u16, body: &[u8]) -> Result<TokenResponse, EngineError> {
    let fail = |why: &str| {
        err(
            ErrorKind::Auth,
            format!("{why} (status {status}): {}", quote_body(body)),
        )
    };
    if !(200..300).contains(&status) {
        return Err(fail("the token endpoint refused the request"));
    }
    let obj = json::object(body).ok_or_else(|| fail("the token response is not a JSON object"))?;
    let text = |k: &str| match obj.get(k) {
        Some(json::Value::Str(s)) => Some(s.as_str()),
        _ => None,
    };
    let access = match text("access_token") {
        Some(a) if !a.is_empty() => a,
        _ => return Err(fail("the token response has no access_token")),
    };
    match text("token_type") {
        Some(t) if t.eq_ignore_ascii_case("bearer") => {}
        Some(_) => {
            return Err(err(
                ErrorKind::Auth,
                format!("unsupported token_type (status {status}); only bearer is supported"),
            ));
        }
        None => return Err(fail("the token response has no token_type")),
    }
    let expires_in = match obj.get("expires_in") {
        Some(json::Value::Num(n)) => Some(*n),
        Some(json::Value::Str(s)) => s.trim().parse::<f64>().ok(),
        _ => None,
    };
    let expires_in = expires_in
        .filter(|s| s.is_finite() && *s >= 0.0)
        .map(Duration::from_secs_f64);
    let refresh_token = text("refresh_token")
        .filter(|s| !s.is_empty())
        .map(Secret::new);
    Ok(TokenResponse {
        access_token: Secret::new(access),
        refresh_token,
        expires_in,
    })
}

/// A response body fit for an error message: token values removed, at most 2 KiB.
fn quote_body(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    let mut out = redact_tokens(&text);
    if out.len() > MAX_ERROR_BODY {
        let mut end = MAX_ERROR_BODY;
        while !out.is_char_boundary(end) {
            end -= 1;
        }
        out.truncate(end);
        out.push_str("...");
    }
    out
}

const TOKEN_KEYS: [&str; 3] = ["access_token", "refresh_token", "id_token"];

/// Replaces the value after `access_token`, `refresh_token` and `id_token` (JSON or form
/// style, quoted or not) with `<redacted>`. Done before truncation, so no token is cut in half.
fn redact_tokens(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let hit = TOKEN_KEYS
            .iter()
            .filter_map(|k| lower[i..].find(k).map(|p| (i + p, k.len())))
            .min_by_key(|(p, _)| *p);
        let Some((at, klen)) = hit else { break };
        let mut j = at + klen;
        if bytes.get(j) == Some(&b'"') {
            j += 1;
        }
        while bytes.get(j).is_some_and(u8::is_ascii_whitespace) {
            j += 1;
        }
        if !matches!(bytes.get(j), Some(b':' | b'=')) {
            out.push_str(&s[i..at + klen]);
            i = at + klen;
            continue;
        }
        j += 1;
        while bytes.get(j).is_some_and(u8::is_ascii_whitespace) {
            j += 1;
        }
        out.push_str(&s[i..j]);
        if bytes.get(j) == Some(&b'"') {
            let mut k = j + 1;
            while k < bytes.len() && bytes[k] != b'"' {
                k += if bytes[k] == b'\\' { 2 } else { 1 };
            }
            out.push_str("\"<redacted>\"");
            i = (k + 1).min(s.len());
        } else {
            let mut k = j;
            while k < bytes.len()
                && !matches!(bytes[k], b' ' | b'\t' | b'\r' | b'\n' | b',' | b'&' | b'}')
            {
                k += 1;
            }
            out.push_str("<redacted>");
            i = k;
        }
    }
    out.push_str(&s[i.min(s.len())..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redaction_covers_json_form_and_truncated_values() {
        let r = redact_tokens(
            r#"{"Access_Token" : "SECRETVAL","refresh_token":"SECRETVAL", "id_token":"SECRETVAL"#,
        );
        assert!(!r.contains("SECRETVAL"), "{r}");
        assert_eq!(r.matches("<redacted>").count(), 3, "{r}");
        let r = redact_tokens("access_token=SECRETVAL&x=1");
        assert!(!r.contains("SECRETVAL") && r.contains("&x=1"), "{r}");
        let r = redact_tokens(r#"{"access_token":"a\"SECRETVAL","error":"e"}"#);
        assert!(
            !r.contains("SECRETVAL") && r.contains(r#""error":"e""#),
            "{r}"
        );
    }

    #[test]
    fn quoted_body_is_capped_after_redaction() {
        let body = format!(
            r#"{{"access_token":"SECRETVAL","pad":"{}"}}"#,
            "é".repeat(5000)
        );
        let q = quote_body(body.as_bytes());
        assert!(q.len() <= MAX_ERROR_BODY + 3, "{}", q.len());
        assert!(!q.contains("SECRETVAL"));
    }

    #[test]
    fn debug_redacts_secrets() {
        let t = TokenResponse {
            access_token: Secret::new("ACCESSMARK"),
            refresh_token: Some(Secret::new("REFRESHMARK")),
            expires_in: None,
        };
        let c = ClientCreds {
            id: "cid",
            secret: "CSECRETMARK",
            mode: ClientAuth::Basic,
        };
        for text in [
            format!("{t:?}"),
            format!("{t:#?}"),
            format!("{:?}", Secret::new("ACCESSMARK")),
            format!("{c:?}"),
            format!("{c:#?}"),
        ] {
            assert!(text.contains("<redacted>"), "{text}");
            for m in ["ACCESSMARK", "REFRESHMARK", "CSECRETMARK"] {
                assert!(!text.contains(m), "{text}");
            }
        }
    }

    #[test]
    fn parse_accepts_bearer_in_any_case_and_numeric_strings() {
        let r = parse(
            200,
            br#"{"access_token":"a","token_type":"BeArEr","expires_in":"60"}"#,
        )
        .unwrap();
        assert_eq!(r.expires_in, Some(Duration::from_secs(60)));
        let e = parse(200, br#"{"access_token":"a","token_type":"mac"}"#).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Auth);
    }

    #[test]
    fn endpoint_check_allows_https_and_loopback_http_only() {
        for ok in [
            "https://idp.test/t",
            "http://127.0.0.1:1/t",
            "http://[::1]:1/t",
            "http://localhost/t",
        ] {
            assert!(check_endpoint(ok, "x").is_ok(), "{ok}");
        }
        for bad in [
            "http://idp.test/t",
            "ftp://localhost/t",
            "nonsense",
            "http://127.0.0.2/t",
        ] {
            assert_eq!(
                check_endpoint(bad, "x").unwrap_err().kind,
                ErrorKind::Auth,
                "{bad}"
            );
        }
    }
}
