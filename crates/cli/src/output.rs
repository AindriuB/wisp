//! Writing documents. JSON goes out only through [`json`], which takes a [`Dto`].

use std::io::{self, Write};

use crate::dto::{Dto, InspectDoc};

pub fn json<T: Dto>(w: &mut dyn Write, doc: &T) -> io::Result<()> {
    let text = serde_json::to_string_pretty(doc).map_err(io::Error::other)?;
    writeln!(w, "{text}")
}

/// Escapes control characters, C1 controls and Unicode bidi controls so an
/// untrusted string cannot drive the terminal or forge lines.
pub fn display(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let bidi = matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
        if c.is_control() || bidi {
            out.push_str(&format!("\\u{{{:04x}}}", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}

pub fn inspect_text(w: &mut dyn Write, d: &InspectDoc) -> io::Result<()> {
    let c = &d.collection;
    writeln!(
        w,
        "collection: {}",
        display(c.id.as_deref().unwrap_or("(no id)"))
    )?;
    writeln!(w, "format: {}", c.format)?;
    let invalid = c
        .invalid
        .map_or_else(|| "unknown (truncated)".to_owned(), |n| n.to_string());
    writeln!(
        w,
        "folders: {}  requests: {}  invalid: {invalid}",
        c.folders, c.requests
    )?;
    if let Some(e) = &d.environment {
        writeln!(w, "selected environment: {}", display(e))?;
    }
    writeln!(
        w,
        "environments: {}",
        d.environments
            .iter()
            .map(|e| display(e))
            .collect::<Vec<_>>()
            .join(", ")
    )?;
    for n in &d.nodes {
        write!(
            w,
            "{} {} \"{}\"",
            n.kind,
            display(&n.path),
            display(&n.name)
        )?;
        if let Some(m) = &n.method {
            write!(w, " {}", display(m))?;
        }
        if let Some(u) = &n.url {
            write!(w, " {}", display(u))?;
        }
        if let Some(a) = n.auth {
            write!(w, " auth={a}")?;
        }
        if !n.headers.is_empty() {
            write!(
                w,
                " headers={}",
                n.headers
                    .iter()
                    .map(|h| display(h))
                    .collect::<Vec<_>>()
                    .join(",")
            )?;
        }
        if !n.valid {
            write!(w, " INVALID")?;
        }
        writeln!(w)?;
    }
    if d.truncated {
        writeln!(
            w,
            "(truncated: more nodes than the {} shown)",
            d.nodes.len()
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::display;

    #[test]
    fn controls_are_escaped() {
        let s = display("a\nb\r\u{1b}]0;x\u{7}\u{85}\u{202e}c");
        assert_eq!(
            s,
            "a\\u{000a}b\\u{000d}\\u{001b}]0;x\\u{0007}\\u{0085}\\u{202e}c"
        );
        assert_eq!(display("plain é/x"), "plain é/x");
    }
}
