//! The reqwest client cache and TLS setup.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::cookies::CookieJar;
use crate::engine::err;
use crate::model::{ClientSettings, EngineError, ErrorKind};

/// One `reqwest::Client` per distinct `ClientSettings`.
///
/// `ClientSettings` carries the collection id, so each collection has its own client and its own
/// `CookieJar`; a request with no collection has a jar of its own too.
#[derive(Default)]
pub(crate) struct ClientCache {
    clients: Mutex<HashMap<ClientSettings, reqwest::Client>>,
    /// One jar per collection id; unused by clients until the cookie provider lands.
    jars: Mutex<HashMap<Option<String>, CookieJar>>,
}

impl ClientCache {
    /// The jar for a collection, created on first use.
    pub(crate) fn jar(&self, collection: &Option<String>) -> CookieJar {
        let mut jars = self.jars.lock().unwrap_or_else(|e| e.into_inner());
        jars.entry(collection.clone()).or_default().clone()
    }

    pub(crate) fn get(&self, settings: &ClientSettings) -> Result<reqwest::Client, EngineError> {
        self.jar(&settings.collection);
        let mut map = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = map.get(settings) {
            return Ok(c.clone());
        }
        install_crypto_provider();
        // Bundled Mozilla roots only: rustls-platform-verifier is linked by reqwest but never used.
        let roots = webpki_root_certs::TLS_SERVER_ROOT_CERTS
            .iter()
            .map(|c| reqwest::Certificate::from_der(c.as_ref()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| err(ErrorKind::Other, format!("cannot load bundled roots: {e}")))?;
        let mut builder = reqwest::Client::builder()
            .tls_backend_rustls()
            .tls_certs_only(roots)
            .http1_max_headers(HTTP1_MAX_RESPONSE_HEADERS);
        if !settings.verify_tls {
            builder = builder.tls_danger_accept_invalid_certs(true);
        }
        if !settings.follow_redirects {
            builder = builder.redirect(reqwest::redirect::Policy::none());
        }
        if let Some(proxy) = &settings.proxy {
            let proxy = reqwest::Proxy::all(proxy.as_str())
                .map_err(|e| err(ErrorKind::InvalidUrl, format!("invalid proxy: {e}")))?;
            builder = builder.proxy(proxy);
        }
        let client = builder
            .build()
            .map_err(|e| err(ErrorKind::Other, format!("cannot build client: {e}")))?;
        map.insert(settings.clone(), client.clone());
        Ok(client)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.clients.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

/// HTTP/1 response header-count limit, passed to `ClientBuilder::http1_max_headers`.
pub(crate) const HTTP1_MAX_RESPONSE_HEADERS: usize = 1000;

/// reqwest is built with `rustls-no-provider`; install ring as the process default once.
fn install_crypto_provider() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        if rustls::crypto::ring::default_provider()
            .install_default()
            .is_err()
        {
            tracing::debug!("a rustls crypto provider was already installed");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(verify_tls: bool) -> ClientSettings {
        ClientSettings {
            verify_tls,
            follow_redirects: true,
            proxy: None,
            collection: None,
        }
    }

    #[test]
    fn equal_settings_share_a_client_and_different_settings_do_not() {
        let cache = ClientCache::default();
        cache.get(&settings(true)).unwrap();
        cache.get(&settings(true)).unwrap();
        assert_eq!(cache.len(), 1);
        cache.get(&settings(false)).unwrap();
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn each_collection_id_gets_its_own_client_and_jar() {
        let cache = ClientCache::default();
        let mut a = settings(true);
        a.collection = Some("a".into());
        let mut b = settings(true);
        b.collection = Some("b".into());
        cache.get(&a).unwrap();
        cache.get(&b).unwrap();
        cache.get(&settings(true)).unwrap();
        assert_eq!(cache.len(), 3);
        assert!(!cache.jar(&a.collection).same_jar(&cache.jar(&b.collection)));
        assert!(!cache.jar(&a.collection).same_jar(&cache.jar(&None)));
        assert!(
            cache
                .jar(&a.collection)
                .same_jar(&cache.jar(&Some("a".into())))
        );
        assert!(cache.jar(&None).same_jar(&cache.jar(&None)));
    }

    #[test]
    fn every_bundled_root_is_accepted_by_a_root_store() {
        let mut store = rustls::RootCertStore::empty();
        let (added, ignored) = store
            .add_parsable_certificates(webpki_root_certs::TLS_SERVER_ROOT_CERTS.iter().cloned());
        assert_eq!(ignored, 0);
        assert_eq!(added, webpki_root_certs::TLS_SERVER_ROOT_CERTS.len());
    }

    #[test]
    fn install_crypto_provider_leaves_a_default_provider() {
        install_crypto_provider();
        assert!(rustls::crypto::CryptoProvider::get_default().is_some());
    }

    #[test]
    fn an_invalid_proxy_is_an_invalid_url_error() {
        let mut s = settings(true);
        s.proxy = Some("not a proxy".into());
        let e = ClientCache::default().get(&s).unwrap_err();
        assert_eq!(e.kind, ErrorKind::InvalidUrl);
    }
}
