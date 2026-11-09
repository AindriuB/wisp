//! HTTP Digest authentication (RFC 7616): challenge parsing and response computation.
//!
//! Pure: the cnonce is an argument. Supports MD5, SHA-256, SHA-512-256 and their `-sess`
//! forms, `qop=auth` and `userhash`. `auth-int` is not supported.

use std::fmt;

use md5::Md5;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use sha2::{Digest as _, Sha256, Sha512_256};

use super::hex;

/// A parsed `WWW-Authenticate: Digest` challenge.
#[derive(Debug, Clone, PartialEq)]
pub struct Challenge {
    pub realm: String,
    pub nonce: String,
    pub opaque: Option<String>,
    /// As sent by the server; `MD5` when absent.
    pub algorithm: String,
    /// Offered `qop` values; empty when the server sent none.
    pub qop: Vec<String>,
    pub userhash: bool,
}

/// Why a challenge cannot be answered. Never carries credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DigestError {
    UnsupportedAlgorithm(String),
    /// The server offered only `auth-int`, or `-sess` without `qop`.
    UnsupportedQop,
}

impl fmt::Display for DigestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DigestError::UnsupportedAlgorithm(a) => {
                write!(f, "Digest algorithm `{a}` is not supported")
            }
            DigestError::UnsupportedQop => {
                f.write_str("Digest quality of protection is not supported (only qop=auth)")
            }
        }
    }
}

impl std::error::Error for DigestError {}

#[derive(Clone, Copy)]
enum Hash {
    Md5,
    Sha256,
    Sha512_256,
    /// Test only: the first 256 bits of SHA-512, which is what RFC 7616 section 3.9.2's
    /// example values were computed with (see the test there).
    #[cfg(test)]
    Sha512Truncated,
}

impl Hash {
    /// The hash and whether the algorithm is a `-sess` form.
    fn of(algorithm: &str) -> Option<(Hash, bool)> {
        let upper = algorithm.to_ascii_uppercase();
        let (base, sess) = match upper.strip_suffix("-SESS") {
            Some(b) => (b, true),
            None => (upper.as_str(), false),
        };
        let hash = match base {
            "MD5" => Hash::Md5,
            "SHA-256" => Hash::Sha256,
            "SHA-512-256" => Hash::Sha512_256,
            #[cfg(test)]
            "SHA-512-TRUNCATED" => Hash::Sha512Truncated,
            _ => return None,
        };
        Some((hash, sess))
    }

    fn hex(self, input: &str) -> String {
        let bytes = input.as_bytes();
        match self {
            Hash::Md5 => hex(&Md5::digest(bytes)),
            Hash::Sha256 => hex(&Sha256::digest(bytes)),
            Hash::Sha512_256 => hex(&Sha512_256::digest(bytes)),
            #[cfg(test)]
            Hash::Sha512Truncated => hex(&sha2::Sha512::digest(bytes)[..32]),
        }
    }
}

/// Parses the first Digest challenge in one `WWW-Authenticate` value.
pub fn parse(value: &str) -> Option<Challenge> {
    let lower = value.to_ascii_lowercase();
    let start = lower.match_indices("digest").map(|(i, _)| i).find(|&i| {
        let before_ok = value[..i].trim_end_matches([' ', '\t']).is_empty()
            || value[..i].trim_end_matches([' ', '\t']).ends_with(',');
        before_ok && value[i + 6..].starts_with([' ', '\t'])
    })?;
    let mut challenge = Challenge {
        realm: String::new(),
        nonce: String::new(),
        opaque: None,
        algorithm: "MD5".into(),
        qop: Vec::new(),
        userhash: false,
    };
    let mut have_nonce = false;
    for (name, val) in params(&value[start + 6..]) {
        match name.to_ascii_lowercase().as_str() {
            "realm" => challenge.realm = val,
            "nonce" => {
                challenge.nonce = val;
                have_nonce = true;
            }
            "opaque" => challenge.opaque = Some(val),
            "algorithm" => challenge.algorithm = val,
            "qop" => {
                challenge.qop = val
                    .split(',')
                    .map(|q| q.trim().to_ascii_lowercase())
                    .filter(|q| !q.is_empty())
                    .collect();
            }
            "userhash" => challenge.userhash = val.eq_ignore_ascii_case("true"),
            _ => {}
        }
    }
    have_nonce.then_some(challenge)
}

/// `name=value` pairs (value bare or quoted) up to the first item that is not one.
fn params(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = s;
    loop {
        rest = rest.trim_start_matches([' ', '\t', ',']);
        let Some(eq) = rest.find('=') else { break };
        let name = &rest[..eq];
        if name.is_empty() || name.contains([' ', '\t', ',', '"']) {
            break;
        }
        rest = rest[eq + 1..].trim_start_matches([' ', '\t']);
        let mut val = String::new();
        if let Some(quoted) = rest.strip_prefix('"') {
            let mut chars = quoted.char_indices();
            let mut end = quoted.len();
            while let Some((i, c)) = chars.next() {
                match c {
                    '\\' => {
                        if let Some((_, n)) = chars.next() {
                            val.push(n);
                        }
                    }
                    '"' => {
                        end = i;
                        break;
                    }
                    _ => val.push(c),
                }
            }
            rest = quoted.get(end + 1..).unwrap_or("");
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            val = rest[..end].trim().to_owned();
            rest = &rest[end..];
        }
        out.push((name.to_owned(), val));
    }
    out
}

/// Picks the first Digest challenge, in server order, whose algorithm is supported.
///
/// `Ok(None)` when no value holds a Digest challenge.
pub fn select(values: &[&str]) -> Result<Option<Challenge>, DigestError> {
    let challenges: Vec<Challenge> = values.iter().filter_map(|v| parse(v)).collect();
    if let Some(c) = challenges.iter().find(|c| Hash::of(&c.algorithm).is_some()) {
        return Ok(Some(c.clone()));
    }
    match challenges.first() {
        Some(c) => Err(DigestError::UnsupportedAlgorithm(c.algorithm.clone())),
        None => Ok(None),
    }
}

fn quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if matches!(c, '"' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// Builds the `Authorization` header value answering `challenge`.
///
/// `uri` is the request target (path and query); `cnonce` is a fresh random string.
pub fn respond(
    challenge: &Challenge,
    method: &str,
    uri: &str,
    username: &str,
    password: &str,
    cnonce: &str,
) -> Result<String, DigestError> {
    let (hash, sess) = Hash::of(&challenge.algorithm)
        .ok_or_else(|| DigestError::UnsupportedAlgorithm(challenge.algorithm.clone()))?;
    let use_qop = if challenge.qop.is_empty() {
        if sess {
            return Err(DigestError::UnsupportedQop);
        }
        false
    } else if challenge.qop.iter().any(|q| q == "auth") {
        true
    } else {
        return Err(DigestError::UnsupportedQop);
    };
    const NC: &str = "00000001";
    let realm = &challenge.realm;
    let nonce = &challenge.nonce;
    let mut ha1 = hash.hex(&format!("{username}:{realm}:{password}"));
    if sess {
        ha1 = hash.hex(&format!("{ha1}:{nonce}:{cnonce}"));
    }
    let ha2 = hash.hex(&format!("{method}:{uri}"));
    let response = if use_qop {
        hash.hex(&format!("{ha1}:{nonce}:{NC}:{cnonce}:auth:{ha2}"))
    } else {
        hash.hex(&format!("{ha1}:{nonce}:{ha2}"))
    };

    let mut parts = Vec::new();
    if challenge.userhash {
        parts.push(format!(
            "username={}",
            quoted(&hash.hex(&format!("{username}:{realm}")))
        ));
    } else if username.is_ascii() {
        parts.push(format!("username={}", quoted(username)));
    } else {
        parts.push(format!(
            "username*=UTF-8''{}",
            utf8_percent_encode(username, NON_ALPHANUMERIC)
        ));
    }
    parts.push(format!("realm={}", quoted(realm)));
    parts.push(format!("uri={}", quoted(uri)));
    parts.push(format!("algorithm={}", challenge.algorithm));
    parts.push(format!("nonce={}", quoted(nonce)));
    if use_qop {
        parts.push(format!("nc={NC}"));
        parts.push(format!("cnonce={}", quoted(cnonce)));
        parts.push("qop=auth".to_owned());
    }
    parts.push(format!("response={}", quoted(&response)));
    if let Some(opaque) = &challenge.opaque {
        parts.push(format!("opaque={}", quoted(opaque)));
    }
    if challenge.userhash {
        parts.push("userhash=true".to_owned());
    }
    Ok(format!("Digest {}", parts.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Vectors: RFC 7616 section 3.9.1 (SHA-256 and MD5) and 3.9.2 (SHA-512-256, userhash).
    // Header lines wrapped in the RFC are joined here.

    const NONCE_1: &str = "7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v";
    const OPAQUE_1: &str = "FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS";
    const CNONCE_1: &str = "f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ";

    fn challenge_1(algorithm: &str) -> String {
        format!(
            "Digest realm=\"http-auth@example.org\", qop=\"auth, auth-int\", \
             algorithm={algorithm}, nonce=\"{NONCE_1}\", opaque=\"{OPAQUE_1}\""
        )
    }

    #[test]
    fn rfc7616_3_9_1_md5() {
        let c = parse(&challenge_1("MD5")).unwrap();
        let got = respond(
            &c,
            "GET",
            "/dir/index.html",
            "Mufasa",
            "Circle of Life",
            CNONCE_1,
        )
        .unwrap();
        let want = format!(
            "Digest username=\"Mufasa\", realm=\"http-auth@example.org\", \
             uri=\"/dir/index.html\", algorithm=MD5, nonce=\"{NONCE_1}\", nc=00000001, \
             cnonce=\"{CNONCE_1}\", qop=auth, \
             response=\"8ca523f5e9506fed4657c9700eebdbec\", opaque=\"{OPAQUE_1}\""
        );
        assert_eq!(got, want);
    }

    #[test]
    fn rfc7616_3_9_1_sha256() {
        let c = parse(&challenge_1("SHA-256")).unwrap();
        let got = respond(
            &c,
            "GET",
            "/dir/index.html",
            "Mufasa",
            "Circle of Life",
            CNONCE_1,
        )
        .unwrap();
        let want = format!(
            "Digest username=\"Mufasa\", realm=\"http-auth@example.org\", \
             uri=\"/dir/index.html\", algorithm=SHA-256, nonce=\"{NONCE_1}\", nc=00000001, \
             cnonce=\"{CNONCE_1}\", qop=auth, \
             response=\"753927fa0e85d155564e2e272a28d1802ca10daf4496794697cf8db5856cb6c1\", \
             opaque=\"{OPAQUE_1}\""
        );
        assert_eq!(got, want);
    }

    /// RFC 7616 section 3.9.2. The RFC's example values were computed with the first 256
    /// bits of SHA-512, not with SHA-512/256 (different initial values), so they cannot be
    /// reproduced with the real algorithm. This test runs the RFC inputs through the same
    /// code with the truncated hash and checks every value in the RFC's header; the real
    /// SHA-512/256 is checked separately against the NIST "abc" vector.
    #[test]
    fn rfc7616_3_9_2_userhash_with_the_rfcs_truncated_sha512() {
        let c = parse(
            "Digest realm=\"api@example.org\", qop=\"auth\", algorithm=SHA-512-TRUNCATED, \
             nonce=\"5TsQWLVdgBdmrQ0XsxbDODV+57QdFR34I9HAbC/RVvkK\", \
             opaque=\"HRPCssKJSGjCrkzDg8OhwpzCiGPChXYjwrI2QmXDnsOS\", \
             charset=UTF-8, userhash=true",
        )
        .unwrap();
        assert!(c.userhash);
        let got = respond(
            &c,
            "GET",
            "/doe.json",
            "J\u{e4}s\u{f8}n Doe",
            "Secret, or not?",
            "NTg6RKcb9boFIAS3KrFK9BGeh+iDa/sm6jUMp2wds69v",
        )
        .unwrap()
        .replace("SHA-512-TRUNCATED", "SHA-512-256");
        let want = "Digest username=\"488869477bf257147b804c45308cd62ac4e25eb717b12b298c79e62dcea254ec\", \
             realm=\"api@example.org\", uri=\"/doe.json\", algorithm=SHA-512-256, \
             nonce=\"5TsQWLVdgBdmrQ0XsxbDODV+57QdFR34I9HAbC/RVvkK\", nc=00000001, \
             cnonce=\"NTg6RKcb9boFIAS3KrFK9BGeh+iDa/sm6jUMp2wds69v\", qop=auth, \
             response=\"ae66e67d6b427bd3f120414a82e4acff38e8ecd9101d6c861229025f607a79dd\", \
             opaque=\"HRPCssKJSGjCrkzDg8OhwpzCiGPChXYjwrI2QmXDnsOS\", userhash=true";
        assert_eq!(got, want);
    }

    /// NIST FIPS 180-4 example: SHA-512/256("abc").
    #[test]
    fn sha512_256_matches_the_nist_abc_vector() {
        assert_eq!(
            Hash::Sha512_256.hex("abc"),
            "53048e2681941ef99b2e29b76b4c7dabe4c2d0c634fc6d46e0e2f13107e7af23"
        );
    }

    #[test]
    fn auth_int_alone_is_an_error() {
        let c = parse("Digest realm=\"r\", nonce=\"n\", qop=\"auth-int\"").unwrap();
        assert_eq!(
            respond(&c, "GET", "/", "u", "p", "c"),
            Err(DigestError::UnsupportedQop)
        );
    }

    #[test]
    fn auth_is_chosen_when_offered_with_auth_int() {
        let c = parse("Digest realm=\"r\", nonce=\"n\", qop=\"auth-int, auth\"").unwrap();
        assert!(
            respond(&c, "GET", "/", "u", "p", "c")
                .unwrap()
                .contains("qop=auth,")
        );
    }

    #[test]
    fn unknown_algorithm_is_an_error_and_select_prefers_a_supported_one() {
        let bad = "Digest realm=\"r\", nonce=\"n\", algorithm=SHA-999";
        let good = "Digest realm=\"r\", nonce=\"n\", algorithm=SHA-256";
        assert_eq!(
            select(&[bad]),
            Err(DigestError::UnsupportedAlgorithm("SHA-999".into()))
        );
        assert_eq!(select(&[bad, good]).unwrap().unwrap().algorithm, "SHA-256");
        assert_eq!(select(&["Basic realm=\"x\""]), Ok(None));
    }

    #[test]
    fn digest_is_found_after_another_scheme_in_one_value() {
        let c = parse("Basic realm=\"b\", Digest realm=\"d\", nonce=\"n\"").unwrap();
        assert_eq!(c.realm, "d");
    }

    #[test]
    fn sess_form_changes_the_response() {
        let plain = parse("Digest realm=\"r\", nonce=\"n\", qop=auth").unwrap();
        let sess = parse("Digest realm=\"r\", nonce=\"n\", qop=auth, algorithm=MD5-sess").unwrap();
        let a = respond(&plain, "GET", "/", "u", "p", "c").unwrap();
        let b = respond(&sess, "GET", "/", "u", "p", "c").unwrap();
        assert_ne!(a.split("response=").nth(1), b.split("response=").nth(1));
        assert!(b.contains("algorithm=MD5-sess"));
    }

    #[test]
    fn quotes_in_the_username_are_escaped() {
        let c = parse("Digest realm=\"r\", nonce=\"n\", qop=auth").unwrap();
        let h = respond(&c, "GET", "/", "a\"b", "p", "c").unwrap();
        assert!(h.contains("username=\"a\\\"b\""));
    }

    #[test]
    fn error_messages_carry_no_credentials() {
        let c = parse("Digest realm=\"r\", nonce=\"n\", qop=\"auth-int\"").unwrap();
        let e = respond(&c, "GET", "/", "alice", "hunter2", "c").unwrap_err();
        assert!(!e.to_string().contains("hunter2"));
    }
}
