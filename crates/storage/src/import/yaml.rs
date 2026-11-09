//! YAML input for the OpenAPI importer, read into `serde_json::Value`.
//!
//! The parser expands aliases itself with a budget of 100,000 nodes; this
//! module adds the depth check the parser lacks.

use saphyr::{LoadableYamlNode, Scalar, Yaml};
use serde_json::{Map, Number, Value};

use super::{ImportError, MAX_DEPTH, check_size};

/// Parse one YAML document into a JSON value.
pub fn parse(bytes: &[u8]) -> Result<Value, ImportError> {
    check_size(bytes.len())?;
    let text = std::str::from_utf8(bytes)
        .map_err(|_| ImportError::Parse("input is not valid UTF-8".into()))?;
    check_depth(text)?;
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

/// Reject text whose block indentation or flow brackets nest too deeply.
/// Quotes are skipped per line, so brackets in short strings do not count.
fn check_depth(text: &str) -> Result<(), ImportError> {
    let mut indents: Vec<usize> = Vec::new();
    let mut flow = 0usize;
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
            // `- - - x` nests without indentation.
            let dashes = trimmed
                .split_inclusive(' ')
                .take_while(|t| t.trim_end() == "-")
                .count();
            if indents.len() + dashes > MAX_DEPTH + 1 {
                return Err(ImportError::TooDeep);
            }
        }
        let mut quote: Option<char> = None;
        for c in trimmed.chars() {
            match (quote, c) {
                (Some(q), c) if c == q => quote = None,
                (Some(_), _) => {}
                (None, '"' | '\'') => quote = Some(c),
                (None, '#') => break,
                (None, '[' | '{') => {
                    flow += 1;
                    if flow + indents.len() > MAX_DEPTH + 1 {
                        return Err(ImportError::TooDeep);
                    }
                }
                (None, ']' | '}') => flow = flow.saturating_sub(1),
                _ => {}
            }
        }
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

    #[test]
    fn malformed_is_error() {
        assert!(parse(b"a: [1, 2\n").is_err());
        assert!(parse(b"\xff\xfe").is_err());
        assert!(parse(b"").is_err());
    }
}
