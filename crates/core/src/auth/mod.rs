//! Auth schemes that need the final request: Digest and AWS Signature V4.
//!
//! `resolve` only expands the fields into an [`EngineAuth`]; the engine applies it.

use std::fmt;

use serde::{Deserialize, Serialize};

pub mod digest;
pub mod sigv4;

/// Resolved credentials for a scheme the engine applies at send time.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub enum EngineAuth {
    Digest {
        username: String,
        password: String,
    },
    AwsSigV4 {
        access_key: String,
        secret_key: String,
        /// Empty means none.
        session_token: String,
        region: String,
        service: String,
    },
}

impl fmt::Debug for EngineAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineAuth::Digest { username, .. } => f
                .debug_struct("Digest")
                .field("username", username)
                .field("password", &format_args!("\"<redacted>\""))
                .finish(),
            EngineAuth::AwsSigV4 {
                region, service, ..
            } => f
                .debug_struct("AwsSigV4")
                .field("access_key", &format_args!("\"<redacted>\""))
                .field("secret_key", &format_args!("\"<redacted>\""))
                .field("session_token", &format_args!("\"<redacted>\""))
                .field("region", region)
                .field("service", service)
                .finish(),
        }
    }
}

/// Lower-case hex of `bytes`.
pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[usize::from(b >> 4)] as char);
        out.push(DIGITS[usize::from(b & 15)] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest() -> EngineAuth {
        EngineAuth::Digest {
            username: "user".into(),
            password: "s3cret-pw".into(),
        }
    }

    fn sigv4() -> EngineAuth {
        EngineAuth::AwsSigV4 {
            access_key: "AKIDSECRET".into(),
            secret_key: "secretkey-value".into(),
            session_token: "tokenvalue".into(),
            region: "us-east-1".into(),
            service: "s3".into(),
        }
    }

    #[test]
    fn debug_redacts_engine_auth() {
        for (auth, secrets) in [
            (digest(), vec!["s3cret-pw"]),
            (sigv4(), vec!["AKIDSECRET", "secretkey-value", "tokenvalue"]),
        ] {
            for text in [format!("{auth:?}"), format!("{auth:#?}")] {
                assert!(text.contains("<redacted>"));
                for s in &secrets {
                    assert!(!text.contains(s), "{text}");
                }
            }
        }
    }

    #[test]
    fn hex_encodes_lower_case() {
        assert_eq!(hex(&[0, 0xab, 0xff]), "00abff");
    }
}
