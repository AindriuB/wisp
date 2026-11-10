//! YAML input for the OpenAPI importer, read into `serde_json::Value`.
//!
//! Events from `saphyr_parser` are folded into a value iteratively, enforcing
//! the depth cap and an expanded-bytes budget (aliases included) as they
//! arrive.

use std::collections::HashMap;

use saphyr::Scalar;
use saphyr_parser::{Event, Parser};
use serde_json::{Map, Number, Value};

use super::{ImportError, MAX_DEPTH, check_size};

/// YAML input cap; the parser buffers flow collections, so it is lower than
/// the JSON cap.
const MAX_YAML_BYTES: usize = 2 * 1024 * 1024;

/// Total expanded bytes a document may produce, aliases included.
const MAX_EXPANDED_BYTES: u64 = 64 * 1024 * 1024;
/// Fixed cost charged per node, so empty nodes still count.
const NODE_COST: u64 = 32;

fn too_large() -> ImportError {
    ImportError::TooLarge("YAML expanded over 64 MiB".into())
}

/// An open container. `size` and `height` accumulate over its children.
enum Frame {
    Seq(Vec<Value>),
    Map {
        map: Map<String, Value>,
        key: Option<String>,
    },
}

struct Open {
    frame: Frame,
    anchor: usize,
    size: u64,
    height: usize,
}

struct Anchored {
    value: Value,
    size: u64,
    height: usize,
}

fn key_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

/// Parse one YAML document into a JSON value.
///
/// The document is read as a stream of parser events, iteratively. Container
/// depth and the expanded size of every anchor are tracked as events arrive,
/// so a hostile document fails before it is built.
pub fn parse(bytes: &[u8]) -> Result<Value, ImportError> {
    check_size(bytes.len())?;
    if bytes.len() > MAX_YAML_BYTES {
        return Err(ImportError::TooLarge(format!(
            "{} bytes of YAML (limit {MAX_YAML_BYTES})",
            bytes.len()
        )));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| ImportError::Parse("input is not valid UTF-8".into()))?;
    let mut parser = Parser::new_from_str(text);
    let mut stack: Vec<Open> = Vec::new();
    let mut anchors: HashMap<usize, Anchored> = HashMap::new();
    let mut total: u64 = 0;
    let mut root: Option<Value> = None;

    while let Some(ev) = parser.next_event() {
        let (ev, _) = ev.map_err(|e| {
            let msg = e.to_string();
            // The scanner's own nesting limit for flow collections.
            if msg.contains("recursion limit exceeded") {
                ImportError::TooDeep
            } else {
                ImportError::Parse(msg)
            }
        })?;
        // A finished node: (value, anchor id, size, height).
        let done: (Value, usize, u64, usize) = match ev {
            Event::DocumentEnd if root.is_some() => break,
            Event::StreamEnd => break,
            Event::Scalar(v, style, aid, tag) => {
                let size = (v.len() as u64).saturating_add(NODE_COST);
                total = total.saturating_add(size);
                if total > MAX_EXPANDED_BYTES {
                    return Err(too_large());
                }
                let scalar = Scalar::parse_from_cow_and_metadata(v.clone(), style, tag.as_ref())
                    .unwrap_or(Scalar::String(v));
                let value = match scalar {
                    Scalar::Null => Value::Null,
                    Scalar::Boolean(b) => Value::Bool(b),
                    Scalar::Integer(i) => Value::Number(i.into()),
                    Scalar::FloatingPoint(f) => {
                        Number::from_f64(f.0).map_or(Value::Null, Value::Number)
                    }
                    Scalar::String(s) => Value::String(s.into_owned()),
                };
                (value, aid, size, 0)
            }
            Event::Alias(id) => {
                let a = anchors
                    .get(&id)
                    .ok_or_else(|| ImportError::Parse("unknown YAML alias".into()))?;
                if stack.len().saturating_add(a.height) > MAX_DEPTH {
                    return Err(ImportError::TooDeep);
                }
                total = total.saturating_add(a.size);
                if total > MAX_EXPANDED_BYTES {
                    return Err(too_large());
                }
                (a.value.clone(), 0, a.size, a.height)
            }
            Event::SequenceStart(aid, _) | Event::MappingStart(aid, _) => {
                if stack.len() >= MAX_DEPTH {
                    return Err(ImportError::TooDeep);
                }
                total = total.saturating_add(NODE_COST);
                if total > MAX_EXPANDED_BYTES {
                    return Err(too_large());
                }
                let frame = if matches!(ev, Event::SequenceStart(..)) {
                    Frame::Seq(Vec::new())
                } else {
                    Frame::Map {
                        map: Map::new(),
                        key: None,
                    }
                };
                stack.push(Open {
                    frame,
                    anchor: aid,
                    size: NODE_COST,
                    height: 1,
                });
                continue;
            }
            Event::SequenceEnd | Event::MappingEnd => {
                let open = stack
                    .pop()
                    .ok_or_else(|| ImportError::Parse("unbalanced YAML".into()))?;
                let value = match open.frame {
                    Frame::Seq(items) => Value::Array(items),
                    Frame::Map { map, .. } => Value::Object(map),
                };
                (value, open.anchor, open.size, open.height)
            }
            _ => continue,
        };
        let (value, anchor, size, height) = done;
        if anchor != 0 {
            // The stored copy is memory too: charge it.
            total = total.saturating_add(size);
            if total > MAX_EXPANDED_BYTES {
                return Err(too_large());
            }
            anchors.insert(
                anchor,
                Anchored {
                    value: value.clone(),
                    size,
                    height,
                },
            );
        }
        match stack.last_mut() {
            None => root = Some(value),
            Some(top) => {
                top.size = top.size.saturating_add(size);
                top.height = top.height.max(height + 1);
                match &mut top.frame {
                    Frame::Seq(items) => items.push(value),
                    Frame::Map { map, key } => match key.take() {
                        None => *key = Some(key_string(&value)),
                        Some(k) => {
                            map.insert(k, value);
                        }
                    },
                }
            }
        }
    }
    root.ok_or_else(|| ImportError::Parse("empty YAML document".into()))
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
                let r = parse(s.as_bytes());
                assert!(
                    matches!(r, Err(ImportError::TooDeep)),
                    "{r:?} for {}",
                    &s[..20]
                );
            }
        });
    }

    #[test]
    fn stray_brackets_do_not_bypass_depth() {
        let deep = "- ".repeat(100_000) + "1";
        let cases = [
            format!("x: a [\ny:\n{deep}"),
            format!("description: Ranges like [0, 10)\ny:\n{deep}"),
            format!("x: |\n  [\ny:\n{deep}"),
            format!("x: \"a\n  [ b\"\ny:\n{deep}"),
        ];
        on_small_stack(move || {
            for s in cases {
                assert!(matches!(parse(s.as_bytes()), Err(ImportError::TooDeep)));
            }
        });
    }

    #[test]
    fn aliases_hidden_in_flow_are_too_large() {
        let big = "a".repeat(1024 * 1024);
        let one_line = format!("x: [&b \"{big}\"{}]\n", ", *b".repeat(700));
        let multi = format!(
            "b: &b \"{big}\"\nlist: [\"s\n  \"{}, \"s\n  \"]\n",
            ", *b".repeat(100)
        );
        on_small_stack(move || {
            for s in [one_line, multi] {
                let start = std::time::Instant::now();
                let r = parse(s.as_bytes());
                assert!(matches!(r, Err(ImportError::TooLarge(_))), "{r:?}");
                assert!(start.elapsed().as_secs() < 5);
            }
        });
    }

    #[test]
    fn nested_alias_depth_is_too_deep() {
        let mut s = String::from("a0: &a0 [x]\n");
        for i in 1..70 {
            s.push_str(&format!("a{i}: &a{i} [*a{}]\n", i - 1));
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
    fn nested_anchors_are_charged() {
        let big = "a".repeat(100 * 1024);
        let mut inner = "*b".to_string();
        inner = vec![inner; 60].join(", ");
        let mut x = format!("[{inner}]");
        for i in (0..60).rev() {
            x = format!("[&n{i} {x}]");
        }
        let nested = format!("b: &b \"{big}\"\nx: {x}\n");
        let mut anchors = String::from("b: &b \"");
        anchors.push_str(&"a".repeat(1024 * 1024));
        anchors.push_str("\"\nx: ");
        let mut y = vec!["*b"; 63].join(", ");
        y = format!("[{y}]");
        for i in (0..62).rev() {
            y = format!("[&m{i} {y}]");
        }
        anchors.push_str(&y);
        anchors.push('\n');
        on_small_stack(move || {
            for s in [nested, anchors] {
                let start = std::time::Instant::now();
                let r = parse(s.as_bytes());
                assert!(matches!(r, Err(ImportError::TooLarge(_))), "{r:?}");
                assert!(start.elapsed().as_secs() < 5);
            }
        });
    }

    #[test]
    fn yaml_over_2_mib_is_too_large() {
        let s = format!("a: 1\n#{}\n", "x".repeat(2 * 1024 * 1024));
        assert!(matches!(parse(s.as_bytes()), Err(ImportError::TooLarge(_))));
        let ok = format!("a: 1\n#{}\n", "x".repeat(1024 * 1024));
        assert!(parse(ok.as_bytes()).is_ok());
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
