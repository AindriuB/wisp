//! Content-type detection and JSON pretty-printing.

use wisp_core::Header;

pub const PRETTY_LIMIT: u64 = 5 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    Json,
    Xml,
    Html,
    Text,
    Binary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrettyError {
    pub offset: usize,
    pub message: String,
}

pub fn content_kind(headers: &[Header], sniff: &[u8]) -> ContentKind {
    let ct = headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("content-type"))
        .map(|h| {
            h.value
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        });
    if let Some(ct) = ct.as_deref() {
        let kind = if ct == "application/json" || ct.ends_with("+json") {
            Some(ContentKind::Json)
        } else if ct == "text/xml" || ct == "application/xml" || ct.ends_with("+xml") {
            Some(ContentKind::Xml)
        } else if ct == "text/html" {
            Some(ContentKind::Html)
        } else if ct.starts_with("text/") {
            Some(ContentKind::Text)
        } else {
            None
        };
        if let Some(k) = kind {
            return k;
        }
    }
    sniff_kind(sniff)
}

fn sniff_kind(sniff: &[u8]) -> ContentKind {
    if sniff.contains(&0) {
        return ContentKind::Binary;
    }
    // A prefix may end mid-character; only reject errors that are not truncation.
    if let Err(e) = std::str::from_utf8(sniff)
        && e.error_len().is_some()
    {
        return ContentKind::Binary;
    }
    let sniff = sniff.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(sniff);
    let first = sniff
        .iter()
        .copied()
        .find(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'));
    match first {
        Some(b'{') | Some(b'[') => ContentKind::Json,
        Some(b'<') => {
            let rest = &sniff[sniff.iter().position(|&b| b == b'<').unwrap_or(0)..];
            let head: Vec<u8> = rest.iter().take(15).map(u8::to_ascii_lowercase).collect();
            if head.starts_with(b"<!doctype html") || head.starts_with(b"<html") {
                ContentKind::Html
            } else {
                ContentKind::Xml
            }
        }
        _ => ContentKind::Text,
    }
}

fn err<T>(offset: usize, message: &str) -> Result<T, PrettyError> {
    Err(PrettyError {
        offset,
        message: message.to_string(),
    })
}

struct Scanner<'a> {
    b: &'a [u8],
    i: usize,
}

impl Scanner<'_> {
    fn ws(&mut self) {
        while matches!(self.b.get(self.i), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.i += 1;
        }
    }

    /// Consume a string token starting at the opening quote; returns its byte range.
    fn string(&mut self) -> Result<(usize, usize), PrettyError> {
        let start = self.i;
        self.i += 1;
        loop {
            match self.b.get(self.i) {
                None => return err(self.b.len(), "unterminated string"),
                Some(b'"') => {
                    self.i += 1;
                    return Ok((start, self.i));
                }
                Some(b'\\') => match self.b.get(self.i + 1) {
                    Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => self.i += 2,
                    Some(b'u') => {
                        for k in 2..6 {
                            if !matches!(self.b.get(self.i + k), Some(c) if c.is_ascii_hexdigit()) {
                                return err(
                                    (self.i + k).min(self.b.len()),
                                    "invalid unicode escape",
                                );
                            }
                        }
                        self.i += 6;
                    }
                    Some(_) => return err(self.i + 1, "invalid escape"),
                    None => return err(self.b.len(), "unterminated string"),
                },
                Some(c) if *c < 0x20 => return err(self.i, "control character in string"),
                Some(_) => self.i += 1,
            }
        }
    }

    fn digits(&mut self) -> usize {
        let s = self.i;
        while matches!(self.b.get(self.i), Some(b'0'..=b'9')) {
            self.i += 1;
        }
        self.i - s
    }

    fn number(&mut self) -> Result<(usize, usize), PrettyError> {
        let start = self.i;
        if self.b.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        match self.b.get(self.i) {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            None => return err(self.b.len(), "truncated number"),
            _ => return err(self.i, "invalid number"),
        }
        if self.b.get(self.i) == Some(&b'.') {
            self.i += 1;
            if self.digits() == 0 {
                return err(self.i.min(self.b.len()), "invalid number");
            }
        }
        if matches!(self.b.get(self.i), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.b.get(self.i), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if self.digits() == 0 {
                return err(self.i.min(self.b.len()), "invalid number");
            }
        }
        Ok((start, self.i))
    }

    fn literal(&mut self, word: &[u8]) -> Result<(usize, usize), PrettyError> {
        let start = self.i;
        if self.b[start..].starts_with(word) {
            self.i += word.len();
            Ok((start, self.i))
        } else if word.starts_with(&self.b[start..]) {
            err(self.b.len(), "truncated literal")
        } else {
            err(start, "invalid literal")
        }
    }
}

#[derive(Clone, Copy)]
enum Mode {
    Item,
    Key,
    After,
}

const MAX_INDENT_LEVELS: usize = 32;

fn output_budget(input_len: usize) -> usize {
    (32 * 1024 * 1024).max(input_len.saturating_mul(4))
}

const SPACES: &str = "                                                                ";
const _: () = assert!(SPACES.len() == MAX_INDENT_LEVELS * 2);

fn newline(out: &mut String, depth: usize) {
    out.push('\n');
    let n = depth.min(MAX_INDENT_LEVELS) * 2;
    out.push_str(&SPACES[..n]);
}

/// Re-indent JSON text token by token, keeping every token byte-identical.
pub fn pretty_json(input: &[u8]) -> Result<String, PrettyError> {
    let bom = if input.starts_with(&[0xEF, 0xBB, 0xBF]) {
        3
    } else {
        0
    };
    let text = match std::str::from_utf8(input) {
        Ok(t) => t,
        Err(e) => return err(e.valid_up_to(), "invalid UTF-8"),
    };
    let mut sc = Scanner {
        b: text.as_bytes(),
        i: bom,
    };
    let mut out = String::with_capacity(input.len() + input.len() / 4);
    let mut stack: Vec<u8> = Vec::new();
    let mut mode = Mode::Item;
    let budget = output_budget(input.len());
    loop {
        if out.len() > budget {
            return err(sc.i.min(sc.b.len()), "output too large to pretty-print");
        }
        sc.ws();
        let c = sc.b.get(sc.i).copied();
        match mode {
            Mode::Item => {
                let Some(c) = c else {
                    return err(sc.b.len(), "unexpected end of input");
                };
                let tok = match c {
                    b'{' | b'[' => {
                        let close = if c == b'{' { b'}' } else { b']' };
                        sc.i += 1;
                        sc.ws();
                        if sc.b.get(sc.i) == Some(&close) {
                            sc.i += 1;
                            out.push(c as char);
                            out.push(close as char);
                            mode = Mode::After;
                        } else {
                            stack.push(c);
                            out.push(c as char);
                            newline(&mut out, stack.len());
                            mode = if c == b'{' { Mode::Key } else { Mode::Item };
                        }
                        continue;
                    }
                    b'"' => sc.string()?,
                    b'-' | b'0'..=b'9' => sc.number()?,
                    b't' => sc.literal(b"true")?,
                    b'f' => sc.literal(b"false")?,
                    b'n' => sc.literal(b"null")?,
                    _ => return err(sc.i, "unexpected character"),
                };
                out.push_str(&text[tok.0..tok.1]);
                mode = Mode::After;
            }
            Mode::Key => {
                if c != Some(b'"') {
                    return if c.is_none() {
                        err(sc.b.len(), "unexpected end of input")
                    } else {
                        err(sc.i, "expected string key")
                    };
                }
                let tok = sc.string()?;
                out.push_str(&text[tok.0..tok.1]);
                sc.ws();
                match sc.b.get(sc.i) {
                    Some(b':') => sc.i += 1,
                    None => return err(sc.b.len(), "unexpected end of input"),
                    Some(_) => return err(sc.i, "expected ':'"),
                }
                out.push_str(": ");
                mode = Mode::Item;
            }
            Mode::After => {
                let Some(top) = stack.last().copied() else {
                    if c.is_some() {
                        return err(sc.i, "trailing characters");
                    }
                    return Ok(out);
                };
                match c {
                    None => return err(sc.b.len(), "unexpected end of input"),
                    Some(b',') => {
                        sc.i += 1;
                        out.push(',');
                        newline(&mut out, stack.len());
                        mode = if top == b'{' { Mode::Key } else { Mode::Item };
                    }
                    Some(x) if (top == b'{' && x == b'}') || (top == b'[' && x == b']') => {
                        sc.i += 1;
                        stack.pop();
                        newline(&mut out, stack.len());
                        out.push(x as char);
                    }
                    Some(_) => return err(sc.i, "unbalanced or unexpected character"),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(n: &str, v: &str) -> Header {
        Header {
            name: n.into(),
            value: v.into(),
        }
    }

    fn pj(s: &str) -> String {
        pretty_json(s.as_bytes()).unwrap()
    }

    #[test]
    fn indents_with_two_spaces() {
        assert_eq!(
            pj(r#"{"a":1,"b":[1,2],"c":{},"d":[]}"#),
            "{\n  \"a\": 1,\n  \"b\": [\n    1,\n    2\n  ],\n  \"c\": {},\n  \"d\": []\n}"
        );
    }

    #[test]
    fn tokens_are_preserved() {
        let out = pj(r#"[ 1.0 , 1e5, -0, "a\u00e9\n\/", {"z":null,"a":true,"m":false} ]"#);
        for t in ["1.0", "1e5", "-0", r#""a\u00e9\n\/""#] {
            assert!(out.contains(t), "{t}");
        }
        assert!(out.find("\"z\"").unwrap() < out.find("\"a\"").unwrap());
        assert!(out.find("\"a\"").unwrap() < out.find("\"m\"").unwrap());
    }

    #[test]
    fn scalars_and_bom() {
        assert_eq!(pj("  42 "), "42");
        let mut b = vec![0xEF, 0xBB, 0xBF];
        b.extend_from_slice(b"[1]");
        assert_eq!(pretty_json(&b).unwrap(), "[\n  1\n]");
    }

    #[test]
    fn errors_report_offsets() {
        for bad in [
            "",
            "{",
            "[1,",
            "{\"a\"",
            "{\"a\":",
            "[1] x",
            "{}}",
            "[1}",
            "]",
            "tru",
            "trux",
            "nul",
            "01",
            "1.",
            "-",
            "1e",
            "\"abc",
            "\"\\x\"",
            "[1 2]",
            "{1:2}",
            "{\"a\" 1}",
            "[,]",
            "[1,]",
            "\"a\nb\"",
        ] {
            assert!(pretty_json(bad.as_bytes()).is_err(), "{bad:?}");
        }
        assert_eq!(pretty_json(b"[1] x").unwrap_err().offset, 4);
        assert_eq!(pretty_json(b"[1,").unwrap_err().offset, 3);
        assert!(pretty_json(&[b'[', 0xFF, b']']).is_err());
    }

    #[test]
    fn deep_nesting_is_iterative() {
        let n = 10_000;
        let s = format!("{}{}", "[".repeat(n), "]".repeat(n));
        let out = pretty_json(s.as_bytes()).unwrap();
        assert!(out.len() > n && out.len() <= output_budget(s.len()));
        let s = format!("{}1{}", "{\"a\":".repeat(n), "}".repeat(n));
        let out = pretty_json(s.as_bytes()).unwrap();
        assert!(out.len() <= output_budget(s.len()));
    }

    #[test]
    fn hostile_nesting_stays_within_budget() {
        let n = 32_768;
        let s = format!("{}{}", "[".repeat(n), "]".repeat(n));
        let out = pretty_json(s.as_bytes()).unwrap();
        assert!(out.len() <= output_budget(s.len()));
    }

    #[test]
    fn output_over_budget_is_rejected() {
        let n = 300_000;
        let s = format!("{}{}", "[".repeat(n), "]".repeat(n));
        let e = pretty_json(s.as_bytes()).unwrap_err();
        assert_eq!(e.message, "output too large to pretty-print");
        assert!(e.offset > n);
    }

    #[test]
    fn short_row_document_uses_larger_budget() {
        let mut s = String::from(r#"{"data":{"rows":["#);
        let mut first = true;
        while s.len() < 2_000_000 {
            if !first {
                s.push(',');
            }
            first = false;
            s.push_str("[1,2,3]");
        }
        s.push_str("]}}");
        let out = pretty_json(s.as_bytes()).unwrap();
        assert!(out.len() > 4 * s.len());
        assert!(out.len() <= output_budget(s.len()));
    }

    #[test]
    fn indent_is_capped_at_32_levels() {
        let n = 40;
        let s = format!("{}1{}", "[".repeat(n), "]".repeat(n));
        let out = pretty_json(s.as_bytes()).unwrap();
        let max = out.lines().map(|l| l.len() - l.trim_start().len()).max();
        assert_eq!(max, Some(64));
        // Up to 32 levels: exact 2-space-per-level indentation.
        let s = format!("{}1{}", "[".repeat(32), "]".repeat(32));
        let out = pretty_json(s.as_bytes()).unwrap();
        assert!(out.contains(&format!("\n{}1\n", " ".repeat(64))));
    }

    #[test]
    fn five_mib_document() {
        let mut s = String::from("[");
        let mut i = 0;
        while s.len() < 5 * 1024 * 1024 {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!(
                r#"{{"id":{i},"name":"item {i}","tags":["a","b"]}}"#
            ));
            i += 1;
        }
        s.push(']');
        let out = pretty_json(s.as_bytes()).unwrap();
        assert!(out.len() > s.len());
    }

    #[test]
    fn kind_from_content_type() {
        let k = |v: &str| content_kind(&[h("CONTENT-type", v)], b"\x00");
        assert_eq!(k("application/json; charset=utf-8"), ContentKind::Json);
        assert_eq!(k("application/vnd.api+json"), ContentKind::Json);
        assert_eq!(k("text/xml"), ContentKind::Xml);
        assert_eq!(k("application/xml"), ContentKind::Xml);
        assert_eq!(k("image/svg+xml"), ContentKind::Xml);
        assert_eq!(k("Text/HTML"), ContentKind::Html);
        assert_eq!(k("text/plain"), ContentKind::Text);
        assert_eq!(k("application/octet-stream"), ContentKind::Binary);
    }

    #[test]
    fn kind_from_sniffing() {
        let k = |b: &[u8]| content_kind(&[], b);
        assert_eq!(k(b"  \n{\"a\":1}"), ContentKind::Json);
        assert_eq!(k(b"[1]"), ContentKind::Json);
        assert_eq!(k(b"\xEF\xBB\xBF{\"a\":1}"), ContentKind::Json);
        assert_eq!(k(b"<?xml version=\"1.0\"?>"), ContentKind::Xml);
        assert_eq!(k(b"<!DOCTYPE html>"), ContentKind::Html);
        assert_eq!(k(b"<html>"), ContentKind::Html);
        assert_eq!(k(b"hello"), ContentKind::Text);
        assert_eq!(k(b"ab\x00c"), ContentKind::Binary);
        assert_eq!(k(&[0xFF, 0xFE, 0x41]), ContentKind::Binary);
        assert_eq!(k("é".as_bytes().get(..1).unwrap()), ContentKind::Text);
    }
}
