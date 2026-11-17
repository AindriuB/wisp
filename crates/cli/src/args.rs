//! Hand-rolled argument parser.

use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Help,
    Version,
    Validate {
        collection: PathBuf,
    },
    Inspect {
        collection: PathBuf,
        env: Option<String>,
    },
    Explain {
        collection: PathBuf,
        request: String,
        env: Option<String>,
    },
    Run {
        collection: PathBuf,
        request: String,
        env: Option<String>,
        keychain: bool,
        history: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub command: Action,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UsageError {
    NoCommand,
    UnknownCommand(String),
    UnknownFlag(String),
    MissingValue(String),
    MissingArgument(&'static str),
    ExtraArgument(String),
    NotUtf8,
}

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCommand => f.write_str("no command given"),
            Self::UnknownCommand(c) => write!(f, "unknown command `{c}`"),
            Self::UnknownFlag(c) => write!(f, "unknown option `{c}`"),
            Self::MissingValue(c) => write!(f, "option `{c}` needs a value"),
            Self::MissingArgument(a) => write!(f, "missing argument <{a}>"),
            Self::ExtraArgument(a) => write!(f, "unexpected argument `{a}`"),
            Self::NotUtf8 => f.write_str("environment name and request path must be UTF-8"),
        }
    }
}

pub const USAGE: &str = "\
usage: wisp-cli <command> [options]

commands:
  validate <collection>
  inspect <collection> [--env <name>]
  explain <collection> <request-path> [--env <name>]
  run <collection> <request-path> [--env <name>] [--keychain] [--history]

options:
  --json       print one JSON document
  --env <name> select an environment
  --keychain   run: load keychain-backed secrets
  --history    run: append a history entry
  --version    print the version
  --help       print this help
  --           treat the rest as positional arguments
";

/// Parse the arguments after the program name.
pub fn parse<I: IntoIterator<Item = OsString>>(args: I) -> Result<Parsed, UsageError> {
    let mut json = false;
    let mut env: Option<String> = None;
    let mut keychain = false;
    let mut history = false;
    let mut help = false;
    let mut version = false;
    let mut pos: Vec<OsString> = Vec::new();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        let Some(s) = a.to_str() else {
            pos.push(a);
            continue;
        };
        if s == "--" {
            pos.extend(it.by_ref());
            break;
        }
        match s {
            "--json" => json = true,
            "--keychain" => keychain = true,
            "--history" => history = true,
            "--help" | "-h" => help = true,
            "--version" => version = true,
            "--env" => {
                let v = it
                    .next()
                    .ok_or_else(|| UsageError::MissingValue("--env".into()))?;
                env = Some(v.into_string().map_err(|_| UsageError::NotUtf8)?);
            }
            _ if s.starts_with("--env=") => env = Some(s["--env=".len()..].to_owned()),
            _ if s.starts_with("--") && s.len() > 2 => {
                let name = s.split('=').next().unwrap_or(s);
                return Err(UsageError::UnknownFlag(name.to_owned()));
            }
            _ => pos.push(a),
        }
    }
    if help {
        return Ok(Parsed {
            command: Action::Help,
            json,
        });
    }
    if version {
        return Ok(Parsed {
            command: Action::Version,
            json,
        });
    }
    let mut pos = pos.into_iter();
    let name = pos.next().ok_or(UsageError::NoCommand)?;
    let name = name.into_string().map_err(|_| UsageError::NotUtf8)?;
    let mut collection = || {
        pos.next()
            .map(PathBuf::from)
            .ok_or(UsageError::MissingArgument("collection"))
    };
    let command = match name.as_str() {
        "validate" => Action::Validate {
            collection: collection()?,
        },
        "inspect" => Action::Inspect {
            collection: collection()?,
            env: env.take(),
        },
        "explain" => {
            let collection = collection()?;
            let request = pos
                .next()
                .ok_or(UsageError::MissingArgument("request-path"))?
                .into_string()
                .map_err(|_| UsageError::NotUtf8)?;
            Action::Explain {
                collection,
                request,
                env: env.take(),
            }
        }
        "run" => {
            let collection = collection()?;
            let request = pos
                .next()
                .ok_or(UsageError::MissingArgument("request-path"))?
                .into_string()
                .map_err(|_| UsageError::NotUtf8)?;
            Action::Run {
                collection,
                request,
                env: env.take(),
                keychain: std::mem::take(&mut keychain),
                history: std::mem::take(&mut history),
            }
        }
        _ => return Err(UsageError::UnknownCommand(name)),
    };
    if let Some(extra) = pos.next() {
        return Err(UsageError::ExtraArgument(
            extra.to_string_lossy().into_owned(),
        ));
    }
    if let Some(_e) = env {
        return Err(UsageError::UnknownFlag("--env".into()));
    }
    if keychain {
        return Err(UsageError::UnknownFlag("--keychain".into()));
    }
    if history {
        return Err(UsageError::UnknownFlag("--history".into()));
    }
    Ok(Parsed { command, json })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: &[&str]) -> Result<Parsed, UsageError> {
        parse(a.iter().map(OsString::from))
    }
    fn ok(command: Action, json: bool) -> Result<Parsed, UsageError> {
        Ok(Parsed { command, json })
    }
    fn inspect(c: &str, env: Option<&str>) -> Action {
        Action::Inspect {
            collection: c.into(),
            env: env.map(str::to_owned),
        }
    }

    fn run(c: &str, r: &str, env: Option<&str>, keychain: bool, history: bool) -> Action {
        Action::Run {
            collection: c.into(),
            request: r.into(),
            env: env.map(str::to_owned),
            keychain,
            history,
        }
    }

    #[test]
    fn table() {
        let cases: Vec<(Vec<&str>, Result<Parsed, UsageError>)> = vec![
            (vec!["--help"], ok(Action::Help, false)),
            (vec!["inspect", "--help"], ok(Action::Help, false)),
            (vec!["--version"], ok(Action::Version, false)),
            (
                vec!["validate", "c"],
                ok(
                    Action::Validate {
                        collection: "c".into(),
                    },
                    false,
                ),
            ),
            (vec!["inspect", "c"], ok(inspect("c", None), false)),
            (vec!["inspect", "c", "--json"], ok(inspect("c", None), true)),
            (vec!["--json", "inspect", "c"], ok(inspect("c", None), true)),
            (
                vec!["inspect", "c", "--env", "dev"],
                ok(inspect("c", Some("dev")), false),
            ),
            (
                vec!["inspect", "c", "--env=dev"],
                ok(inspect("c", Some("dev")), false),
            ),
            (
                vec!["inspect", "--", "--json"],
                ok(inspect("--json", None), false),
            ),
            (
                vec!["explain", "c", "a/b.toml", "--env", "x"],
                ok(
                    Action::Explain {
                        collection: "c".into(),
                        request: "a/b.toml".into(),
                        env: Some("x".into()),
                    },
                    false,
                ),
            ),
            (vec![], Err(UsageError::NoCommand)),
            (vec!["--json"], Err(UsageError::NoCommand)),
            (vec!["frob"], Err(UsageError::UnknownCommand("frob".into()))),
            (
                vec!["inspect"],
                Err(UsageError::MissingArgument("collection")),
            ),
            (
                vec!["explain", "c"],
                Err(UsageError::MissingArgument("request-path")),
            ),
            (
                vec!["inspect", "c", "--env"],
                Err(UsageError::MissingValue("--env".into())),
            ),
            (
                vec!["inspect", "c", "--bogus"],
                Err(UsageError::UnknownFlag("--bogus".into())),
            ),
            (
                vec!["inspect", "c", "--bogus=1"],
                Err(UsageError::UnknownFlag("--bogus".into())),
            ),
            (
                vec!["inspect", "c", "d"],
                Err(UsageError::ExtraArgument("d".into())),
            ),
            (
                vec!["run", "c", "a.toml"],
                ok(run("c", "a.toml", None, false, false), false),
            ),
            (
                vec!["run", "c", "a.toml", "--env", "x", "--keychain", "--history", "--json"],
                ok(run("c", "a.toml", Some("x"), true, true), true),
            ),
            (
                vec!["run", "c", "a.toml", "--keychain"],
                ok(run("c", "a.toml", None, true, false), false),
            ),
            (
                vec!["run", "c", "a.toml", "--history"],
                ok(run("c", "a.toml", None, false, true), false),
            ),
            (
                vec!["run", "c"],
                Err(UsageError::MissingArgument("request-path")),
            ),
            (
                vec!["run", "c", "a.toml", "--bogus"],
                Err(UsageError::UnknownFlag("--bogus".into())),
            ),
            (
                vec!["explain", "c", "a.toml", "--keychain"],
                Err(UsageError::UnknownFlag("--keychain".into())),
            ),
            (
                vec!["validate", "c", "--history"],
                Err(UsageError::UnknownFlag("--history".into())),
            ),
            (
                vec!["validate", "c", "--env", "x"],
                Err(UsageError::UnknownFlag("--env".into())),
            ),
        ];
        for (args, want) in cases {
            assert_eq!(p(&args), want, "{args:?}");
        }
    }
}
