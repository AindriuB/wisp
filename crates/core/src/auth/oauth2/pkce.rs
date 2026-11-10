//! PKCE (RFC 7636) and the random values of the authorization flow.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

/// `n` random bytes from the OS generator, via the UUID v4 generator the tree already links.
/// Each UUID gives 122 random bits (version and variant bits are fixed); 16 bytes of it are
/// used whole, so the entropy is slightly under 8 bits per byte.
fn random_bytes(n: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(n + 16);
    while out.len() < n {
        out.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    }
    out.truncate(n);
    out
}

/// A code verifier: 64 characters from the unreserved set (RFC 7636 section 4.1).
pub(crate) fn new_verifier() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes(48))
}

/// The S256 challenge of `verifier`.
pub(crate) fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// An unguessable `state` value of 256 bits of requested length (at least 244 random).
pub(crate) fn new_state() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes(32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_matches_the_rfc_7636_appendix_b_vector() {
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn verifiers_are_43_to_128_unreserved_characters_and_differ() {
        let a = new_verifier();
        let b = new_verifier();
        assert_ne!(a, b);
        for v in [&a, &b] {
            assert!((43..=128).contains(&v.len()), "{}", v.len());
            assert!(
                v.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-._~".contains(c))
            );
        }
    }

    #[test]
    fn state_has_at_least_128_bits() {
        let s = new_state();
        // 43 base64 characters carry 256 bits.
        assert!(s.len() >= 22);
        assert_ne!(s, new_state());
    }
}
