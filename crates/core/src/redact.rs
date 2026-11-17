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

/// Fragments that also mark a token when they end it (`githubtoken`, `newpassword`) or end it
/// before a joiner (`jsessionid`).
const STRONG_FRAGMENTS: [&str; 6] = [
    "token", "password", "passwd", "secret", "session", "passcode",
];

/// True when `token` (lowercase) is a credential word: a fragment, its plural, a fragment
/// joined to the words in [`JOINERS`] on either side, or a strong fragment ending it.
fn token_is_credential(token: &str) -> bool {
    let token = token.trim_end_matches(|c: char| c.is_ascii_digit());
    let fragment = SECRET_FRAGMENTS.iter().chain(EXTRA_WORDS.iter()).any(|f| {
        token == *f
            || token.strip_suffix('s') == Some(f)
            || token
                .strip_prefix(f)
                .is_some_and(|r| JOINERS.contains(&r) || r == "s")
            || token.strip_suffix(f).is_some_and(|r| JOINERS.contains(&r))
    });
    fragment
        || STRONG_FRAGMENTS.iter().any(|f| {
            token.match_indices(f).any(|(i, _)| {
                let rest = &token[i + f.len()..];
                rest.is_empty() || rest == "s" || JOINERS.contains(&rest)
            })
        })
}

/// Split a run of alphanumerics at lower-to-upper, letter/digit and acronym boundaries
/// (`JWTToken` gives `JWT`, `Token`; `OAuth2Token` gives `OAuth`, `2`, `Token`).
fn split_run(run: &str) -> Vec<String> {
    let chars: Vec<char> = run.chars().collect();
    let mut pieces = Vec::new();
    let mut piece = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if let Some(&p) = i.checked_sub(1).map(|j| &chars[j]) {
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            let boundary = (p.is_lowercase() && c.is_uppercase())
                || (p.is_alphabetic() != c.is_alphabetic())
                || (p.is_uppercase() && c.is_uppercase() && next_lower);
            if boundary {
                pieces.push(std::mem::take(&mut piece));
            }
        }
        piece.push(c);
    }
    pieces.push(piece);
    pieces
}

/// True when any name token of `text` is a credential word. Tokens are the runs of
/// alphanumerics and the pieces of each run (see [`split_run`]).
fn has_credential_token(text: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric()).any(|run| {
        !run.is_empty()
            && (token_is_credential(&run.to_lowercase())
                || split_run(run)
                    .iter()
                    .any(|p| token_is_credential(&p.to_lowercase())))
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
