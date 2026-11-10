//! YAML input for the OpenAPI importer, read into `serde_json::Value`.
//!
//! The parser expands aliases itself with a budget of 100,000 nodes; this
//! module adds the depth and expanded-bytes checks the parser lacks.

use saphyr::{LoadableYamlNode, Scalar, Yaml};
use serde_json::{Map, Number, Value};

use super::{ImportError, MAX_DEPTH, check_size};

/// Parse one YAML document into a JSON value.
pub fn parse(bytes: &[u8]) -> Result<Value, ImportError> {
    check_size(bytes.len())?;
    let text = std::str::from_utf8(bytes)
        .map_err(|_| ImportError::Parse("input is not valid UTF-8".into()))?;
    check_depth(text)?;
    check_alias_bytes(text)?;
    let docs = Yaml::load_from_str(text).map_err(|e| {
        let msg = e.to_string();
        if msg.contains("alias expansion exceeded") {
            ImportError::TooLarge("YAML alias expansion over 100,000 nodes".into())
        } else {
            ImportError::Parse(msg)
        }
    })?;
    let doc = docs
        .into_iter()
        .next()
        .ok_or_else(|| ImportError::Parse("empty YAML document".into()))?;
    to_value(&doc, 0)
}

/// Total bytes alias expansion may produce, estimated before parsing.
const MAX_ALIAS_BYTES: u64 = 64 * 1024 * 1024;

enum Tok<'a> {
    Open,
    Close,
    /// A `? ` complex-key indicator.
    Complex,
    Anchor(&'a str),
    Alias(&'a str),
}

/// Walk one trimmed line, reporting flow brackets, complex-key indicators,
/// anchors and aliases. Quoted spans and comments are skipped, and brackets
/// or quotes only count at the start of a token, so `x: a [` is a plain
/// scalar. `flow` carries the open flow depth across lines; the callback gets
/// it as it stands after the token.
fn scan_line<'a>(line: &'a str, flow: &mut usize, f: &mut impl FnMut(Tok<'a>, usize)) {
    let mut chars = line.char_indices().peekable();
    let mut boundary = true;
    while let Some((i, c)) = chars.next() {
        let mut next_boundary = c.is_whitespace();
        if boundary {
            match c {
                '#' => break,
                '"' | '\'' => {
                    while let Some((_, d)) = chars.next() {
                        if c == '"' && d == '\\' {
                            chars.next();
                        } else if d == c {
                            break;
                        }
                    }
                    next_boundary = false;
                }
                '[' | '{' => {
                    *flow += 1;
                    f(Tok::Open, *flow);
                }
                '?' if chars.peek().is_none_or(|&(_, d)| d.is_whitespace()) => {
                    f(Tok::Complex, *flow);
                }
                '&' | '*' => {
                    let start = i + 1;
                    let mut end = start;
                    while let Some(&(j, d)) = chars.peek() {
                        if d.is_whitespace() || matches!(d, ',' | '[' | ']' | '{' | '}') {
                            break;
                        }
                        end = j + d.len_utf8();
                        chars.next();
                    }
                    if end > start {
                        let name = &line[start..end];
                        f(
                            if c == '&' {
                                Tok::Anchor(name)
                            } else {
                                Tok::Alias(name)
                            },
                            *flow,
                        );
                    }
                }
                _ => {}
            }
        }
        if matches!(c, ']' | '}') && *flow > 0 {
            *flow -= 1;
            f(Tok::Close, *flow);
        }
        boundary = next_boundary || (*flow > 0 && matches!(c, '[' | '{' | ',' | ':'));
    }
}

/// Reject text whose block indentation, `-`/`?`/`:` indicator runs, flow
/// brackets or complex keys nest too deeply.
fn check_depth(text: &str) -> Result<(), ImportError> {
    let mut indents: Vec<usize> = Vec::new();
    let mut flow = 0usize;
    let mut extra = 0usize;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if flow == 0 {
            let indent = line.len() - trimmed.len();
            while indents.last().is_some_and(|&i| i > indent) {
                indents.pop();
            }
            if indents.last().is_none_or(|&i| i < indent) {
                indents.push(indent);
            }
            // `- - ? x` nests without indentation.
            let run = trimmed
                .split_whitespace()
                .take_while(|t| matches!(*t, "-" | "?" | ":"))
                .count();
            if indents.len() + run > MAX_DEPTH + 1 {
                return Err(ImportError::TooDeep);
            }
        }
        let base = indents.len();
        let mut too_deep = false;
        scan_line(trimmed, &mut flow, &mut |tok, fl| match tok {
            Tok::Open => too_deep |= fl + extra + base > MAX_DEPTH + 1,
            Tok::Complex => {
                extra += 1;
                too_deep |= fl + extra + base > MAX_DEPTH + 1;
            }
            _ => {}
        });
        if too_deep {
            return Err(ImportError::TooDeep);
        }
        if flow == 0 {
            extra = 0;
        }
    }
    Ok(())
}

struct OpenAnchor<'a> {
    name: &'a str,
    start: usize,
    indent: usize,
    dash: bool,
    acc: u64,
}

/// Estimate the bytes alias expansion will produce and reject a document
/// over the budget. An anchor's size is the text it spans (until a later line
/// at the same or lower indent) plus the sizes of the aliases inside it, so
/// nested aliases of a large anchor are counted at their expanded size.
fn check_alias_bytes(text: &str) -> Result<(), ImportError> {
    use std::collections::HashMap;
    let too_large = || ImportError::TooLarge("YAML alias expansion over 64 MiB".into());
    let mut stack: Vec<OpenAnchor<'_>> = Vec::new();
    let mut sizes: HashMap<&str, u64> = HashMap::new();
    let mut total = 0u64;
    let mut flow = 0usize;
    let mut offset = 0usize;
    fn close<'a>(stack: &mut Vec<OpenAnchor<'a>>, sizes: &mut HashMap<&'a str, u64>, end: usize) {
        if let Some(top) = stack.pop() {
            sizes.insert(top.name, (end - top.start) as u64 + top.acc);
            if let Some(parent) = stack.last_mut() {
                parent.acc = parent.acc.saturating_add(top.acc);
            }
        }
    }
    for raw in text.split_inclusive('\n') {
        let line_start = offset;
        offset += raw.len();
        let line = raw.trim_end_matches(['\n', '\r']);
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - trimmed.len();
        let dash = trimmed == "-" || trimmed.starts_with("- ");
        if flow == 0 {
            while stack
                .last()
                .is_some_and(|t| t.indent > indent || (t.indent == indent && (t.dash || !dash)))
            {
                close(&mut stack, &mut sizes, line_start);
            }
        }
        let mut over = false;
        scan_line(trimmed, &mut flow, &mut |tok, _| match tok {
            Tok::Anchor(name) => stack.push(OpenAnchor {
                name,
                start: line_start,
                indent,
                dash,
                acc: 0,
            }),
            Tok::Alias(name) => {
                let size = stack
                    .iter()
                    .rev()
                    .find(|t| t.name == name)
                    .map(|t| (line_start - t.start) as u64 + t.acc)
                    .or_else(|| sizes.get(name).copied())
                    .unwrap_or(0);
                total = total.saturating_add(size);
                if let Some(top) = stack.last_mut() {
                    top.acc = top.acc.saturating_add(size);
                }
                over |= total > MAX_ALIAS_BYTES;
            }
            _ => {}
        });
        if over {
            return Err(too_large());
        }
    }
    while !stack.is_empty() {
        close(&mut stack, &mut sizes, text.len());
    }
    Ok(())
}

fn key_string(y: &Yaml<'_>) -> String {
    match y {
        Yaml::Value(Scalar::String(s)) => s.to_string(),
        Yaml::Value(Scalar::Integer(i)) => i.to_string(),
        Yaml::Value(Scalar::Boolean(b)) => b.to_string(),
        Yaml::Value(Scalar::FloatingPoint(f)) => f.0.to_string(),
        Yaml::Representation(s, _, _) => s.to_string(),
        _ => String::new(),
    }
}

fn to_value(y: &Yaml<'_>, depth: usize) -> Result<Value, ImportError> {
    if depth > MAX_DEPTH + 1 {
        return Err(ImportError::TooDeep);
    }
    Ok(match y {
        Yaml::Value(Scalar::Null) => Value::Null,
        Yaml::Value(Scalar::Boolean(b)) => Value::Bool(*b),
        Yaml::Value(Scalar::Integer(i)) => Value::Number((*i).into()),
        Yaml::Value(Scalar::FloatingPoint(f)) => {
            Number::from_f64(f.0).map_or(Value::Null, Value::Number)
        }
        Yaml::Value(Scalar::String(s)) => Value::String(s.to_string()),
        Yaml::Representation(s, _, _) => Value::String(s.to_string()),
        Yaml::Sequence(seq) => Value::Array(
            seq.iter()
                .map(|v| to_value(v, depth + 1))
                .collect::<Result<_, _>>()?,
        ),
        Yaml::Mapping(map) => {
            let mut out = Map::new();
            for (k, v) in map {
                out.insert(key_string(k), to_value(v, depth + 1)?);
            }
            Value::Object(out)
        }
        Yaml::Tagged(_, inner) => to_value(inner, depth)?,
        Yaml::Alias(_) | Yaml::BadValue => Value::Null,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars_and_nesting() {
        let v = parse(b"a: 1\nb: [x, true, null]\nc:\n  d: 'q'\n200: ok\n").unwrap();
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"][1], true);
        assert_eq!(v["c"]["d"], "q");
        assert_eq!(v["200"], "ok");
    }

    #[test]
    fn alias_bomb_is_too_large() {
        let mut s = String::from("a0: &a0 [x, x, x, x, x, x, x, x, x, x]\n");
        for i in 1..12 {
            let prev = format!("*a{}", i - 1);
            let items = vec![prev; 10].join(", ");
            s.push_str(&format!("a{i}: &a{i} [{items}]\n"));
        }
        let start = std::time::Instant::now();
        assert!(matches!(parse(s.as_bytes()), Err(ImportError::TooLarge(_))));
        assert!(start.elapsed().as_secs() < 2);
    }

    #[test]
    fn deep_flow_is_too_deep() {
        let s = format!("{}1{}", "[".repeat(200), "]".repeat(200));
        assert!(matches!(parse(s.as_bytes()), Err(ImportError::TooDeep)));
    }

    #[test]
    fn deep_block_is_too_deep() {
        let mut s = String::new();
        for i in 0..100 {
            s.push_str(&" ".repeat(i));
            s.push_str("k:\n");
        }
        assert!(matches!(parse(s.as_bytes()), Err(ImportError::TooDeep)));
    }

    fn on_small_stack<F: FnOnce() + Send + 'static>(f: F) {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(f)
            .unwrap()
            .join()
            .expect("parse overflowed a 2 MiB stack");
    }

    #[test]
    fn deep_complex_keys_are_too_deep() {
        on_small_stack(|| {
            for s in [
                "- ? ".repeat(2000) + "1\n",
                "? ".repeat(100_000) + "1",
                format!("{}1{}", "{? ".repeat(2000), "}".repeat(2000)),
            ] {
                assert!(matches!(parse(s.as_bytes()), Err(ImportError::TooDeep)));
            }
        });
    }

    #[test]
    fn unbalanced_bracket_in_scalar_keeps_depth_check() {
        let mut s = String::from("x: a [\n");
        for i in 0..100 {
            s.push_str(&" ".repeat(i));
            s.push_str("k:\n");
        }
        on_small_stack(move || {
            assert!(matches!(parse(s.as_bytes()), Err(ImportError::TooDeep)));
        });
    }

    #[test]
    fn large_aliased_anchor_is_too_large() {
        let mut s = format!("x-big: &b \"{}\"\nlist:\n", "a".repeat(100 * 1024));
        for _ in 0..700 {
            s.push_str("  - *b\n");
        }
        let start = std::time::Instant::now();
        assert!(matches!(parse(s.as_bytes()), Err(ImportError::TooLarge(_))));
        assert!(start.elapsed().as_secs() < 2);
    }

    #[test]
    fn small_aliases_still_parse() {
        let v = parse(b"base: &b {a: 1}\nuse:\n  - *b\n  - *b\n").unwrap();
        assert_eq!(v["use"][1]["a"], 1);
    }

    #[test]
    fn malformed_is_error() {
        assert!(parse(b"a: [1, 2\n").is_err());
        assert!(parse(b"\xff\xfe").is_err());
        assert!(parse(b"").is_err());
    }
}
