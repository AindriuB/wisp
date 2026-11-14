//! Shared credential-name heuristic for redacting stored requests.

/// Name fragments that mark a header, param, var or field as credential-bearing
/// (matched case-insensitively anywhere in the name).
pub const SECRET_FRAGMENTS: [&str; 17] = [
    "auth",
    "token",
    "secret",
    "key",
    "session",
    "cookie",
    "password",
    "passwd",
    "credential",
    "signature",
    "sig",
    "pass",
    "pwd",
    "jwt",
    "bearer",
    "dpop",
    "assertion",
];

/// True when `text`, lowercased, contains any of [`SECRET_FRAGMENTS`] (for free-form bodies).
pub fn contains_credential_fragment(text: &str) -> bool {
    let lower = text.to_lowercase();
    SECRET_FRAGMENTS.iter().any(|f| lower.contains(f))
}

/// True when `name`, trimmed and lowercased, contains any of [`SECRET_FRAGMENTS`].
pub fn is_credential_name(name: &str) -> bool {
    let lower = name.trim().to_lowercase();
    SECRET_FRAGMENTS.iter().any(|f| lower.contains(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_fragments_are_credential_names() {
        for name in [
            "x-sig",
            "my_pass",
            "db_pwd",
            "id-jwt",
            "bearer_x",
            "DPoP",
            "saml_assertion",
        ] {
            assert!(is_credential_name(name), "{name}");
        }
        assert!(contains_credential_fragment("a Bearer b"));
        assert!(!contains_credential_fragment("hello"));
    }

    #[test]
    fn credential_names() {
        for (name, want) in [
            ("Authorization", true),
            ("  X-Api-Key ", true),
            ("access_TOKEN", true),
            ("client_secret", true),
            ("Set-Cookie", true),
            ("PASSWD", true),
            ("my_credentials", true),
            ("x-signature", true),
            ("session_id", true),
            ("page", false),
            ("Content-Type", false),
            ("region", false),
            ("", false),
        ] {
            assert_eq!(is_credential_name(name), want, "{name}");
        }
    }
}
