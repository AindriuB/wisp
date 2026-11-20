//! Which certificate authorities the engine trusts, beyond the bundled Mozilla roots.

use std::fmt;
use std::io::Read;
use std::path::Path;

use base64::Engine as _;
use rustls::pki_types::CertificateDer;

/// The largest CA file the loader reads.
pub const MAX_CA_FILE_BYTES: u64 = 1024 * 1024;

/// Engine-wide TLS trust. The bundled roots are always trusted; this only ever adds to them.
#[derive(Clone, PartialEq, Eq)]
pub struct TlsTrust {
    /// Also trust the operating system's store (through the platform verifier).
    pub system: bool,
    /// Extra CA certificates, DER encoded.
    pub extra: Vec<Vec<u8>>,
}

impl Default for TlsTrust {
    fn default() -> Self {
        Self {
            system: true,
            extra: Vec::new(),
        }
    }
}

impl fmt::Debug for TlsTrust {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsTrust")
            .field("system", &self.system)
            .field("extra_certificates", &self.extra.len())
            .finish()
    }
}

/// Why a CA file was refused. Messages name the path and the reason, never file content.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct TrustError(String);

impl TrustError {
    fn new(path: &Path, reason: impl fmt::Display) -> Self {
        Self(format!(
            "CA file {}: {reason}",
            path.display().to_string().escape_debug()
        ))
    }
}

impl TlsTrust {
    /// Read the certificates of a PEM file. Fails closed: any problem refuses the whole file.
    pub fn load_pem_file(path: &Path) -> Result<Vec<Vec<u8>>, TrustError> {
        let meta = std::fs::metadata(path).map_err(|e| TrustError::new(path, e.kind()))?;
        if !meta.is_file() {
            return Err(TrustError::new(path, "not a regular file"));
        }
        if meta.len() > MAX_CA_FILE_BYTES {
            return Err(TrustError::new(path, "larger than 1 MiB"));
        }
        let file = std::fs::File::open(path).map_err(|e| TrustError::new(path, e.kind()))?;
        let mut bytes = Vec::new();
        file.take(MAX_CA_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| TrustError::new(path, e.kind()))?;
        if bytes.len() as u64 > MAX_CA_FILE_BYTES {
            return Err(TrustError::new(path, "larger than 1 MiB"));
        }
        let text = String::from_utf8(bytes).map_err(|_| TrustError::new(path, "not PEM text"))?;
        parse_pem(&text).map_err(|reason| TrustError::new(path, reason))
    }
}

fn parse_pem(text: &str) -> Result<Vec<Vec<u8>>, String> {
    let mut certs = Vec::new();
    let mut current: Option<(String, String)> = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(label) = line
            .strip_prefix("-----BEGIN ")
            .and_then(|l| l.strip_suffix("-----"))
        {
            if current.is_some() {
                return Err("a PEM block is not closed".into());
            }
            if label.contains("PRIVATE KEY") {
                return Err("contains a private key".into());
            }
            if label != "CERTIFICATE" {
                return Err("contains a block that is not a certificate".into());
            }
            current = Some((label.to_string(), String::new()));
        } else if line.starts_with("-----END ") {
            let Some((_, body)) = current.take() else {
                return Err("a PEM block ends without beginning".into());
            };
            let der = base64::engine::general_purpose::STANDARD
                .decode(body.as_bytes())
                .map_err(|_| "a certificate block is not valid base64".to_string())?;
            let mut store = rustls::RootCertStore::empty();
            store
                .add(CertificateDer::from(der.clone()))
                .map_err(|_| "a block does not parse as a certificate".to_string())?;
            certs.push(der);
        } else if let Some((_, body)) = current.as_mut() {
            body.push_str(line);
        }
    }
    if current.is_some() {
        return Err("a PEM block is not closed".into());
    }
    if certs.is_empty() {
        return Err("contains no certificate".into());
    }
    Ok(certs)
}
