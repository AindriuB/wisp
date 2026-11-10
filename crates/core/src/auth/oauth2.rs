//! OAuth2 in the engine: the token cache, the grant flows and the refresh-token store seam.
//!
//! Access tokens live in memory only. Refresh tokens live in memory too, and also go to the
//! [`RefreshTokenStore`] when the auth sets `persist_refresh`.

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

mod loopback;
mod manager;
mod pkce;
mod token;

pub use manager::Clock;
pub(crate) use manager::{OAuth2, SystemClock};

/// Which OAuth2 grant a token belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Grant {
    ClientCredentials,
    AuthCode,
}

/// Identifies one cached token. No part is secret. `collection` and `environment` keep two
/// identities on the same IdP client from sharing a token.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TokenKey {
    pub grant: Grant,
    pub token_url: String,
    pub client_id: String,
    pub scope: String,
    /// Collection id; `None` for a scratch request.
    pub collection: Option<String>,
    /// Environment name; `None` when no environment is active.
    pub environment: Option<String>,
}

/// A failure of a [`RefreshTokenStore`]. The message must never hold a token; the engine shows
/// it to the user and logs it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct RefreshStoreError(String);

impl RefreshStoreError {
    /// A store failure with a message that holds no token.
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// Where refresh tokens live between runs. Values are secrets: implementations must not log them.
///
/// Implementations may block (a keychain prompt waits for the user). The engine must therefore
/// call a blocking store through `tokio::task::spawn_blocking`; that wiring arrives with the
/// keychain store. [`MemoryOnly`] never blocks and never fails.
pub trait RefreshTokenStore: Send + Sync {
    fn load(&self, key: &TokenKey) -> Result<Option<String>, RefreshStoreError>;
    fn save(&self, key: &TokenKey, refresh_token: &str) -> Result<(), RefreshStoreError>;
    fn forget(&self, key: &TokenKey) -> Result<(), RefreshStoreError>;
}

/// Keeps refresh tokens in process memory only; the default store.
#[derive(Default)]
pub struct MemoryOnly {
    tokens: Mutex<HashMap<TokenKey, String>>,
}

impl RefreshTokenStore for MemoryOnly {
    fn load(&self, key: &TokenKey) -> Result<Option<String>, RefreshStoreError> {
        let map = self.tokens.lock().unwrap_or_else(|e| e.into_inner());
        Ok(map.get(key).cloned())
    }

    fn save(&self, key: &TokenKey, refresh_token: &str) -> Result<(), RefreshStoreError> {
        let mut map = self.tokens.lock().unwrap_or_else(|e| e.into_inner());
        map.insert(key.clone(), refresh_token.to_owned());
        Ok(())
    }

    fn forget(&self, key: &TokenKey) -> Result<(), RefreshStoreError> {
        let mut map = self.tokens.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(key);
        Ok(())
    }
}

impl fmt::Debug for MemoryOnly {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemoryOnly")
            .field("tokens", &format_args!("<redacted>"))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> TokenKey {
        TokenKey {
            grant: Grant::AuthCode,
            token_url: "https://idp.test/token".into(),
            client_id: "cid".into(),
            scope: "read".into(),
            collection: Some("c1".into()),
            environment: Some("dev".into()),
        }
    }

    #[test]
    fn keys_differing_only_in_collection_or_environment_are_distinct() {
        let s = MemoryOnly::default();
        s.save(&key(), "rt-1").unwrap();
        let mut other_collection = key();
        other_collection.collection = Some("c2".into());
        let mut other_env = key();
        other_env.environment = None;
        assert_ne!(key(), other_collection);
        assert_ne!(key(), other_env);
        assert_eq!(s.load(&other_collection).unwrap(), None);
        assert_eq!(s.load(&other_env).unwrap(), None);
        assert_eq!(s.load(&key()).unwrap().as_deref(), Some("rt-1"));
    }

    #[test]
    fn memory_only_saves_loads_and_forgets() {
        let s = MemoryOnly::default();
        assert_eq!(s.load(&key()).unwrap(), None);
        s.save(&key(), "rt-1").unwrap();
        assert_eq!(s.load(&key()).unwrap().as_deref(), Some("rt-1"));
        s.forget(&key()).unwrap();
        assert_eq!(s.load(&key()).unwrap(), None);
    }

    #[test]
    fn debug_redacts_memory_only_tokens() {
        let s = MemoryOnly::default();
        s.save(&key(), "REFRESHMARKER").unwrap();
        for text in [format!("{s:?}"), format!("{s:#?}")] {
            assert!(text.contains("<redacted>"));
            assert!(!text.contains("REFRESHMARKER"), "{text}");
        }
    }

    #[test]
    fn token_key_debug_prints_every_part() {
        let text = format!("{:?}", key());
        for part in [
            "AuthCode",
            "https://idp.test/token",
            "cid",
            "read",
            "c1",
            "dev",
        ] {
            assert!(text.contains(part), "{text}");
        }
    }
}
