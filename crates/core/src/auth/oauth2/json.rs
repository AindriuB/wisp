//! A minimal reader for a token response: one JSON object, kept as top-level string and number
//! members. `serde_json` is a dev-dependency of this crate only; a token response needs none of
//! its generality, so this avoids a manifest change.

use std::collections::HashMap;

/// A top-level member value; arrays, objects, booleans and null are skipped, not kept.
#[derive(Debug, PartialEq)]
pub(crate) enum Value {
    Str(String),
    Num(f64),
    Other,
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

/// Nesting deeper than this is rejected, so hostile input cannot overflow the stack.
const MAX_DEPTH: usize = 32;

impl Reader<'_> {
    fn ws(&mut self) {
        while self.b.get(self.i).is_some_and(u8::is_ascii_whitespace) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> Option<()> {
        self.ws();
        (self.b.get(self.i) == Some(&c)).then(|| self.i += 1)
    }

    fn hex4(&mut self) -> Option<u32> {
        let h = self.b.get(self.i..self.i + 4)?;
        self.i += 4;
        u32::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()
    }

    fn string(&mut self) -> Option<String> {
        self.eat(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = *self.b.get(self.i)?;
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let e = *self.b.get(self.i)?;
                    self.i += 1;
                    let ch = match e {
                        b'"' | b'\\' | b'/' => char::from(e),
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            let cp = if (0xD800..0xDC00).contains(&hi) {
                                if self.b.get(self.i..self.i + 2) != Some(b"\\u") {
                                    return None;
                                }
                                self.i += 2;
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return None;
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else {
                                hi
                            };
                            char::from_u32(cp)?
                        }
                        _ => return None,
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                0..=0x1f => return None,
                _ => out.push(c),
            }
        }
    }

    fn number(&mut self) -> Option<f64> {
        let start = self.i;
        while self
            .b
            .get(self.i)
            .is_some_and(|c| matches!(c, b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E'))
        {
            self.i += 1;
        }
        std::str::from_utf8(&self.b[start..self.i])
            .ok()?
            .parse()
            .ok()
    }

    fn literal(&mut self, word: &[u8]) -> Option<()> {
        (self.b.get(self.i..self.i + word.len()) == Some(word)).then(|| self.i += word.len())
    }

    /// Parses and drops one value of any type.
    fn skip(&mut self, depth: usize) -> Option<()> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.ws();
        match self.b.get(self.i)? {
            b'"' => self.string().map(drop),
            b'{' => {
                self.i += 1;
                self.members(depth, |_, _| ())
            }
            b'[' => {
                self.i += 1;
                self.ws();
                if self.b.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Some(());
                }
                loop {
                    self.skip(depth + 1)?;
                    self.ws();
                    match self.b.get(self.i)? {
                        b',' => self.i += 1,
                        b']' => {
                            self.i += 1;
                            return Some(());
                        }
                        _ => return None,
                    }
                }
            }
            b't' => self.literal(b"true"),
            b'f' => self.literal(b"false"),
            b'n' => self.literal(b"null"),
            _ => self.number().map(drop),
        }
    }

    /// Reads `"key": value, ...}` after the opening brace, handing each to `each`.
    fn members(&mut self, depth: usize, mut each: impl FnMut(String, Value)) -> Option<()> {
        self.ws();
        if self.b.get(self.i) == Some(&b'}') {
            self.i += 1;
            return Some(());
        }
        loop {
            let key = self.string()?;
            self.eat(b':')?;
            self.ws();
            let value = match self.b.get(self.i)? {
                b'"' => Value::Str(self.string()?),
                b'-' | b'0'..=b'9' => Value::Num(self.number()?),
                _ => {
                    self.skip(depth + 1)?;
                    Value::Other
                }
            };
            each(key, value);
            self.ws();
            match self.b.get(self.i)? {
                b',' => self.i += 1,
                b'}' => {
                    self.i += 1;
                    return Some(());
                }
                _ => return None,
            }
        }
    }
}

/// The top-level members of a JSON object, or `None` when `body` is not exactly one object.
/// A repeated key keeps its last value.
pub(crate) fn object(body: &[u8]) -> Option<HashMap<String, Value>> {
    let mut r = Reader { b: body, i: 0 };
    r.eat(b'{')?;
    let mut map = HashMap::new();
    r.members(0, |k, v| {
        map.insert(k, v);
    })?;
    r.ws();
    (r.i == body.len()).then_some(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_strings_numbers_and_skips_the_rest() {
        let m = object(
            br#" {"a":"x\u00e9\ud83d\ude00\n","n":-1.5e1,"o":{"k":[1,{"z":null}]},"t":true,"s":"q"} "#,
        )
        .unwrap();
        assert_eq!(m["a"], Value::Str("x\u{e9}\u{1F600}\n".into()));
        assert_eq!(m["n"], Value::Num(-15.0));
        assert_eq!(m["o"], Value::Other);
        assert_eq!(m["t"], Value::Other);
        assert_eq!(m["s"], Value::Str("q".into()));
    }

    #[test]
    fn rejects_non_objects_trailing_text_and_truncation() {
        for bad in [
            &b"[1]"[..],
            b"",
            b"{\"a\":1} x",
            b"{\"a\":\"b",
            b"{\"a\":1,}",
            b"{\"a\" 1}",
            b"{\"a\":\"\\q\"}",
        ] {
            assert!(object(bad).is_none(), "{}", String::from_utf8_lossy(bad));
        }
        assert!(object(b"{}").unwrap().is_empty());
        let deep = format!("{{\"a\":{}{}}}", "[".repeat(100), "]".repeat(100));
        assert!(object(deep.as_bytes()).is_none());
    }
}
