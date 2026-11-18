//! Shared credential-name heuristic for redacting stored requests.

/// Name fragments that mark a header, param, var or field as credential-bearing
/// (matched case-insensitively as substrings of name tokens, see [`ALLOWED_WORDS`]).
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

/// Ordinary words that contain a fragment (`bypass` has `pass`, `design` has `sig`). A token
/// equal to one of these contributes no match; the same word inside a longer token still does.
const ALLOWED_WORDS: [&str; 5] = ["bypass", "compass", "design", "signal", "assignment"];

/// True when a token of `text` (a maximal run of alphanumerics, lowercased) that is not in
/// [`ALLOWED_WORDS`] contains any of [`SECRET_FRAGMENTS`] as a substring.
fn has_credential_token(text: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric()).any(|run| {
        let token = run.to_lowercase();
        !token.is_empty()
            && !ALLOWED_WORDS.contains(&token.as_str())
            && SECRET_FRAGMENTS.iter().any(|f| token.contains(f))
    })
}

/// True when a token of `text` contains a credential fragment (for free-form bodies).
pub fn contains_credential_fragment(text: &str) -> bool {
    has_credential_token(text)
}

/// True when a token of `name` contains a credential fragment.
pub fn is_credential_name(name: &str) -> bool {
    has_credential_token(name.trim())
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

    const CREDENTIAL_NAME_ROWS: &[(&str, bool)] = &[
        ("Authorization", true),
        ("  X-Api-Key ", true),
        ("access_TOKEN", true),
        ("client_secret", true),
        ("Set-Cookie", true),
        ("PASSWD", true),
        ("my_credentials", true),
        ("x-signature", true),
        ("session_id", true),
        ("jsessionid", true),
        ("JSESSIONID", true),
        ("sessiontoken", true),
        ("newpassword", true),
        ("adminpassword", true),
        ("OAuth2Token", true),
        ("JWTToken", true),
        ("githubtoken", true),
        ("oauthtoken", true),
        ("bottoken", true),
        ("passcode", true),
        ("privkey", true),
        ("adminpass", true),
        ("rootpwd", true),
        ("masterkey", true),
        ("authcode", true),
        ("rootpass", true),
        ("X-Masterkey", true),
        ("Authentication", true),
        ("webhooksignature", true),
        ("awscredentials", true),
        ("page", false),
        ("Content-Type", false),
        ("region", false),
        ("", false),
    ];

    #[test]
    fn credential_names() {
        for &(name, want) in CREDENTIAL_NAME_ROWS {
            assert_eq!(is_credential_name(name), want, "{name}");
        }
    }

    #[test]
    fn old_true_names_still_match() {
        let extra = [
            "x-signature",
            "x-sig",
            "my_pass",
            "db_pwd",
            "id-jwt",
            "bearer_x",
            "DPoP",
            "saml_assertion",
            "bypassCache",
            "signals",
            "assignmentId",
            "monkey",
        ];
        let table = CREDENTIAL_NAME_ROWS
            .iter()
            .filter(|(_, want)| *want)
            .map(|(name, _)| *name);
        for name in table.chain(extra) {
            assert!(is_credential_name(name), "{name}");
            assert!(contains_credential_fragment(name), "{name}");
        }
    }

    #[test]
    fn ordinary_words_are_not_credentials() {
        for name in [
            "X-Bypass-Cache",
            "X-Assignment-Id",
            "design",
            "signal",
            "compass",
            "bypass",
        ] {
            assert!(!is_credential_name(name), "{name}");
            assert!(!contains_credential_fragment(name), "{name}");
        }
        for name in [
            "apikey",
            "accessToken",
            "authToken",
            "x-api-key",
            "client_secret",
            "DPoP",
            "Proxy-Authorization",
            "sessionid",
        ] {
            assert!(is_credential_name(name), "{name}");
        }
    }
}
