//! Import one curl command line as a single request.

use wisp_core::Method;
use wisp_core::request::{
    Auth, Body, KeyValue, Part, PartValue, Request, RequestSettings, TextBody,
};

use super::{
    ImportError, ImportItem, ImportResult, MAX_WARNINGS, SecretSink, check_size, method_name,
    parse_method, parse_query, percent_encode, request_name, split_query,
};

/// Short options that take an argument and are not imported.
/// Checked against the short options of `curl --help all` (7.84.0).
const IGNORED_SHORT_WITH_ARG: &str = "oAemxwETKycDrUzCtQPY";
/// Long options that take an argument and are not imported.
///
/// Built from `curl --help all` of curl 7.84.0 (every option shown with an
/// argument), plus a few newer ones (`ech`, `ip-tos`, `upload-flags`, ...) and
/// `proxy`. An option missing here would leave its argument to become the URL.
const IGNORED_LONG_WITH_ARG: &[&str] = &[
    "output",
    "json",
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
    "cookie-jar",
    "dump-header",
    "range",
    "proxy-user",
    "oauth2-bearer",
    "time-cond",
    "continue-at",
    "telnet-option",
    "quote",
    "ftp-port",
    "speed-limit",
    "speed-time",
    "proxy-header",
    "proxy-cacert",
    "proxy-cert",
    "proxy-key",
    "cert-type",
    "key-type",
    "pass",
    "capath",
    "ciphers",
    "interface",
    "local-port",
    "max-redirs",
    "max-filesize",
    "retry-delay",
    "retry-max-time",
    "trace",
    "trace-ascii",
    "stderr",
    "url-query",
    "form-string",
    "variable",
    "expect100-timeout",
    "dns-servers",
    "doh-url",
    "unix-socket",
    "aws-sigv4",
    "output-dir",
    "noproxy",
    "preproxy",
    "netrc-file",
    "pinnedpubkey",
    "request-target",
    "etag-save",
    "etag-compare",
    "mail-from",
    "mail-rcpt",
    "socks5",
    "socks5-hostname",
    "keepalive-time",
    "tls-max",
    "proxy-pass",
    "tlspassword",
    "tlsuser",
    "proxy-tlspassword",
    "proxy-tlsuser",
    "sasl-authzid",
    "login-options",
    "ftp-account",
    "connect-to",
    "proto",
    "hostpubsha256",
    "abstract-unix-socket",
    "alt-svc",
    "create-file-mode",
    "crlfile",
    "curves",
    "delegation",
    "dns-interface",
    "dns-ipv4-addr",
    "dns-ipv6-addr",
    "egd-file",
    "engine",
    "ftp-alternative-to-user",
    "ftp-method",
    "ftp-ssl-ccc-mode",
    "happy-eyeballs-timeout-ms",
    "hostpubmd5",
    "hsts",
    "krb",
    "libcurl",
    "mail-auth",
    "parallel-max",
    "proto-default",
    "proto-redir",
    "proxy-capath",
    "proxy-cert-type",
    "proxy-ciphers",
    "proxy-crlfile",
    "proxy-key-type",
    "proxy-pinnedpubkey",
    "proxy-service-name",
    "proxy-tls13-ciphers",
    "proxy-tlsauthtype",
    "proxy1.0",
    "pubkey",
    "random-file",
    "rate",
    "service-name",
    "socks4",
    "socks4a",
    "socks5-gssapi-service",
    "tftp-blksize",
    "tls13-ciphers",
    "tlsauthtype",
    "haproxy-clientip",
    "ip-tos",
    "vlan-priority",
    "trace-config",
    "upload-flags",
    "ssl-sessions",
    "ech",
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

impl Flags {
    /// Keep at most one more than [`MAX_WARNINGS`]; the rest are dropped here.
    fn warn(&mut self, message: String) {
        if self.warnings.len() <= MAX_WARNINGS {
            self.warnings.push(message);
        }
    }
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
            // `--expand-X` (curl 8.3+) takes the same arguments as `--X`.
            let long = long.strip_prefix("expand-").unwrap_or(long).to_string();
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
                    need(&mut args, &arg)?;
                    f.warn(format!("option `{arg}` and its argument are not imported"));
                }
                _ => {
                    let name = arg.split_once('=').map_or(arg.as_str(), |(n, _)| n);
                    f.warn(format!("option `{name}` is not imported"));
                }
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
                    (c, Some(_)) => {
                        f.warn(format!("option `-{c}` and its argument are not imported"))
                    }
                    (c, None) => f.warn(format!("option `-{c}` is not imported")),
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
        let url = url.split(['?', '#']).next().unwrap_or_default().to_string();
        let (url, _) = super::split_userinfo(&url);
        f.warn(format!("extra URL `{url}` is not imported"));
    }
}

fn add_data(f: &mut Flags, kind: &str, v: String) {
    if kind != "data-raw" && v.starts_with('@') {
        f.warn(format!(
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
        f.warn(format!("`{v}` reads the value from a file; not imported"));
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
    let (url, userinfo) = super::split_userinfo(&url);
    if userinfo.is_some() && f.user.is_some() {
        f.warn("the URL has credentials and -u was given; -u is used".into());
    }

    let mut method = if f.head { Method::Head } else { Method::Get };
    let has_body = (!f.data.is_empty() && !f.get) || !f.form.is_empty();
    if has_body {
        method = Method::Post;
    }
    if let Some(m) = &f.method {
        match parse_method(m) {
            Some(parsed) => method = parsed,
            None => f.warn(format!(
                "method `{m}` is not supported; using {}",
                method_name(method)
            )),
        }
    }
    let name = request_name(method_name(method), &url);

    let mut sink = SecretSink::default();
    let mut headers = Vec::new();
    let header_args = std::mem::take(&mut f.headers);
    for h in &header_args {
        match h.split_once(':') {
            Some((n, v)) if !n.trim().is_empty() => {
                if !v.trim().is_empty() {
                    let mut value = v.trim().to_string();
                    if super::is_credential_header(n.trim()) {
                        value = sink.secret_ref(&name, &n.trim().to_ascii_lowercase(), value);
                    }
                    headers.push(KeyValue {
                        name: n.trim().to_string(),
                        value,
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
                _ => f.warn(format!("header `{h}` is malformed and was skipped")),
            },
        }
    }
    if let Some(c) = f.cookie.take() {
        if c.contains('=') {
            headers.push(KeyValue {
                name: "Cookie".into(),
                value: sink.secret_ref(&name, "cookie", c),
                enabled: true,
            });
        } else {
            f.warn(format!("cookie file `{c}` is not imported"));
        }
    }
    let content_type = headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("content-type"))
        .map(|h| h.value.to_ascii_lowercase());

    let mut body = Body::None;
    if !f.form.is_empty() {
        if !f.data.is_empty() {
            f.warn("both -d and -F given; -d data is not imported".into());
        }
        let mut parts = Vec::new();
        let forms = std::mem::take(&mut f.form);
        for spec in &forms {
            let Some((n, v)) = spec.split_once('=') else {
                f.warn(format!("form field `{spec}` has no `=`; skipped"));
                continue;
            };
            let v = v.split(";type=").next().unwrap_or("");
            let value = if v.starts_with('@') {
                f.warn(format!(
                    "file part `{n}` is imported disabled and empty; pick the file after import"
                ));
                parts.push(Part {
                    name: n.to_string(),
                    value: PartValue::Text(String::new()),
                    enabled: false,
                });
                continue;
            } else if v.starts_with('<') {
                f.warn(format!(
                    "form field `{n}` reads a file as text; imported disabled and empty; pick the file after import"
                ));
                parts.push(Part {
                    name: n.to_string(),
                    value: PartValue::Text(String::new()),
                    enabled: false,
                });
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
                username: sink.secret_ref(&name, "username", user.to_string()),
                password: sink.secret_ref(&name, "password", pass.to_string()),
            }
        }
        None => match userinfo {
            Some((user, pass)) => Auth::Basic {
                username: sink.secret_ref(&name, "username", user),
                password: sink.secret_ref(&name, "password", pass),
            },
            None => Auth::Inherit,
        },
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
        environments: sink.into_env().into_iter().collect(),
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
        let (r, _) = req("curl -F 'name=bob' -F 'f=@assets/a.png;type=image/png' http://h/up");
        let Body::Multipart(parts) = r.body else {
            panic!()
        };
        assert_eq!(parts[0].value, PartValue::Text("bob".into()));
        assert_eq!(parts[1].value, PartValue::Text(String::new()));
        assert!(!parts[1].enabled);
        assert_eq!(r.method, Method::Post);
    }

    #[test]
    fn basic_auth_cookie_flags() {
        let (r, res) = req("curl -u me:pw -b 'a=1; b=2' -k -L http://h/x");
        assert_eq!(
            r.auth,
            Auth::Basic {
                username: "{{get_x_username}}".into(),
                password: "{{get_x_password}}".into()
            }
        );
        assert_eq!(header(&r, "cookie"), Some("{{get_x_cookie}}"));
        let secrets = &res.environments[0].secrets;
        assert!(secrets.iter().any(|s| s.value == "a=1; b=2"));
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

    #[test]
    fn arg_taking_options_do_not_swallow_the_url() {
        for cmd in [
            "curl -c jar.txt https://h/x",
            "curl -D - https://h/x",
            "curl -r 0-99 https://h/x",
            "curl -U u:p https://h/x",
            "curl -z yesterday https://h/x",
            "curl -C - https://h/x",
            "curl -t opt https://h/x",
            "curl -Q cmd https://h/x",
            "curl -P - https://h/x",
            "curl -Y 10 https://h/x",
            "curl --oauth2-bearer SECRETTOKEN https://h/x",
            "curl --cookie-jar jar.txt https://h/x",
            "curl --dump-header - https://h/x",
            "curl --range 0-9 https://h/x",
            "curl --proxy-user u:p https://h/x",
        ] {
            let (r, res) = req(cmd);
            assert_eq!(r.url, "https://h/x", "{cmd}");
            assert!(
                res.warnings.iter().all(|w| !w.message.contains("SECRET")),
                "{cmd}"
            );
        }
    }

    #[test]
    fn json_option_takes_its_argument_and_warns() {
        let (r, w) = req(r#"curl --json '{"a":1}' https://h/x"#);
        assert_eq!(r.url, "https://h/x");
        assert!(matches!(r.body, Body::None));
        assert!(w.warnings.iter().any(|m| m.message.contains("--json")));
    }

    #[test]
    fn file_paths_are_never_imported() {
        for bad in [
            "/etc/passwd",
            "~/.ssh/id_rsa",
            "../x",
            "a/../../x",
            "C:\\x",
            "\\\\host\\share\\x",
        ] {
            let (r, res) = req(&format!("curl -F 'f=@{bad}' http://h/up"));
            let Body::Multipart(parts) = r.body else {
                panic!()
            };
            assert_eq!(parts[0].value, PartValue::Text(String::new()), "{bad}");
            assert!(!parts[0].enabled);
            let w = res
                .warnings
                .iter()
                .find(|w| w.message.contains("`f`"))
                .unwrap();
            assert!(!w.message.contains(bad) && !w.at.contains(bad), "{bad}");
        }
    }

    #[test]
    fn plus_is_space_in_form_data_and_queries() {
        let (r, _) = req("curl -d 'k=a+b&j=a%2Bb' http://h/x");
        let Body::Form(kv) = r.body else { panic!() };
        assert_eq!(kv[0].value, "a b");
        assert_eq!(kv[1].value, "a+b");
        let (r, _) = req("curl -G -d 'k=a+b&j=a%2Bb' http://h/x");
        assert_eq!(r.params[0].value, "a b");
        assert_eq!(r.params[1].value, "a+b");
        let (r, _) = req("curl 'http://h/x?k=a+b&j=a%2Bb'");
        assert_eq!(r.params[0].value, "a b");
        assert_eq!(r.params[1].value, "a+b");
    }

    #[test]
    fn credentials_go_to_secrets_env() {
        let (r, res) = req(
            "curl -u bob:hunter2 -H 'Authorization: Bearer abc123' -H 'Cookie: s=zzz' http://h/x",
        );
        let Auth::Basic { username, password } = &r.auth else {
            panic!()
        };
        assert!(username.starts_with("{{") && password.starts_with("{{"));
        assert!(header(&r, "authorization").unwrap().starts_with("{{"));
        assert!(header(&r, "cookie").unwrap().starts_with("{{"));
        assert!(!format!("{:?}", r).contains("hunter2"));
        let secrets = &res.environments[0].secrets;
        for v in ["bob", "hunter2", "Bearer abc123", "s=zzz"] {
            assert!(secrets.iter().any(|s| s.value == v), "{v}");
        }
    }

    #[test]
    fn huge_flag_cluster_warnings_are_capped() {
        let mut f = Flags::default();
        for _ in 0..MAX_WARNINGS * 3 {
            f.warn("w".into());
        }
        assert_eq!(f.warnings.len(), MAX_WARNINGS + 1);
        let (_, res) = req(&format!("curl -{} http://h/x", "v".repeat(2_000_000)));
        assert_eq!(res.warnings.len(), MAX_WARNINGS + 1);
    }

    #[test]
    fn form_file_as_text_part_is_disabled_and_empty() {
        let (r, res) = req("curl -F 'n=<secret.txt' http://h/x");
        let Body::Multipart(parts) = r.body else {
            panic!()
        };
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].name, "n");
        assert_eq!(parts[0].value, PartValue::Text(String::new()));
        assert!(!parts[0].enabled);
        let w = &res.warnings[0].message;
        assert!(w.contains("`n`") && !w.contains("secret.txt"));
    }

    #[test]
    fn url_userinfo_becomes_basic_auth_secrets() {
        let (r, res) = req("curl 'http://bob:p%40ss@h/x'");
        assert_eq!(res.name, "h");
        assert_eq!(r.url, "http://h/x");
        let Auth::Basic { username, password } = &r.auth else {
            panic!()
        };
        assert!(username.starts_with("{{") && password.starts_with("{{"));
        let env = &res.environments[0];
        assert!(env.secrets.iter().any(|v| v.value == "p@ss"));
        assert!(env.secrets.iter().any(|v| v.value == "bob"));
        assert!(!format!("{r:?}{:?}", res.warnings).contains("p@ss"));
        assert!(!format!("{:?}", res.name).contains("bob"));
    }

    #[test]
    fn schemeless_url_userinfo_goes_to_secrets() {
        let (r, res) = req("curl bob:SECRETPW@h/x");
        assert!(!r.url.contains("bob:SECRETPW@") && r.url.ends_with("h/x"));
        assert!(!res.name.contains("SECRETPW") && !res.name.contains("bob:"));
        assert!(matches!(r.auth, Auth::Basic { .. }));
        assert!(
            res.environments[0]
                .secrets
                .iter()
                .any(|v| v.value == "SECRETPW")
        );
        assert!(!format!("{r:?}{:?}", res.warnings).contains("SECRETPW"));
    }

    #[test]
    fn extra_url_warning_has_no_userinfo() {
        let (_, res) = req("curl http://h/x http://bob:SECRETPW@h2/y");
        assert!(!res.warnings.is_empty());
        assert!(!format!("{:?}", res.warnings).contains("SECRETPW"));
    }

    #[test]
    fn explicit_user_beats_url_userinfo() {
        let (r, res) = req("curl -u a:b http://bob:pw@h/x");
        assert_eq!(r.url, "http://h/x");
        assert!(matches!(r.auth, Auth::Basic { .. }));
        let env = &res.environments[0];
        assert!(env.secrets.iter().all(|v| v.value != "pw"));
        assert!(res.warnings.iter().any(|w| w.message.contains("-u")));
    }

    #[test]
    fn many_credential_headers_with_huge_name_are_cheap() {
        let t = std::time::Instant::now();
        let mut cmd = format!("curl http://h/{}", "a".repeat(1024 * 1024));
        for i in 0..2000 {
            cmd.push_str(&format!(" -H 'Authorization: v{i}'"));
        }
        let r = parse(&cmd).unwrap();
        for e in &r.environments {
            assert!(e.secrets.iter().all(|v| v.name.len() < 100));
        }
        assert!(t.elapsed().as_secs() < 2);
    }

    #[test]
    fn punctuation_name_with_many_credential_headers_is_cheap() {
        let t = std::time::Instant::now();
        let mut cmd = format!("curl http://h/{}", "-".repeat(1024 * 1024));
        for i in 0..2000 {
            cmd.push_str(&format!(" -H 'Authorization: v{i}'"));
        }
        parse(&cmd).unwrap();
        assert!(t.elapsed().as_secs() < 2);
    }

    #[test]
    fn proxy_authorization_and_spaced_names_go_to_secrets() {
        let (r, res) = req(
            "curl -H 'Proxy-Authorization: Basic SECRETPA' -H ' authorization : SECRETAU' http://h/x",
        );
        assert!(!format!("{r:?}").contains("SECRET"));
        let secrets = &res.environments[0].secrets;
        assert!(secrets.iter().any(|v| v.value == "Basic SECRETPA"));
        assert!(secrets.iter().any(|v| v.value == "SECRETAU"));
    }

    #[test]
    fn credential_long_options_consume_their_argument() {
        for opt in [
            "proxy-pass",
            "proxy-user",
            "tlspassword",
            "tlsuser",
            "proxy-tlspassword",
            "proxy-tlsuser",
            "pass",
            "key",
            "sasl-authzid",
            "login-options",
            "ftp-account",
            "oauth2-bearer",
            "aws-sigv4",
            "proxy-header",
            "connect-to",
            "proto",
            "hostpubsha256",
        ] {
            for cmd in [
                format!("curl --{opt} SECRETVAL http://h/x"),
                format!("curl http://h/x --{opt} SECRETVAL"),
            ] {
                let (r, res) = req(&cmd);
                assert_eq!(r.url, "http://h/x", "{opt}");
                let all = format!("{r:?}{:?}{:?}", res.name, res.warnings);
                assert!(!all.contains("SECRETVAL"), "{opt}");
                assert!(res.environments.iter().all(|e| e.secrets.is_empty()));
            }
        }
        let (_, res) = req("curl --pass=SECRETVAL http://h/x");
        assert!(!format!("{:?}", res.warnings).contains("SECRETVAL"));
    }

    #[test]
    fn expand_options_consume_their_argument_and_route_credentials() {
        for (opt, val) in [
            ("expand-user", "bob:SECRETVAL"),
            ("expand-oauth2-bearer", "SECRETVAL"),
            ("expand-proxy-user", "bob:SECRETVAL"),
            ("expand-pass", "SECRETVAL"),
            ("expand-header", "Authorization: Bearer SECRETVAL"),
        ] {
            for cmd in [
                format!("curl --{opt} '{val}' http://h/x"),
                format!("curl http://h/x --{opt} '{val}'"),
            ] {
                let (r, res) = req(&cmd);
                assert_eq!(r.url, "http://h/x", "{opt}");
                let all = format!("{r:?}{:?}{:?}", res.name, res.warnings);
                assert!(!all.contains("SECRETVAL"), "{opt}: {all}");
            }
        }
    }

    /// Every option of `curl --help all` (7.84.0) shown with an argument.
    #[test]
    fn every_argument_taking_option_consumes_its_argument() {
        let longs = [
            "abstract-unix-socket",
            "alt-svc",
            "aws-sigv4",
            "cacert",
            "capath",
            "cert",
            "cert-type",
            "ciphers",
            "config",
            "connect-timeout",
            "connect-to",
            "continue-at",
            "cookie-jar",
            "create-file-mode",
            "crlfile",
            "curves",
            "delegation",
            "dns-interface",
            "dns-ipv4-addr",
            "dns-ipv6-addr",
            "dns-servers",
            "doh-url",
            "dump-header",
            "egd-file",
            "engine",
            "etag-compare",
            "etag-save",
            "expect100-timeout",
            "form-string",
            "ftp-account",
            "ftp-alternative-to-user",
            "ftp-method",
            "ftp-port",
            "ftp-ssl-ccc-mode",
            "happy-eyeballs-timeout-ms",
            "hostpubmd5",
            "hostpubsha256",
            "hsts",
            "interface",
            "keepalive-time",
            "key",
            "key-type",
            "krb",
            "libcurl",
            "limit-rate",
            "local-port",
            "login-options",
            "mail-auth",
            "mail-from",
            "mail-rcpt",
            "max-filesize",
            "max-redirs",
            "max-time",
            "netrc-file",
            "noproxy",
            "oauth2-bearer",
            "output",
            "output-dir",
            "parallel-max",
            "pass",
            "pinnedpubkey",
            "proto",
            "proto-default",
            "proto-redir",
            "proxy",
            "proxy-cacert",
            "proxy-capath",
            "proxy-cert",
            "proxy-cert-type",
            "proxy-ciphers",
            "proxy-crlfile",
            "proxy-header",
            "proxy-key",
            "proxy-key-type",
            "proxy-pass",
            "proxy-pinnedpubkey",
            "proxy-service-name",
            "proxy-tls13-ciphers",
            "proxy-tlsauthtype",
            "proxy-tlspassword",
            "proxy-tlsuser",
            "proxy-user",
            "proxy1.0",
            "pubkey",
            "quote",
            "random-file",
            "range",
            "rate",
            "referer",
            "request-target",
            "resolve",
            "retry",
            "retry-delay",
            "retry-max-time",
            "sasl-authzid",
            "service-name",
            "socks4",
            "socks4a",
            "socks5",
            "socks5-gssapi-service",
            "socks5-hostname",
            "speed-limit",
            "speed-time",
            "stderr",
            "telnet-option",
            "tftp-blksize",
            "time-cond",
            "tls-max",
            "tls13-ciphers",
            "tlsauthtype",
            "tlspassword",
            "tlsuser",
            "trace",
            "trace-ascii",
            "unix-socket",
            "upload-file",
            "user-agent",
            "write-out",
        ];
        for opt in longs {
            for cmd in [
                format!("curl --{opt} SECRETVAL http://h/x"),
                format!("curl http://h/x --{opt} SECRETVAL"),
            ] {
                let (r, res) = req(&cmd);
                assert_eq!(r.url, "http://h/x", "{opt}");
                let all = format!("{r:?}{:?}{:?}", res.name, res.warnings);
                assert!(!all.contains("SECRETVAL"), "{opt}: {all}");
            }
        }
        for c in "EKCcDPQrYyxtTzAwUmeo".chars() {
            for cmd in [
                format!("curl -{c} SECRETVAL http://h/x"),
                format!("curl http://h/x -{c}SECRETVAL"),
            ] {
                let (r, res) = req(&cmd);
                assert_eq!(r.url, "http://h/x", "-{c}");
                let all = format!("{r:?}{:?}{:?}", res.name, res.warnings);
                assert!(!all.contains("SECRETVAL"), "-{c}: {all}");
            }
        }
    }

    #[test]
    fn extra_url_warning_drops_query_and_userinfo() {
        let (_, res) = req("curl http://h/x 'http://bob:SECRETPW@h2/y?token=SECRETVAL#SECRETFR'");
        assert_eq!(res.warnings.len(), 1);
        let all = format!("{:?}", res.warnings);
        assert!(all.contains("h2/y"), "{all}");
        for s in ["SECRETVAL", "SECRETPW", "SECRETFR"] {
            assert!(!all.contains(s), "{all}");
        }
    }

    #[test]
    fn credential_name_headers_go_to_secrets() {
        let (r, res) = req(
            "curl -H 'X-API-Key: SECRETK' -H 'X-Auth-Token: SECRETT' -H 'Accept: text/plain' http://h/x",
        );
        assert_eq!(header(&r, "accept"), Some("text/plain"));
        assert!(!format!("{r:?}").contains("SECRET"));
        let secrets = &res.environments[0].secrets;
        for (n, v) in [("x-api-key", "SECRETK"), ("x-auth-token", "SECRETT")] {
            assert!(secrets.iter().any(|s| s.value == v), "{n}");
            let refs = wisp_core::template::var_refs(header(&r, n).unwrap());
            assert_eq!(refs.len(), 1);
            assert!(secrets.iter().any(|s| s.name == refs[0].name));
        }
    }

    #[test]
    fn odd_credential_header_names_get_resolvable_refs() {
        let (r, res) = req(
            "curl -H '{{apiKeyHeader}}: SECRETA' -H 'X-Key}}: SECRETB' -H 'X Api Key: SECRETC' http://h/x",
        );
        assert!(!format!("{r:?}").contains("SECRET"));
        let secrets = &res.environments[0].secrets;
        assert_eq!(r.headers.len(), 3);
        for (h, v) in r.headers.iter().zip(["SECRETA", "SECRETB", "SECRETC"]) {
            let refs = wisp_core::template::var_refs(&h.value);
            assert_eq!(refs.len(), 1, "{h:?}");
            assert!(
                secrets
                    .iter()
                    .any(|s| s.name == refs[0].name && s.value == v)
            );
        }
    }
}
