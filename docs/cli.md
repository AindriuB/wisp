# wisp-cli

`wisp-cli` is a read-only console tool for Wisp collections, shipped from 0.3.0
as a separate binary beside the GUI. It is meant for a terminal, a script or an
agent that needs to check a collection or see how a request would be built. It
links `wisp-core` and `wisp-storage` and no GPUI (see
[`architecture.md`](architecture.md#boundaries-that-must-not-be-crossed)). The
source is `crates/cli/src`.

## Contract

- **Read-only.** It never writes a file, never sends a request, opens no
  network connection and never writes a collection id. `scripts/check-boundaries.sh`
  fails the build if `crates/cli/src` mentions the engine, `reqwest`, `tokio`, a
  secret store or a write call.
- **No secret value is printed.** File-mode secret values are replaced with a
  placeholder before the variable scope is built. An environment whose secrets
  live in the OS keychain is not opened at all; `explain` says
  `keychain: not loaded by the CLI`. A value a header, auth field or URL would
  carry as a credential is shown as hidden, never as text. Secrets appear only
  as a name and a `present` or `resolved` flag.
- **Untrusted text is escaped.** Control characters, C1 controls and Unicode
  bidirectional controls in names, paths and messages are printed as
  `\u{XXXX}` in text output, so a collection cannot drive the terminal.
- **Bounded.** Names, paths and messages are cut at 200 characters, `inspect`
  lists at most 2,000 nodes and 100 header names per request, and `validate`
  reports `truncated` when its own finding limit is reached.

## Usage

```
wisp-cli <command> [options]

commands:
  validate <collection>
  inspect <collection> [--env <name>]
  explain <collection> <request-path> [--env <name>]

options:
  --json       print one JSON document
  --env <name> select an environment (also --env=<name>)
  --version    print "wisp-cli <version>"
  --help, -h   print the usage text
  --           treat the rest as positional arguments
```

`--json` may appear anywhere. `--env` is accepted only by `inspect` and
`explain`; on `validate` it is an unknown option. `--help` and `--version` win
over a command and exit 0.

## Commands

### validate

`wisp-cli validate <collection>` walks the whole collection with the tolerant
storage reader and reports every finding: broken folder and request files,
malformed or unrecognised assertions, and so on. It exits 1 when there is at
least one error. Warnings alone exit 0.

JSON fields: `schema`, `command`, `errors`, `warnings`, `truncated` and
`findings`, each finding `{ path, key, message, severity }` with `severity` of
`error` or `warning` and `key` possibly null. Paths are relative to the
collection.

### inspect

`wisp-cli inspect <collection> [--env <name>]` prints a summary: the collection
id (null when `collection.toml` has none), format, counts of folders, requests
and invalid files, the environment names, and one line per folder and request.
It never reads a value into the output.

JSON fields: `schema`, `command`, `collection` (`id`, `format`, `folders`,
`requests`, `invalid`), `environments`, `environment` (the selected name or
null), `truncated` and `nodes`, each node `{ path, kind, name, method, url,
auth, headers, valid }`. `url` is origin and path only, with no userinfo, query
or fragment. `headers` are names only. `auth` is the kind (`none`, `inherit`,
`basic`, `bearer`, `apikey`, `digest`, `awssigv4`, `oauth2-client-credentials`
or `oauth2-auth-code`). When the 2,000-node cap is hit, `truncated` is true and
`collection.invalid` is null (unknown). `--env` only checks that the name
exists and echoes it.

### explain

`wisp-cli explain <collection> <request-path> [--env <name>]` prints the
resolution report of one request: where every variable, header, auth setting
and request setting comes from, without sending anything. `<request-path>` is
relative to the collection, for example `users/get-user.toml`. An absolute path,
a path with `..`, and a path through a symlink are refused. Without `--env` no
environment layer is applied.

JSON fields: `schema`, `command`, `request`, `environment` (`name`, `keychain`,
`secrets` as `{ name, present }`; null without `--env`), `method`, `url`,
`params`, `headers`, `auth`, `settings`, `body`, `variables`, `problems` and
`warnings`. Display text is a list of parts. Each part is an object tagged by a
`kind` key, one of `text` (`text`), `secret` (`name`, `resolved`), `dynamic`
(`name`) or `hidden` (`reason` of `credential`, `userinfo` or `too-long`), for
example `{"kind":"text","text":"..."}` and
`{"kind":"secret","name":"token","resolved":true}`. `params`, `headers` and `variables` carry an
`origin` (`request`, `folder <path>`, `collection`, `environment`, `secret`,
`dynamic`, `body type`, `auth`); a header also lists the origins it
`overridden` and any `synthetic` reason. `auth` has `kind`, `origin`,
`application` (`none`, `header`, `query` or `at-send`), `target` and `fields`.
`problems` carry unresolved variables, cycles and similar, as `{ kind, name,
location, message }`. A dynamic variable such as `{{$uuid}}` is marked, never
generated.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success. Also a closed output pipe, `--help` and `--version`, and `explain` of a request that exists even when it has unresolved variables (they are the report). |
| 1 | `validate` found at least one error, or writing the output failed. |
| 2 | Usage error: no command, unknown command or option, missing or extra argument, an unknown environment name. Usage text goes to stderr. |
| 3 | The collection or request could not be read: not a collection, request not found, a refused path, an unreadable file. |

Errors go to stderr as `wisp-cli: <message>`; results go to stdout.

## JSON schema version

Every JSON document carries `"schema": "wisp-cli/1"` (`crates/cli/src/dto.rs:19`).
Additive fields do not change the number; a removed or retyped field does
(spec `plan/specs/2026-11-14-wisp-0-3-0.md`, Feature 4). Output is pretty-printed, one document per run.
