//! Writing documents. JSON goes out only through [`json`], which takes a [`Dto`].

use std::io::{self, Write};

use crate::dto::{Dto, ExplainDoc, InspectDoc, PartDoc, ValidateDoc};

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

pub fn validate_text(w: &mut dyn Write, d: &ValidateDoc) -> io::Result<()> {
    for f in &d.findings {
        let key = f
            .key
            .as_deref()
            .map_or_else(String::new, |k| format!(" [{}]", display(k)));
        writeln!(
            w,
            "{}: {}{key}: {}",
            f.severity,
            display(&f.path),
            display(&f.message)
        )?;
    }
    if d.truncated {
        writeln!(w, "(truncated: the folder limit stopped the walk early)")?;
    }
    writeln!(w, "{} error(s), {} warning(s)", d.errors, d.warnings)
}

/// Markers are bracketed when they sit among other parts, so they read as markers.
fn parts(p: &[PartDoc]) -> String {
    let mixed = p.len() > 1;
    p.iter()
        .map(|p| {
            let marker = match p {
                PartDoc::Text { text } => return display(text),
                PartDoc::Secret { name, resolved } => format!(
                    "secret {} ({})",
                    display(name),
                    if *resolved { "resolved" } else { "not set" }
                ),
                PartDoc::Dynamic { name } => format!("{}, generated at send", display(name)),
                PartDoc::Hidden { reason } => match *reason {
                    "credential" => "hidden: credential name".to_owned(),
                    "userinfo" => "hidden: userinfo".to_owned(),
                    _ => "hidden: too long".to_owned(),
                },
            };
            if mixed { format!("[{marker}]") } else { marker }
        })
        .collect()
}

pub fn explain_text(w: &mut dyn Write, d: &ExplainDoc) -> io::Result<()> {
    writeln!(w, "request: {}", display(&d.request))?;
    match &d.environment {
        None => writeln!(w, "environment: none")?,
        Some(e) => {
            writeln!(w, "environment: {}", display(&e.name))?;
            if let Some(k) = e.keychain {
                writeln!(w, "  {k}")?;
            }
            for s in &e.secrets {
                writeln!(
                    w,
                    "  secret {} ({})",
                    display(&s.name),
                    if s.present { "resolved" } else { "not set" }
                )?;
            }
        }
    }
    writeln!(w, "{} {}", d.method, parts(&d.url))?;
    if !d.params.is_empty() {
        writeln!(w, "params:")?;
        for p in &d.params {
            writeln!(
                w,
                "  {} {} = {}  ({})",
                p.kind,
                parts(&p.name),
                parts(&p.value),
                display(&p.origin)
            )?;
        }
    }
    if !d.headers.is_empty() {
        writeln!(w, "headers:")?;
        for h in &d.headers {
            write!(
                w,
                "  {}: {}  ({}",
                parts(&h.name),
                parts(&h.value),
                display(&h.origin)
            )?;
            if let Some(s) = &h.synthetic {
                write!(w, ", {}", display(s))?;
            }
            if !h.overridden.is_empty() {
                write!(
                    w,
                    ", overrides {}",
                    h.overridden
                        .iter()
                        .map(|o| display(o))
                        .collect::<Vec<_>>()
                        .join(", ")
                )?;
            }
            writeln!(w, ")")?;
        }
    }
    write!(w, "auth: {}", d.auth.kind)?;
    if let Some(o) = &d.auth.origin {
        write!(w, "  ({})", display(o))?;
    }
    match (d.auth.application, &d.auth.target) {
        ("header", Some(t)) => write!(w, ", as header {}", parts(t))?,
        ("query", Some(t)) => write!(w, ", as query {}", parts(t))?,
        ("at-send", _) => write!(w, ", applied at send")?,
        _ => {}
    }
    writeln!(w)?;
    for f in &d.auth.fields {
        writeln!(w, "  {}: {}", f.name, parts(&f.value))?;
    }
    writeln!(w, "settings:")?;
    for s in &d.settings {
        writeln!(w, "  {}: {}  ({})", s.name, display(&s.value), s.source)?;
    }
    write!(w, "body: {}", d.body.kind)?;
    if let Some(b) = d.body.bytes {
        write!(w, ", {b} bytes")?;
    }
    if !d.body.vars.is_empty() {
        write!(
            w,
            ", vars: {}",
            d.body
                .vars
                .iter()
                .map(|v| display(v))
                .collect::<Vec<_>>()
                .join(", ")
        )?;
    }
    writeln!(w)?;
    if !d.variables.is_empty() {
        writeln!(w, "variables:")?;
        for v in &d.variables {
            write!(
                w,
                "  {} = {}  ({}",
                display(&v.name),
                parts(&v.value),
                display(&v.origin)
            )?;
            if !v.shadowed.is_empty() {
                write!(
                    w,
                    ", shadows {}",
                    v.shadowed
                        .iter()
                        .map(|o| display(o))
                        .collect::<Vec<_>>()
                        .join(", ")
                )?;
            }
            if !v.locations.is_empty() {
                write!(
                    w,
                    ", used in {}",
                    v.locations
                        .iter()
                        .map(|o| display(o))
                        .collect::<Vec<_>>()
                        .join(", ")
                )?;
            }
            writeln!(w, ")")?;
        }
    }
    if !d.problems.is_empty() {
        writeln!(w, "problems:")?;
        for p in &d.problems {
            write!(w, "  {}", p.kind)?;
            if let Some(n) = &p.name {
                write!(w, " {}", display(n))?;
            }
            if let Some(l) = &p.location {
                write!(w, " in {}", display(l))?;
            }
            if let Some(m) = &p.message {
                write!(w, ": {}", display(m))?;
            }
            writeln!(w)?;
        }
    }
    for x in &d.warnings {
        writeln!(w, "warning: {}", display(x))?;
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
