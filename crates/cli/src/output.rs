//! Writing documents. JSON goes out only through [`json`], which takes a [`Dto`].

use std::io::{self, Write};

use crate::dto::{Dto, InspectDoc};

pub fn json<T: Dto>(w: &mut dyn Write, doc: &T) -> io::Result<()> {
    let text = serde_json::to_string_pretty(doc).map_err(io::Error::other)?;
    writeln!(w, "{text}")
}

pub fn inspect_text(w: &mut dyn Write, d: &InspectDoc) -> io::Result<()> {
    let c = &d.collection;
    writeln!(w, "collection: {}", c.id.as_deref().unwrap_or("(no id)"))?;
    writeln!(w, "format: {}", c.format)?;
    writeln!(
        w,
        "folders: {}  requests: {}  invalid: {}",
        c.folders, c.requests, c.invalid
    )?;
    if let Some(e) = &d.environment {
        writeln!(w, "selected environment: {e}")?;
    }
    writeln!(w, "environments: {}", d.environments.join(", "))?;
    for n in &d.nodes {
        write!(w, "{} {} \"{}\"", n.kind, n.path, n.name)?;
        if let Some(m) = &n.method {
            write!(w, " {m}")?;
        }
        if let Some(u) = &n.url {
            write!(w, " {u}")?;
        }
        if let Some(a) = n.auth {
            write!(w, " auth={a}")?;
        }
        if !n.headers.is_empty() {
            write!(w, " headers={}", n.headers.join(","))?;
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
