//! Translate a closed set of Postman test statements into assertions.
//!
//! The parser is hand-written, non-recursive and linear: every step moves the
//! cursor forward, and nothing is ever evaluated. A test is translated only
//! when every statement in it is in the closed set; otherwise none of it is.

use wisp_core::assert::{Assertion, Expected, check_path};

/// Script text over this many bytes per request is not parsed.
pub(super) const MAX_SCRIPT_BYTES: usize = 64 * 1024;

/// The outcome of translating one request's test scripts.
#[derive(Debug, Default)]
pub(super) struct Translated {
    pub assertions: Vec<Assertion>,
    /// Test units found.
    pub total: usize,
    /// Test units not translated.
    pub untranslated: usize,
}

/// Count `pm.test(` occurrences, at least one: used when the text is too big to parse.
fn rough_count(src: &str) -> usize {
    src.matches("pm.test(").count().max(1)
}

/// Translate one script's text. Over the size cap nothing is parsed.
pub(super) fn translate(src: &str) -> Translated {
    let mut out = Translated::default();
    if src.trim().is_empty() {
        return out;
    }
    if src.len() > MAX_SCRIPT_BYTES {
        let n = rough_count(src);
        out.total = n;
        out.untranslated = n;
        return out;
    }
    let mut cur = Cursor::new(src);
    loop {
        cur.skip_ws();
        if cur.at_end() {
            break;
        }
        out.total += 1;
        let start = cur.pos;
        match test_unit(&mut cur) {
            Some(mut asserts) => out.assertions.append(&mut asserts),
            None => {
                out.untranslated += 1;
                if cur.exhausted || cur.give_up {
                    // A block scan ran to the end of the script; rescanning
                    // for each later unit would be quadratic. Count the rest.
                    let rest = src.get(start + 1..).unwrap_or("");
                    let n = rest.matches("pm.test(").count();
                    out.total += n;
                    out.untranslated += n;
                    break;
                }
                // Resynchronise past the failed unit; always moves forward.
                // Never move back over bytes a block scan already covered.
                cur.pos = start;
                cur.skip_statement();
                if cur.give_up {
                    let rest = src.get(start + 1..).unwrap_or("");
                    let n = rest.matches("pm.test(").count();
                    out.total += n;
                    out.untranslated += n;
                    break;
                }
                if cur.scanned_to > cur.pos {
                    let skipped = src.get(cur.pos..cur.scanned_to).unwrap_or("");
                    let n = skipped.matches("pm.test(").count();
                    out.total += n;
                    out.untranslated += n;
                    cur.pos = cur.scanned_to;
                }
            }
        }
    }
    out
}

struct Cursor<'a> {
    b: &'a [u8],
    src: &'a str,
    pos: usize,
    /// Set when `block_body` failed after scanning to the end of the input.
    exhausted: bool,
    /// Set when a `/` of uncertain meaning (or an unclosed regex) was met: the
    /// rest of the script is not translated. Each regex attempt either moves
    /// the cursor past what it scanned or ends scanning, so no byte is rescanned.
    give_up: bool,
    /// Furthest position a successful `block_body` reached.
    scanned_to: usize,
}

impl<'a> Cursor<'a> {
    fn new(src: &'a str) -> Self {
        Cursor {
            b: src.as_bytes(),
            src,
            pos: 0,
            exhausted: false,
            give_up: false,
            scanned_to: 0,
        }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.b.len()
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.pos).copied()
    }

    /// Skip whitespace and comments.
    fn skip_ws(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_ascii_whitespace() => self.pos += 1,
                Some(b'/') if self.b.get(self.pos + 1) == Some(&b'/') => {
                    while let Some(c) = self.peek() {
                        if c == b'\n' {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                Some(b'/') if self.b.get(self.pos + 1) == Some(&b'*') => {
                    self.pos += 2;
                    while self.pos < self.b.len()
                        && !(self.b[self.pos] == b'*' && self.b.get(self.pos + 1) == Some(&b'/'))
                    {
                        self.pos += 1;
                    }
                    self.pos = (self.pos + 2).min(self.b.len());
                }
                _ => break,
            }
        }
    }

    /// Skip whitespace, then require the exact text.
    fn lit(&mut self, s: &str) -> Option<()> {
        self.skip_ws();
        if self.src.get(self.pos..)?.starts_with(s) {
            self.pos += s.len();
            Some(())
        } else {
            None
        }
    }

    /// Skip a string literal starting at the cursor, without decoding it.
    fn skip_quoted(&mut self) -> bool {
        let Some(q) = self.peek() else { return false };
        self.pos += 1;
        while let Some(c) = self.peek() {
            self.pos += 1;
            if c == b'\\' {
                self.pos = (self.pos + 1).min(self.b.len());
            } else if c == q {
                return true;
            }
        }
        false
    }

    /// Skip to just after the next `;` outside any bracket or string, or to the end.
    /// Always advances by at least one byte.
    fn skip_statement(&mut self) {
        let first = self.pos;
        let mut prev = b';';
        let mut prev_pos = 0usize;
        let mut depth = 0usize;
        while let Some(c) = self.peek() {
            match c {
                b'\'' | b'"' | b'`' => {
                    if !self.skip_quoted() {
                        self.pos = self.b.len();
                    }
                    prev = b'"';
                    prev_pos = self.pos - 1;
                    continue;
                }
                b'/' if matches!(self.b.get(self.pos + 1), Some(b'/' | b'*')) => {
                    self.skip_ws();
                    continue;
                }
                b'/' => {
                    // A certain division is skipped; a certain regex start is
                    // skipped; anything else fails closed.
                    match slash_kind(self.b, prev, prev_pos) {
                        Slash::Division => {
                            prev = b'/';
                            prev_pos = self.pos;
                            self.pos += 1;
                            continue;
                        }
                        Slash::Regex if self.skip_regex() => {
                            prev = b'"';
                            prev_pos = self.pos - 1;
                            continue;
                        }
                        _ => {}
                    }
                    self.give_up = true;
                    self.pos = self.b.len();
                    break;
                }
                b'(' | b'{' | b'[' => depth += 1,
                b')' | b'}' | b']' => depth = depth.saturating_sub(1),
                b';' if depth == 0 => {
                    self.pos += 1;
                    break;
                }
                _ => {}
            }
            if !c.is_ascii_whitespace() {
                prev = c;
                prev_pos = self.pos;
            }
            self.pos += 1;
        }
        if self.pos == first && !self.at_end() {
            self.pos += 1;
        }
    }

    /// With the cursor just after an opening `{`, move to just after its matching `}`
    /// and return the body text between them.
    fn block_body(&mut self) -> Option<&'a str> {
        let r = self.block_body_inner();
        if r.is_none() {
            self.exhausted = true;
        }
        r
    }

    fn block_body_inner(&mut self) -> Option<&'a str> {
        let start = self.pos;
        let mut prev = b'{';
        let mut prev_pos = 0usize;
        let mut depth = 1usize;
        while let Some(c) = self.peek() {
            match c {
                b'\'' | b'"' | b'`' => {
                    if !self.skip_quoted() {
                        return None;
                    }
                    prev = b'"';
                    prev_pos = self.pos - 1;
                    continue;
                }
                b'/' if matches!(self.b.get(self.pos + 1), Some(b'/' | b'*')) => {
                    self.skip_ws();
                    continue;
                }
                b'/' => {
                    match slash_kind(self.b, prev, prev_pos) {
                        Slash::Division => {
                            prev = b'/';
                            prev_pos = self.pos;
                            self.pos += 1;
                            continue;
                        }
                        Slash::Regex if self.skip_regex() => {
                            prev = b'"';
                            prev_pos = self.pos - 1;
                            continue;
                        }
                        _ => {}
                    }
                    self.give_up = true;
                    return None;
                }
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        let body = self.src.get(start..self.pos)?;
                        self.pos += 1;
                        self.scanned_to = self.pos;
                        return Some(body);
                    }
                }
                _ => {}
            }
            if !c.is_ascii_whitespace() {
                prev = c;
                prev_pos = self.pos;
            }
            self.pos += 1;
        }
        None
    }

    /// Skip a regex literal starting at the `/` under the cursor. Returns false,
    /// leaving the cursor unmoved, when it does not close on the same line.
    fn skip_regex(&mut self) -> bool {
        let mut i = self.pos + 1;
        let mut class = false;
        while let Some(&c) = self.b.get(i) {
            match c {
                b'\\' => i += 1,
                b'\n' => return false,
                b'[' => class = true,
                b']' => class = false,
                b'/' if !class => {
                    self.pos = i + 1;
                    return true;
                }
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// A single- or double-quoted string with `\\ \" \' \n` escapes.
    fn string(&mut self) -> Option<String> {
        self.skip_ws();
        let q = self.peek().filter(|c| matches!(c, b'"' | b'\''))?;
        self.pos += 1;
        let mut out = String::new();
        loop {
            let rest = self.src.get(self.pos..)?;
            let ch = rest.chars().next()?;
            self.pos += ch.len_utf8();
            if ch as u32 == u32::from(q) {
                return Some(out);
            }
            if ch == '\\' {
                let esc = self.peek()?;
                self.pos += 1;
                out.push(match esc {
                    b'\\' => '\\',
                    b'"' => '"',
                    b'\'' => '\'',
                    b'n' => '\n',
                    _ => return None,
                });
            } else {
                out.push(ch);
            }
        }
    }

    /// Integer, float, string, `true` or `false`.
    fn literal(&mut self) -> Option<Expected> {
        self.skip_ws();
        match self.peek()? {
            b'"' | b'\'' => self.string().map(Expected::Str),
            b'-' | b'0'..=b'9' => {
                let start = self.pos;
                if self.peek() == Some(b'-') {
                    self.pos += 1;
                }
                let digits = self.pos;
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.pos += 1;
                }
                if self.pos == digits {
                    return None;
                }
                let mut float = false;
                if self.peek() == Some(b'.') {
                    float = true;
                    self.pos += 1;
                    let frac = self.pos;
                    while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                        self.pos += 1;
                    }
                    if self.pos == frac {
                        return None;
                    }
                }
                if self
                    .peek()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
                {
                    return None;
                }
                let t = &self.src[start..self.pos];
                if float {
                    t.parse().ok().map(Expected::Float)
                } else {
                    t.parse().ok().map(Expected::Int)
                }
            }
            _ => {
                let rest = &self.src[self.pos..];
                for (word, v) in [("true", true), ("false", false)] {
                    if let Some(after) = rest.strip_prefix(word)
                        && !after.starts_with(|c: char| c.is_alphanumeric() || c == '_')
                    {
                        self.pos += word.len();
                        return Some(Expected::Bool(v));
                    }
                }
                None
            }
        }
    }

    /// A `.name` / `[n]` tail after `pm.response.json()`, as a `$` path.
    fn json_tail(&mut self) -> Option<String> {
        let mut path = String::from("$");
        while let Some(c) = self.peek() {
            match c {
                b'.' => {
                    path.push('.');
                    self.pos += 1;
                    let s = self.pos;
                    while self
                        .peek()
                        .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
                    {
                        self.pos += 1;
                    }
                    if self.pos == s || &self.src[s..self.pos] == "length" {
                        return None;
                    }
                    path.push_str(&self.src[s..self.pos]);
                }
                b'[' => {
                    let s = self.pos;
                    self.pos += 1;
                    let d = self.pos;
                    while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                        self.pos += 1;
                    }
                    if self.pos == d || self.peek() != Some(b']') {
                        return None;
                    }
                    self.pos += 1;
                    path.push_str(self.src.get(s..self.pos)?);
                }
                _ => break,
            }
            if path.len() > 512 {
                return None;
            }
        }
        (path.len() > 1 && check_path(&path).is_ok()).then_some(path)
    }
}

/// Keywords after which a `/` certainly starts a regex literal.
const REGEX_KEYWORDS: [&[u8]; 9] = [
    b"return",
    b"typeof",
    b"void",
    b"case",
    b"throw",
    b"in",
    b"delete",
    b"instanceof",
    b"new",
];

/// Identifiers whose following `/` is ambiguous (division or regex).
const AMBIGUOUS_WORDS: [&[u8]; 3] = [b"of", b"yield", b"await"];

/// What a `/` means, judged from the significant byte before it.
enum Slash {
    Regex,
    Division,
    Ambiguous,
}

/// Classify a `/` after the significant byte `prev` (at `prev_pos`). Only certain
/// cases are `Regex` or `Division`; the rest (after `)` or `}`, `of`, `yield`,
/// `await`, or another operator) are `Ambiguous` and the caller fails closed.
fn slash_kind(b: &[u8], prev: u8, prev_pos: usize) -> Slash {
    if matches!(
        prev,
        b'(' | b',' | b'=' | b':' | b'[' | b'!' | b'&' | b'|' | b'?' | b'{' | b';'
    ) {
        return Slash::Regex;
    }
    // `=>`
    if prev == b'>' {
        return if prev_pos > 0 && b[prev_pos - 1] == b'=' {
            Slash::Regex
        } else {
            Slash::Ambiguous
        };
    }
    // A number, an identifier, `]` or a closing quote (or a finished regex).
    if prev == b']' || prev == b'"' {
        return Slash::Division;
    }
    let is_id = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'$';
    if !is_id(prev) {
        return Slash::Ambiguous;
    }
    // Look back over at most one longer-than-any-keyword identifier run.
    let end = prev_pos + 1;
    let mut s = end;
    while s > 0 && end - s <= 10 && is_id(b[s - 1]) {
        s -= 1;
    }
    if s > 0 && (is_id(b[s - 1]) || b[s - 1] == b'.') {
        return Slash::Division;
    }
    let word = &b[s..end];
    if REGEX_KEYWORDS.contains(&word) {
        Slash::Regex
    } else if AMBIGUOUS_WORDS.contains(&word) {
        Slash::Ambiguous
    } else {
        Slash::Division
    }
}

/// `pm.test("name", function () { ... })` or the arrow form, then an optional `;`.
fn test_unit(cur: &mut Cursor) -> Option<Vec<Assertion>> {
    cur.lit("pm.test")?;
    cur.lit("(")?;
    cur.string()?;
    cur.lit(",")?;
    cur.skip_ws();
    if cur.lit("function").is_some() {
        cur.lit("(")?;
        cur.lit(")")?;
    } else {
        cur.lit("(")?;
        cur.lit(")")?;
        cur.lit("=>")?;
    }
    cur.lit("{")?;
    let body = cur.block_body()?;
    cur.lit(")")?;
    cur.skip_ws();
    if cur.peek() == Some(b';') {
        cur.pos += 1;
    }
    statements(body)
}

/// Every statement of a test body, or `None` if any is outside the closed set.
fn statements(body: &str) -> Option<Vec<Assertion>> {
    let mut cur = Cursor::new(body);
    let mut out = Vec::new();
    loop {
        cur.skip_ws();
        if cur.at_end() {
            return Some(out);
        }
        out.push(statement(&mut cur)?);
        cur.skip_ws();
        if cur.peek() == Some(b';') {
            cur.pos += 1;
        }
    }
}

fn status_of(e: Expected) -> Option<Assertion> {
    Assertion::from_parts("status", None, "equals", Some(e)).ok()
}

fn statement(cur: &mut Cursor) -> Option<Assertion> {
    cur.lit("pm.")?;
    if cur.lit("response.to.have.").is_some() {
        if cur.lit("status").is_some() {
            cur.lit("(")?;
            let n = cur.literal()?;
            cur.lit(")")?;
            return match n {
                Expected::Int(_) => status_of(n),
                _ => None,
            };
        }
        cur.lit("header")?;
        cur.lit("(")?;
        let name = cur.string()?;
        cur.skip_ws();
        if cur.peek() == Some(b',') {
            cur.pos += 1;
            let v = cur.string()?;
            cur.lit(")")?;
            return Assertion::from_parts("header", Some(&name), "equals", Some(Expected::Str(v)))
                .ok();
        }
        cur.lit(")")?;
        return Assertion::from_parts("header", Some(&name), "exists", None).ok();
    }
    cur.lit("expect")?;
    cur.lit("(")?;
    cur.lit("pm.response.")?;
    enum Subject {
        Code,
        Header(String),
        Json(String),
    }
    let subject = if cur.lit("code").is_some() {
        Subject::Code
    } else if cur.lit("headers.get").is_some() {
        cur.lit("(")?;
        let n = cur.string()?;
        cur.lit(")")?;
        Subject::Header(n)
    } else {
        cur.lit("json")?;
        cur.lit("(")?;
        cur.lit(")")?;
        Subject::Json(cur.json_tail()?)
    };
    cur.lit(")")?;
    cur.lit(".to.")?;
    let verb = if cur.lit("eql").is_some() {
        "eql"
    } else if cur.lit("equal").is_some() {
        "equal"
    } else {
        cur.lit("include")?;
        "include"
    };
    cur.lit("(")?;
    let v = cur.literal()?;
    cur.lit(")")?;
    match subject {
        Subject::Code => match (verb, &v) {
            ("eql" | "equal", Expected::Int(_)) => status_of(v),
            _ => None,
        },
        Subject::Header(n) => {
            let op = match verb {
                "eql" => "equals",
                "include" => "contains",
                _ => return None,
            };
            match v {
                Expected::Str(_) => Assertion::from_parts("header", Some(&n), op, Some(v)).ok(),
                _ => None,
            }
        }
        Subject::Json(path) => match verb {
            "eql" => Assertion::from_parts("json", Some(&path), "equals", Some(v)).ok(),
            _ => None,
        },
    }
}
