//! Strips comments and blank lines from the bundled public suffix list, so the binary
//! embeds only the rules. Section markers and the VERSION/COMMIT header are kept.

use std::{env, fs, path::Path};

fn main() {
    let src = "data/public_suffix_list.dat";
    println!("cargo:rerun-if-changed={src}");
    println!("cargo:rerun-if-changed=build.rs");
    let text = fs::read_to_string(src).expect("read public suffix list");
    let mut out = String::with_capacity(text.len() / 2);
    for line in text.lines() {
        let t = line.trim();
        let keep = match t.strip_prefix("//") {
            None => !t.is_empty(),
            Some(rest) => {
                let rest = rest.trim_start();
                rest.starts_with("===BEGIN ")
                    || rest.starts_with("===END ")
                    || rest.starts_with("VERSION:")
                    || rest.starts_with("COMMIT:")
            }
        };
        if keep {
            out.push_str(t);
            out.push('\n');
        }
    }
    let dest = Path::new(&env::var("OUT_DIR").expect("OUT_DIR")).join("psl.dat");
    fs::write(dest, out).expect("write psl.dat");
}
