//! Translate a closed set of Postman test statements into assertions.
//!
//! A strict grammar, parsed in one forward pass with constant lookahead and no
//! recursion; nothing is ever evaluated. The whole script must be a sequence of
//! `pm.test(<str>, function () { <table statements> })` units (or the arrow
//! form), with only whitespace and comments between tokens. Any other byte
//! makes the whole script untranslated: nothing is skipped or guessed.

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

/// Count `pm.test(` occurrences, at least one: used when a script is not translated.
fn rough_count(src: &str) -> usize {
    src.matches("pm.test(").count().max(1)
}

/// Translate one script's text. Over the size cap nothing is parsed; a script
/// outside the grammar yields no assertions and all its tests are untranslated.
pub(super) fn translate(src: &str) -> Translated {
    let mut out = Translated::default();
    if src.trim().is_empty() {
        return out;
    }
    let parsed = if src.len() > MAX_SCRIPT_BYTES {
        None
    } else {
        script(src)
    };
    match parsed {
        Some((assertions, units)) => {
            out.assertions = assertions;
            out.total = units;
        }
        None => {
            let n = rough_count(src);
            out.total = n;
            out.untranslated = n;
        }
    }
    out
}

/// The whole script as units, or `None` if any byte is outside the grammar.
fn script(src: &str) -> Option<(Vec<Assertion>, usize)> {
    let mut cur = Cursor::new(src);
    let mut out = Vec::new();
    let mut units = 0usize;
    loop {
        cur.skip_ws();
        if cur.bad {
            return None;
        }
        if cur.at_end() {
            return Some((out, units));
        }
        test_unit(&mut cur, &mut out)?;
        units += 1;
    }
}

struct Cursor<'a> {
    b: &'a [u8],
    src: &'a str,
    pos: usize,
    /// Set when a block comment is not closed.
    bad: bool,
}

impl<'a> Cursor<'a> {
    fn new(src: &'a str) -> Self {
        Cursor {
            b: src.as_bytes(),
            src,
            pos: 0,
            bad: false,
        }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.b.len()
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.pos).copied()
    }

    /// Skip whitespace and comments. Returns whether a line terminator was crossed.
    fn skip_ws(&mut self) -> bool {
        let mut nl = false;
        loop {
            match self.peek() {
                Some(c) if c.is_ascii_whitespace() => {
                    nl |= c == b'\n' || c == b'\r';
                    self.pos += 1;
                }
                Some(b'/') if self.b.get(self.pos + 1) == Some(&b'/') => {
                    while self.peek().is_some_and(|c| c != b'\n' && c != b'\r') {
                        // U+2028 / U+2029 also end a comment in JavaScript; stop
                        // there so the next bytes fail the grammar.
                        if self
                            .src
                            .get(self.pos..)
                            .is_some_and(|r| r.starts_with('\u{2028}') || r.starts_with('\u{2029}'))
                        {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                Some(b'/') if self.b.get(self.pos + 1) == Some(&b'*') => {
                    self.pos += 2;
                    loop {
                        match self.peek() {
                            None => {
                                self.bad = true;
                                return nl;
                            }
                            Some(b'*') if self.b.get(self.pos + 1) == Some(&b'/') => {
                                self.pos += 2;
                                break;
                            }
                            Some(c) => {
                                nl |= c == b'\n' || c == b'\r';
                                self.pos += 1;
                            }
                        }
                    }
                }
                _ => return nl,
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

    /// A statement or unit ends with `;`, a line terminator, the end of input,
    /// or (when `close` is set) the closing `}`, which is not consumed.
    fn terminator(&mut self, close: bool) -> Option<()> {
        let nl = self.skip_ws();
        if self.bad {
            return None;
        }
        match self.peek() {
            Some(b';') => {
                self.pos += 1;
                Some(())
            }
            None => Some(()),
            Some(b'}') if close => Some(()),
            Some(_) if nl => Some(()),
            Some(_) => None,
        }
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
            } else if ch == '\n' || ch == '\r' {
                return None;
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
                if self.pos == digits || (self.pos - digits > 1 && self.b[digits] == b'0') {
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

/// `pm.test("name", function () { ... })` or the arrow form, one unit.
fn test_unit(cur: &mut Cursor, out: &mut Vec<Assertion>) -> Option<()> {
    cur.lit("pm.test")?;
    cur.lit("(")?;
    cur.string()?;
    cur.lit(",")?;
    if cur.lit("function").is_some() {
        cur.lit("(")?;
        cur.lit(")")?;
    } else {
        cur.lit("(")?;
        cur.lit(")")?;
        // No line terminator is allowed between `)` and `=>`.
        if cur.skip_ws() || cur.bad {
            return None;
        }
        cur.lit("=>")?;
    }
    cur.lit("{")?;
    let mut count = 0usize;
    loop {
        cur.skip_ws();
        if cur.bad {
            return None;
        }
        if cur.peek() == Some(b'}') && count > 0 {
            cur.pos += 1;
            break;
        }
        out.push(statement(cur)?);
        count += 1;
        cur.terminator(true)?;
    }
    cur.lit(")")?;
    cur.terminator(false)
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
