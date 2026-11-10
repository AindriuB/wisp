//! OAuth2 seams: the refresh-token store keyed by [`TokenKey`].
//!
//! The grant flows themselves arrive with the engine's OAuth2 support.

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

/// Which OAuth2 grant a token belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Grant {
    ClientCredentials,
    AuthCode,
}

/// Identifies one cached token. None of the four parts is secret.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TokenKey {
    pub grant: Grant,
    pub token_url: String,
    pub client_id: String,
    pub scope: String,
}

/// Where refresh tokens live between runs. Values are secrets: implementations must not log them.
pub trait RefreshTokenStore: Send + Sync {
    fn load(&self, key: &TokenKey) -> Option<String>;
    fn save(&self, key: &TokenKey, refresh_token: &str);
    fn forget(&self, key: &TokenKey);
}

/// Keeps refresh tokens in process memory only; the default store.
#[derive(Default)]
pub struct MemoryOnly {
    tokens: Mutex<HashMap<TokenKey, String>>,
}

impl RefreshTokenStore for MemoryOnly {
    fn load(&self, key: &TokenKey) -> Option<String> {
        let map = self.tokens.lock().unwrap_or_else(|e| e.into_inner());
        map.get(key).cloned()
    }

    fn save(&self, key: &TokenKey, refresh_token: &str) {
        let mut map = self.tokens.lock().unwrap_or_else(|e| e.into_inner());
        map.insert(key.clone(), refresh_token.to_owned());
    }

    fn forget(&self, key: &TokenKey) {
        let mut map = self.tokens.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(key);
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
        }
    }

    #[test]
    fn memory_only_saves_loads_and_forgets() {
        let s = MemoryOnly::default();
        assert_eq!(s.load(&key()), None);
        s.save(&key(), "rt-1");
        assert_eq!(s.load(&key()).as_deref(), Some("rt-1"));
        s.forget(&key());
        assert_eq!(s.load(&key()), None);
    }

    #[test]
    fn debug_redacts_memory_only_tokens() {
        let s = MemoryOnly::default();
        s.save(&key(), "REFRESHMARKER");
        for text in [format!("{s:?}"), format!("{s:#?}")] {
            assert!(text.contains("<redacted>"));
            assert!(!text.contains("REFRESHMARKER"), "{text}");
        }
    }

    #[test]
    fn token_key_debug_prints_all_four_parts() {
        let text = format!("{:?}", key());
        for part in ["AuthCode", "https://idp.test/token", "cid", "read"] {
            assert!(text.contains(part), "{text}");
        }
    }
}
