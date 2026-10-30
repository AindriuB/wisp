//! Set-Cookie header parsing for the response Cookies tab.

use wisp_core::Header;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseCookie {
    pub name: String,
    pub value: String,
    pub domain: Option<String>,
    pub path: Option<String>,
    pub expires: Option<String>,
    pub max_age: Option<i64>,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: Option<String>,
}

pub fn parse_set_cookies(headers: &[Header]) -> Vec<ResponseCookie> {
    headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case("set-cookie"))
        .filter_map(|h| parse_one(&h.value))
        .collect()
}

fn parse_one(line: &str) -> Option<ResponseCookie> {
    let mut parts = line.split(';');
    let (name, value) = parts.next()?.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let mut c = ResponseCookie {
        name: name.to_string(),
        value: value.trim().to_string(),
        domain: None,
        path: None,
        expires: None,
        max_age: None,
        secure: false,
        http_only: false,
        same_site: None,
    };
    for attr in parts {
        let (k, v) = match attr.split_once('=') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => (attr.trim(), ""),
        };
        let some = |v: &str| (!v.is_empty()).then(|| v.to_string());
        match k.to_ascii_lowercase().as_str() {
            "domain" => c.domain = some(v),
            "path" => c.path = some(v),
            "expires" => c.expires = some(v),
            "max-age" => c.max_age = parse_max_age(v),
            "secure" => c.secure = true,
            "httponly" => c.http_only = true,
            "samesite" => c.same_site = some(v),
            _ => {}
        }
    }
    Some(c)
}

/// RFC 6265 §5.2.2: ignore unless the first char is a digit or `-` and the rest are digits;
/// out-of-range values saturate.
fn parse_max_age(v: &str) -> Option<i64> {
    let (neg, digits) = match v.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, v),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(match v.parse::<i64>() {
        Ok(n) => n,
        Err(_) if neg => i64::MIN,
        Err(_) => i64::MAX,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sc(v: &str) -> Header {
        Header {
            name: "Set-Cookie".into(),
            value: v.into(),
        }
    }

    #[test]
    fn parses_all_attributes() {
        let hs = [
            sc(
                "id = a=b ; DOMAIN=example.com; path=/x; Expires=Wed, 21 Oct 2026 07:28:00 GMT; MAX-AGE=60; SECURE; httponly; SameSite=Lax; Foo=bar",
            ),
            Header {
                name: "SET-COOKIE".into(),
                value: "k=v".into(),
            },
            Header {
                name: "X".into(),
                value: "no=pe".into(),
            },
        ];
        let c = parse_set_cookies(&hs);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].name, "id");
        assert_eq!(c[0].value, "a=b");
        assert_eq!(c[0].domain.as_deref(), Some("example.com"));
        assert_eq!(c[0].path.as_deref(), Some("/x"));
        assert_eq!(
            c[0].expires.as_deref(),
            Some("Wed, 21 Oct 2026 07:28:00 GMT")
        );
        assert_eq!(c[0].max_age, Some(60));
        assert!(c[0].secure && c[0].http_only);
        assert_eq!(c[0].same_site.as_deref(), Some("Lax"));
        assert_eq!(c[1].name, "k");
        assert!(!c[1].secure && c[1].max_age.is_none());
    }

    #[test]
    fn skips_bad_headers_and_ignores_bad_max_age() {
        let c = parse_set_cookies(&[sc("=v"), sc("novalue"), sc(" =x"), sc("a=1; Max-Age=abc")]);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].max_age, None);
    }

    #[test]
    fn max_age_follows_rfc_6265() {
        let age = |v: &str| parse_one(&format!("a=b; Max-Age={v}")).and_then(|c| c.max_age);
        assert_eq!(age("+5"), None);
        assert_eq!(age("99999999999999999999"), Some(i64::MAX));
        assert_eq!(age("-99999999999999999999"), Some(i64::MIN));
        assert_eq!(age("-5"), Some(-5));
        assert_eq!(age("60"), Some(60));
        assert_eq!(age("5x"), None);
    }
}
