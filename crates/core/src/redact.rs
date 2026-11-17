//! Shared credential-name heuristic for redacting stored requests.

/// Name fragments that mark a header, param, var or field as credential-bearing
/// (matched case-insensitively at name-token boundaries).
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

/// Words that stand for a credential beyond [`SECRET_FRAGMENTS`] themselves.
const EXTRA_WORDS: [&str; 3] = ["authorization", "authorisation", "passphrase"];

/// Words that may join a fragment into one token (`apikey`, `sessionid`, `authtoken`).
const JOINERS: [&str; 18] = [
    "api", "access", "refresh", "client", "private", "public", "id", "x", "my", "db", "user",
    "app", "csrf", "xsrf", "auth", "set", "secret", "key",
];

/// True when `token` (lowercase) is a credential word: a fragment, its plural, or a fragment
/// joined to the words in [`JOINERS`] on either side.
fn token_is_credential(token: &str) -> bool {
    let token = token.trim_end_matches(|c: char| c.is_ascii_digit());
    SECRET_FRAGMENTS.iter().chain(EXTRA_WORDS.iter()).any(|f| {
        token == *f
            || token.strip_suffix('s') == Some(f)
            || token
                .strip_prefix(f)
                .is_some_and(|r| JOINERS.contains(&r) || r == "s")
            || token.strip_suffix(f).is_some_and(|r| JOINERS.contains(&r))
    })
}

/// True when any name token of `text` is a credential word. Tokens are the runs of
/// alphanumerics, and the pieces of each run split where a lowercase letter meets an
/// uppercase one.
fn has_credential_token(text: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric()).any(|run| {
        if run.is_empty() {
            return false;
        }
        if token_is_credential(&run.to_lowercase()) {
            return true;
        }
        let mut piece = String::new();
        let mut prev_lower = false;
        for c in run.chars() {
            if prev_lower && c.is_uppercase() {
                if token_is_credential(&piece.to_lowercase()) {
                    return true;
                }
                piece.clear();
            }
            prev_lower = c.is_lowercase();
            piece.push(c);
        }
        token_is_credential(&piece.to_lowercase())
    })
}

/// True when a name token of `text` is a credential word (for free-form bodies).
pub fn contains_credential_fragment(text: &str) -> bool {
    has_credential_token(text)
}

/// True when `name` has a credential word among its name tokens.
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

    #[test]
    fn ordinary_words_are_not_credentials() {
        for name in [
            "X-Bypass-Cache",
            "X-Assignment-Id",
            "design",
            "signal",
            "compass",
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
