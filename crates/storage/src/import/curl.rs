//! Import one curl command line as a single request.

use std::path::PathBuf;

use wisp_core::Method;
use wisp_core::request::{
    Auth, Body, KeyValue, Part, PartValue, Request, RequestSettings, TextBody,
};

use super::{
    ImportError, ImportItem, ImportResult, check_size, method_name, parse_method, parse_query,
    percent_encode, request_name, split_query,
};

/// Short options that take an argument and are not imported.
const IGNORED_SHORT_WITH_ARG: &str = "oAemxwETKy";
/// Long options that take an argument and are not imported.
const IGNORED_LONG_WITH_ARG: &[&str] = &[
    "output",
    "user-agent",
    "referer",
    "max-time",
    "connect-timeout",
    "proxy",
    "cacert",
    "cert",
    "key",
    "retry",
    "write-out",
    "upload-file",
    "config",
    "resolve",
    "limit-rate",
];

/// Split a shell-like command line into words.
fn tokenize(s: &str) -> Result<Vec<String>, ImportError> {
    let unterminated = || ImportError::Parse("unterminated quote".into());
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_tok = false;
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            c if c.is_whitespace() => {
                if in_tok {
                    out.push(std::mem::take(&mut cur));
                    in_tok = false;
                }
            }
            '\\' => match it.next() {
                Some('\n') => {}
                Some('\r') if it.peek() == Some(&'\n') => {
                    it.next();
                }
                Some(n) => {
                    cur.push(n);
                    in_tok = true;
                }
                None => {}
            },
            '\'' => {
                in_tok = true;
                loop {
                    match it.next() {
                        Some('\'') => break,
                        Some(n) => cur.push(n),
                        None => return Err(unterminated()),
                    }
                }
            }
            '"' => {
                in_tok = true;
                loop {
                    match it.next() {
                        Some('"') => break,
                        Some('\\') => match it.next() {
                            Some('\n') => {}
                            Some(n @ ('"' | '\\' | '$' | '`')) => cur.push(n),
                            Some(n) => {
                                cur.push('\\');
                                cur.push(n);
                            }
                            None => return Err(unterminated()),
                        },
                        Some(n) => cur.push(n),
                        None => return Err(unterminated()),
                    }
                }
            }
            '$' if it.peek() == Some(&'\'') => {
                it.next();
                in_tok = true;
                ansi_c(&mut it, &mut cur).ok_or_else(unterminated)?;
            }
            c => {
                cur.push(c);
                in_tok = true;
            }
        }
    }
    if in_tok {
        out.push(cur);
    }
    Ok(out)
}

/// Read the body of a `$'...'` string up to the closing quote.
fn ansi_c(it: &mut std::iter::Peekable<std::str::Chars<'_>>, out: &mut String) -> Option<()> {
    loop {
        match it.next()? {
            '\'' => return Some(()),
            '\\' => match it.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'a' => out.push('\x07'),
                'b' => out.push('\x08'),
                'e' | 'E' => out.push('\x1b'),
                'f' => out.push('\x0c'),
                'v' => out.push('\x0b'),
                'x' => push_code(it, out, 16, 2),
                'u' => push_code(it, out, 16, 4),
                'U' => push_code(it, out, 16, 8),
                c @ '0'..='7' => {
                    let mut v = c.to_digit(8).unwrap_or(0);
                    for _ in 0..2 {
                        match it.peek().and_then(|d| d.to_digit(8)) {
                            Some(d) => {
                                v = v * 8 + d;
                                it.next();
                            }
                            None => break,
                        }
                    }
                    out.push(char::from_u32(v).unwrap_or('\u{fffd}'));
                }
                c => out.push(c),
            },
            c => out.push(c),
        }
    }
}

fn push_code(
    it: &mut std::iter::Peekable<std::str::Chars<'_>>,
    out: &mut String,
    radix: u32,
    max: usize,
) {
    let mut v: u32 = 0;
    let mut n = 0;
    while n < max {
        match it.peek().and_then(|d| d.to_digit(radix)) {
            Some(d) => {
                v = v.saturating_mul(radix).saturating_add(d);
                it.next();
                n += 1;
            }
            None => break,
        }
    }
    if n > 0 {
        out.push(char::from_u32(v).unwrap_or('\u{fffd}'));
    }
}

#[derive(Default)]
struct Flags {
    method: Option<String>,
    url: Option<String>,
    headers: Vec<String>,
    data: Vec<String>,
    form: Vec<String>,
    user: Option<String>,
    cookie: Option<String>,
    insecure: bool,
    location: bool,
    get: bool,
    head: bool,
    warnings: Vec<String>,
}

/// Parse a curl command line into one request.
pub fn parse(input: &str) -> Result<ImportResult, ImportError> {
    check_size(input.len())?;
    let tokens = tokenize(input)?;
    let mut args = tokens.into_iter().peekable();
    if args.peek().is_some_and(|t| t == "curl") {
        args.next();
    }
    let mut f = Flags::default();
    let mut rest_positional = false;
    while let Some(arg) = args.next() {
        let need = |args: &mut std::iter::Peekable<std::vec::IntoIter<String>>, opt: &str| {
            args.next()
                .ok_or_else(|| ImportError::Parse(format!("`{opt}` needs an argument")))
        };
        if rest_positional || !arg.starts_with('-') || arg == "-" {
            set_url(&mut f, arg);
        } else if arg == "--" {
            rest_positional = true;
        } else if let Some(long) = arg.strip_prefix("--") {
            let long = long.to_string();
            match long.as_str() {
                "request" => f.method = Some(need(&mut args, &arg)?),
                "header" => f.headers.push(need(&mut args, &arg)?),
                "data" | "data-ascii" | "data-raw" | "data-binary" => {
                    let v = need(&mut args, &arg)?;
                    add_data(&mut f, &long, v);
                }
                "data-urlencode" => {
                    let v = need(&mut args, &arg)?;
                    add_urlencoded(&mut f, v);
                }
                "form" => f.form.push(need(&mut args, &arg)?),
                "user" => f.user = Some(need(&mut args, &arg)?),
                "cookie" => f.cookie = Some(need(&mut args, &arg)?),
                "url" => set_url(&mut f, need(&mut args, &arg)?),
                "insecure" => f.insecure = true,
                "location" | "location-trusted" => f.location = true,
                "get" => f.get = true,
                "head" => f.head = true,
                l if IGNORED_LONG_WITH_ARG.contains(&l) => {
                    let v = need(&mut args, &arg)?;
                    f.warnings
                        .push(format!("option `{arg} {v}` is not imported"));
                }
                _ => f.warnings.push(format!("option `{arg}` is not imported")),
            }
        } else {
            let chars: Vec<char> = arg[1..].chars().collect();
            let mut i = 0;
            while i < chars.len() {
                let c = chars[i];
                let takes_arg = "XHdFubA".contains(c) || IGNORED_SHORT_WITH_ARG.contains(c);
                let value = if takes_arg {
                    let attached: String = chars[i + 1..].iter().collect();
                    i = chars.len();
                    if attached.is_empty() {
                        Some(need(&mut args, &format!("-{c}"))?)
                    } else {
                        Some(attached)
                    }
                } else {
                    i += 1;
                    None
                };
                match (c, value) {
                    ('X', Some(v)) => f.method = Some(v),
                    ('H', Some(v)) => f.headers.push(v),
                    ('d', Some(v)) => add_data(&mut f, "data", v),
                    ('F', Some(v)) => f.form.push(v),
                    ('u', Some(v)) => f.user = Some(v),
                    ('b', Some(v)) => f.cookie = Some(v),
                    ('k', None) => f.insecure = true,
                    ('L', None) => f.location = true,
                    ('G', None) => f.get = true,
                    ('I', None) => f.head = true,
                    (c, Some(v)) => f
                        .warnings
                        .push(format!("option `-{c} {v}` is not imported")),
                    (c, None) => f.warnings.push(format!("option `-{c}` is not imported")),
                }
            }
        }
    }
    build(f)
}

fn set_url(f: &mut Flags, url: String) {
    if f.url.is_none() {
        f.url = Some(url);
    } else {
        f.warnings
            .push(format!("extra URL `{url}` is not imported"));
    }
}

fn add_data(f: &mut Flags, kind: &str, v: String) {
    if kind != "data-raw" && v.starts_with('@') {
        f.warnings.push(format!(
            "`{v}` reads the body from a file; body not imported"
        ));
    } else {
        f.data.push(v);
    }
}

fn add_urlencoded(f: &mut Flags, v: String) {
    if let Some((name, content)) = v.split_once('=') {
        if name.is_empty() {
            f.data.push(percent_encode(content));
        } else {
            f.data.push(format!("{name}={}", percent_encode(content)));
        }
    } else if v.contains('@') {
        f.warnings
            .push(format!("`{v}` reads the value from a file; not imported"));
    } else {
        f.data.push(percent_encode(&v));
    }
}

fn build(mut f: Flags) -> Result<ImportResult, ImportError> {
    let url_raw = f
        .url
        .take()
        .ok_or_else(|| ImportError::Parse("no URL in the curl command".into()))?;
    let (url, mut params) = split_query(url_raw.trim());

    let mut method = if f.head { Method::Head } else { Method::Get };
    let has_body = (!f.data.is_empty() && !f.get) || !f.form.is_empty();
    if has_body {
        method = Method::Post;
    }
    if let Some(m) = &f.method {
        match parse_method(m) {
            Some(parsed) => method = parsed,
            None => f.warnings.push(format!(
                "method `{m}` is not supported; using {}",
                method_name(method)
            )),
        }
    }
    let name = request_name(method_name(method), &url);

    let mut headers = Vec::new();
    for h in &f.headers {
        match h.split_once(':') {
            Some((n, v)) if !n.trim().is_empty() => {
                if !v.trim().is_empty() {
                    headers.push(KeyValue {
                        name: n.trim().to_string(),
                        value: v.trim().to_string(),
                        enabled: true,
                    });
                }
            }
            _ => match h.strip_suffix(';') {
                Some(n) if !n.trim().is_empty() => headers.push(KeyValue {
                    name: n.trim().to_string(),
                    value: String::new(),
                    enabled: true,
                }),
                _ => f
                    .warnings
                    .push(format!("header `{h}` is malformed and was skipped")),
            },
        }
    }
    if let Some(c) = &f.cookie {
        if c.contains('=') {
            headers.push(KeyValue {
                name: "Cookie".into(),
                value: c.clone(),
                enabled: true,
            });
        } else {
            f.warnings
                .push(format!("cookie file `{c}` is not imported"));
        }
    }
    let content_type = headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("content-type"))
        .map(|h| h.value.to_ascii_lowercase());

    let mut body = Body::None;
    if !f.form.is_empty() {
        if !f.data.is_empty() {
            f.warnings
                .push("both -d and -F given; -d data is not imported".into());
        }
        let mut parts = Vec::new();
        for spec in &f.form {
            let Some((n, v)) = spec.split_once('=') else {
                f.warnings
                    .push(format!("form field `{spec}` has no `=`; skipped"));
                continue;
            };
            let v = v.split(";type=").next().unwrap_or("");
            let value = if let Some(path) = v.strip_prefix('@') {
                PartValue::File(PathBuf::from(path.split(';').next().unwrap_or(path)))
            } else if v.starts_with('<') {
                f.warnings.push(format!(
                    "form field `{n}` reads a file as text; not imported"
                ));
                continue;
            } else {
                PartValue::Text(v.to_string())
            };
            parts.push(Part {
                name: n.to_string(),
                value,
                enabled: true,
            });
        }
        body = Body::Multipart(parts);
    } else if !f.data.is_empty() {
        let joined = f.data.join("&");
        if f.get {
            params.extend(parse_query(&joined));
        } else {
            let ct = content_type.as_deref().unwrap_or("");
            let text = || TextBody {
                content: joined.clone(),
                sidecar: None,
            };
            body = if ct.contains("json") {
                Body::Json(text())
            } else if ct.contains("xml") {
                Body::Xml(text())
            } else if (ct.is_empty() || ct.contains("x-www-form-urlencoded"))
                && joined.split('&').all(|p| p.contains('='))
            {
                Body::Form(parse_query(&joined))
            } else {
                Body::Text(text())
            };
        }
    }

    let auth = match &f.user {
        Some(u) => {
            let (user, pass) = u.split_once(':').unwrap_or((u.as_str(), ""));
            Auth::Basic {
                username: user.to_string(),
                password: pass.to_string(),
            }
        }
        None => Auth::Inherit,
    };

    let request = Request {
        name: name.clone(),
        method,
        url: url.clone(),
        params,
        headers,
        auth,
        body,
        settings: RequestSettings {
            timeout: None,
            follow_redirects: f.location,
            verify_tls: !f.insecure,
        },
        ..Request::default()
    };
    let host = url
        .split_once("://")
        .map(|(_, r)| r.split('/').next().unwrap_or(r).to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "curl".into());
    let mut result = ImportResult {
        name: host,
        items: vec![ImportItem::Request {
            name: name.clone(),
            request,
        }],
        ..ImportResult::default()
    };
    for w in std::mem::take(&mut f.warnings) {
        result.warn(&name, w);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(input: &str) -> (Request, ImportResult) {
        let r = parse(input).unwrap();
        let ImportItem::Request { request, .. } = r.items[0].clone() else {
            panic!("expected request");
        };
        (request, r)
    }

    fn header<'a>(r: &'a Request, n: &str) -> Option<&'a str> {
        r.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(n))
            .map(|h| h.value.as_str())
    }

    #[test]
    fn plain_get() {
        let (r, res) = req("curl https://api.example.com/users/1");
        assert_eq!(r.method, Method::Get);
        assert_eq!(r.url, "https://api.example.com/users/1");
        assert_eq!(r.name, "GET /users/1");
        assert_eq!(res.name, "api.example.com");
        assert!(res.warnings.is_empty());
        assert!(!r.settings.follow_redirects);
        assert!(r.settings.verify_tls);
    }

    #[test]
    fn explicit_method_and_headers() {
        let (r, _) = req("curl -X PUT -H 'X-A: 1' --header \"Accept: text/plain\" http://h/x");
        assert_eq!(r.method, Method::Put);
        assert_eq!(header(&r, "x-a"), Some("1"));
        assert_eq!(header(&r, "accept"), Some("text/plain"));
    }

    #[test]
    fn attached_short_args() {
        let (r, _) = req("curl -XDELETE -H'X-B: 2' http://h/x");
        assert_eq!(r.method, Method::Delete);
        assert_eq!(header(&r, "x-b"), Some("2"));
    }

    #[test]
    fn data_defaults_to_post_and_form() {
        let (r, _) = req("curl -d 'a=1&b=x%20y' http://h/x");
        assert_eq!(r.method, Method::Post);
        let Body::Form(kv) = r.body else { panic!() };
        assert_eq!(kv[1].value, "x y");
        assert_eq!(kv.len(), 2);
    }

    #[test]
    fn json_body_by_content_type() {
        let (r, _) =
            req(r#"curl http://h/x -H "Content-Type: application/json" --data-raw '{"a":1}'"#);
        assert_eq!(r.method, Method::Post);
        let Body::Json(t) = r.body else { panic!() };
        assert_eq!(t.content, r#"{"a":1}"#);
    }

    #[test]
    fn data_variants_join() {
        for flag in ["--data", "--data-raw", "--data-binary"] {
            let (r, _) = req(&format!("curl {flag} 'k=v' {flag} 'k2=v2' http://h/x"));
            let Body::Form(kv) = r.body else { panic!() };
            assert_eq!(kv.len(), 2, "{flag}");
        }
    }

    #[test]
    fn data_urlencode() {
        let (r, _) = req("curl --data-urlencode 'q=a b&c' --data-urlencode 'x y' http://h/x");
        let Body::Text(t) = r.body else { panic!() };
        assert_eq!(t.content, "q=a%20b%26c&x%20y");
    }

    #[test]
    fn multipart_with_file() {
        let (r, _) = req("curl -F 'name=bob' -F 'f=@/tmp/a.png;type=image/png' http://h/up");
        let Body::Multipart(parts) = r.body else {
            panic!()
        };
        assert_eq!(parts[0].value, PartValue::Text("bob".into()));
        assert_eq!(parts[1].value, PartValue::File(PathBuf::from("/tmp/a.png")));
        assert_eq!(r.method, Method::Post);
    }

    #[test]
    fn basic_auth_cookie_flags() {
        let (r, _) = req("curl -u me:pw -b 'a=1; b=2' -k -L http://h/x");
        assert_eq!(
            r.auth,
            Auth::Basic {
                username: "me".into(),
                password: "pw".into()
            }
        );
        assert_eq!(header(&r, "cookie"), Some("a=1; b=2"));
        assert!(!r.settings.verify_tls);
        assert!(r.settings.follow_redirects);
    }

    #[test]
    fn get_flag_moves_data_to_query() {
        let (r, _) = req("curl -G -d 'a=1' -d 'b=2' 'http://h/x?c=3'");
        assert_eq!(r.method, Method::Get);
        assert_eq!(r.body, Body::None);
        let names: Vec<_> = r.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["c", "a", "b"]);
        assert_eq!(r.url, "http://h/x");
    }

    #[test]
    fn continuations_and_ansi_quotes() {
        let (r, _) = req("curl \\\n  -H $'X-N: a\\tb' \\\n  -d 'x=1' \\\n  http://h/x");
        assert_eq!(header(&r, "x-n"), Some("a\tb"));
        assert_eq!(r.method, Method::Post);
    }

    #[test]
    fn double_quote_escapes() {
        let (r, _) = req(r#"curl -H "X-Q: say \"hi\"" http://h/x"#);
        assert_eq!(header(&r, "x-q"), Some("say \"hi\""));
    }

    #[test]
    fn unknown_flags_warn() {
        let (_, res) = req("curl -s --compressed --retry 3 http://h/x");
        assert_eq!(res.warnings.len(), 3);
    }

    #[test]
    fn head_flag() {
        let (r, _) = req("curl -I http://h/x");
        assert_eq!(r.method, Method::Head);
    }

    #[test]
    fn errors_not_panics() {
        assert!(parse("curl").is_err());
        assert!(parse("curl 'http://h").is_err());
        assert!(parse("curl http://h -H").is_err());
        assert!(parse("").is_err());
        assert!(parse("curl $'\\").is_err());
    }

    #[test]
    fn oversize_rejected() {
        let big = format!(
            "curl http://h/{}",
            "a".repeat(super::super::MAX_INPUT_BYTES)
        );
        assert!(matches!(parse(&big), Err(ImportError::TooLarge(_))));
    }
}
