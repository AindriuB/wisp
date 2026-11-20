//! `wisp-cli`: read-only console tool for Wisp collections.
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod args;
mod cmd;
mod dto;
mod output;

use std::io::{self, Write};
use std::process::ExitCode;

use args::{Action, USAGE};
use cmd::inspect::Failure;

const EXIT_USAGE: u8 = 2;
const EXIT_UNREADABLE: u8 = 3;
const EXIT_IO: u8 = 1;
/// `run` only: writing the result failed. 1 stays "an assertion failed".
const EXIT_OUTPUT: u8 = 8;

fn main() -> ExitCode {
    let stdout = io::stdout();
    let stderr = io::stderr();
    ExitCode::from(run(
        std::env::args_os().skip(1),
        &mut stdout.lock(),
        &mut stderr.lock(),
    ))
}

fn finish(r: io::Result<()>) -> u8 {
    match r {
        Ok(()) => 0,
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(_) => EXIT_IO,
    }
}

fn failure(err: &mut dyn Write, f: Failure) -> u8 {
    match f {
        Failure::Usage(m) => {
            finish(writeln!(err, "wisp-cli: {}", output::display(&m))).max(EXIT_USAGE)
        }
        Failure::Unreadable(m) => finish(writeln!(
            err,
            "wisp-cli: cannot read collection: {}",
            output::display(&m)
        ))
        .max(EXIT_UNREADABLE),
    }
}

fn run<I: IntoIterator<Item = std::ffi::OsString>>(
    argv: I,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let parsed = match args::parse(argv) {
        Ok(p) => p,
        Err(e) => {
            return finish(writeln!(err, "wisp-cli: {e}\n\n{USAGE}")).max(EXIT_USAGE);
        }
    };
    match parsed.command {
        Action::Help => finish(write!(out, "{USAGE}")),
        Action::Version => finish(writeln!(out, "wisp-cli {}", env!("CARGO_PKG_VERSION"))),
        Action::Validate { collection } => match cmd::validate::run(&collection) {
            Ok(doc) => {
                let code = u8::from(doc.errors > 0);
                finish(if parsed.json {
                    output::json(out, &doc)
                } else {
                    output::validate_text(out, &doc)
                })
                .max(code)
            }
            Err(f) => failure(err, f),
        },
        Action::Explain {
            collection,
            request,
            env,
        } => match cmd::explain::run(&collection, &request, env.as_deref()) {
            Ok(doc) => finish(if parsed.json {
                output::json(out, &doc)
            } else {
                output::explain_text(out, &doc)
            }),
            Err(f) => failure(err, f),
        },
        Action::Run {
            collection,
            request,
            env,
            keychain,
            history,
            ca_bundle,
            no_system_trust,
        } => match cmd::run::run(cmd::run::Args {
            collection,
            request,
            env,
            keychain,
            history,
            ca_bundle,
            no_system_trust,
        }) {
            Ok(doc) => match if parsed.json {
                output::json(out, &doc)
            } else {
                output::run_text(out, &doc)
            } {
                Ok(()) => doc.exit,
                Err(e) if e.kind() == io::ErrorKind::BrokenPipe => doc.exit,
                Err(_) => EXIT_OUTPUT,
            },
            Err(f) => failure(err, f),
        },
        Action::Snapshot {
            collection,
            env,
            request,
            recent,
        } => match cmd::snapshot::run(&collection, env.as_deref(), request.as_deref(), recent) {
            Ok(s) => {
                for n in &s.notes {
                    let _ = writeln!(err, "wisp-cli: {}", output::display(n));
                }
                finish(match (&s.doc, parsed.json) {
                    (cmd::snapshot::Out::Full(d), true) => output::json(out, d),
                    (cmd::snapshot::Out::Full(d), false) => output::snapshot_text(out, d),
                    (cmd::snapshot::Out::Minimal(d), true) => output::json(out, d),
                    (cmd::snapshot::Out::Minimal(d), false) => {
                        output::snapshot_minimal_text(out, d)
                    }
                })
            }
            Err(f) => failure(err, f),
        },
        Action::Inspect { collection, env } => {
            match cmd::inspect::run(&collection, env.as_deref()) {
                Ok(doc) => finish(if parsed.json {
                    output::json(out, &doc)
                } else {
                    output::inspect_text(out, &doc)
                }),
                Err(f) => failure(err, f),
            }
        }
    }
}
