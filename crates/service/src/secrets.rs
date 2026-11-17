//! Keychain store selection shared by the GUI and the CLI.

use wisp_storage::keychain;
use wisp_storage::vault::{SecretStore, SecretStores, StoreKind};

/// Environment variable that overrides the keychain service name.
pub const KEYCHAIN_SERVICE_ENV: &str = "WISP_KEYCHAIN_SERVICE";

/// The keychain service: `WISP_KEYCHAIN_SERVICE` when set and non-empty, else `wisp`.
pub fn service_name(var: Option<&str>) -> String {
    match var {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => "wisp".to_string(),
    }
}

/// True when a `portable` marker sits beside the executable (the rule `DataDir::resolve` uses).
pub fn portable_marker() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("portable").exists()))
        .unwrap_or(false)
}

/// The app's one [`SecretStores`]. A future store kind is added in `get` and nowhere else.
pub struct KeychainStores {
    keychain: Box<dyn SecretStore>,
}

impl KeychainStores {
    /// Open the OS keychain store (inert when `portable`).
    pub fn open(portable: bool) -> Self {
        let service = service_name(std::env::var(KEYCHAIN_SERVICE_ENV).ok().as_deref());
        Self {
            keychain: keychain::open(&service, portable),
        }
    }
}

impl SecretStores for KeychainStores {
    fn get(&self, kind: StoreKind) -> Option<&dyn SecretStore> {
        match kind {
            StoreKind::Keychain => Some(&*self.keychain),
            _ => None,
        }
    }
}
