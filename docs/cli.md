# wisp-cli

`wisp-cli` is a console tool for Wisp collections, shipped from 0.3.0 as a
separate binary beside the GUI. It is meant for a terminal, a script or an
agent that needs to check a collection, see how a request would be built, send
one request in CI, or take a picture of a workspace. `validate`, `inspect`,
`explain` and `snapshot` are read-only; `run` sends one request. **`--json` is
the agent interface:** there is no MCP server, no change set and no approval
step, and every command prints one versioned JSON document with `--json`. It
links `wisp-core`, `wisp-storage` and `wisp-service` and no GPUI (see
[`architecture.md`](architecture.md#boundaries-that-must-not-be-crossed)). The
source is `crates/cli/src`.

## Contract

- **Read-only, except `run`.** `validate`, `inspect`, `explain` and `snapshot`
  never write a file, never send a request, open no network connection and
  never write a collection id. `run` sends one request, and with `--history`
  appends one history entry; it writes nothing else and never writes a
  collection id. `scripts/check-boundaries.sh` fails the build if
  `crates/cli/src` mentions the engine, `reqwest`, `tokio`, a secret store or a
  write call: `run` reaches the network only through `wisp-service`.
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
  run <collection> <request-path> [--env <name>] [--keychain] [--history]
      [--ca-bundle <file>] [--no-system-trust]
  snapshot <collection> [--env <name>] [--request <request-path>] [--recent <n>]

options:
  --json       print one JSON document
  --env <name> select an environment (also --env=<name>)
  --keychain   (run) load keychain-backed secrets
  --history    (run) append a history entry
  --ca-bundle <file>  (run) also trust the CA certificates in a PEM file
  --no-system-trust   (run) do not trust the operating system's store
  --request <request-path>  (snapshot) include that request's explain document
  --recent <n> (snapshot) include the last n history entries, 1 to 50
  --version    print "wisp-cli <version>"
  --help, -h   print the usage text
  --           treat the rest as positional arguments
```

`--json` may appear anywhere. `--env` is accepted only by `inspect`,
`explain`, `run` and `snapshot`; on `validate` it is an unknown option. `--help` and `--version` win
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
`warnings`. `body.kind` is one of `none`, `JSON`, `text`, `XML`, `graphql`,
`form`, `multipart` or `file`; `graphql` is a request with `[body] type =
"graphql"` (a query and a variables string). `inspect` does not list body
kinds: it counts such a request as a valid request, where Wisp 0.4.0 would
count it as invalid. Display text is a list of parts. Each part is an object tagged by a
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

### run

`wisp-cli run <collection> <request-path> [--env <name>] [--keychain] [--history] [--json]`
sends one saved request through the same service code as the GUI and reports
what happened. Arguments are as for `explain`. One request per call: there are
no folder runs, so loop in the shell. Example:

```sh
wisp-cli run ./shop orders/create.toml --env staging --json
```

- **Never retries.** A failed send is reported, never repeated. `retry_safe`
  tells the caller whether repeating it is safe.
- **Ignores the GUI send guard.** The per-user "confirm before send" setting in
  `settings.toml` is a GUI accident guard; `run` does not read it.
- **Cookies.** `run` uses a fresh in-memory cookie jar per call and writes no
  cookie file.
- **OAuth2 authorization code** needs a browser, so `run` fails with exit 4 and
  says so. Client credentials work; refresh tokens are held in memory only.
- **`--keychain`** loads keychain-backed environment secrets. `run` never
  prompts for secrets by itself. Without `--keychain`, a request that uses a
  keychain secret fails with exit 4 and `secret NAME is in the keychain; pass
  --keychain`. File-mode secrets (`*.secrets.toml`) load as in the GUI. After
  `--keychain`, the operating system may show its own keychain prompt: on macOS
  access is per binary, so the first use by `wisp-cli` (inside `Wisp.app`) of an
  item the app created may ask for permission.
- **`--history`** appends one history entry as the GUI would, only when the
  collection already has an id; `run` never creates one. Without an id the
  result says `history: skipped (collection has no id)`. Off by default.
- **TLS trust.** `run` never reads the GUI's `settings.toml`; trust comes from
  flags only. By default it trusts the bundled Mozilla roots and the operating
  system's store. `--ca-bundle <file>` adds the certificates in a PEM file
  (at most 1 MiB, certificates only; a private key or any other block refuses
  the whole file). `--no-system-trust` leaves out the operating system's store;
  the bundled roots stay. A bundle that is missing, unreadable or refused exits 3
  before anything is prepared or sent. Both flags are accepted only by `run`.
- **Status.** The status is acceptable when it is in the request's
  `expected_status`, or, without one, below 400. Redirects are followed per the
  request's settings before the check. A `status` assertion is separate: it
  decides exit 1, `expected_status` decides exit 7.
- **Output.** The result lists assertion label, state and skip reason, never
  actual values, and never the response body. Each dynamic value (`{{$uuid}}`
  and the like) is reported once per occurrence.
- **Spill file.** A response body over 10 MiB is spilled to a file in a
  private temp dir. If `run` is killed mid-send, one such file can remain
  there.

JSON fields: `schema`, `command`, `request`, `environment`, `method`, `url`,
`result`, `exit`, `delivery`, `retry_safe`, `status`, `expected_status`,
`status_ok`, `timing_ms` (`ttfb`, `total`), `size`, `error`, `assertions`,
`dynamic`, `history` and `problems`.

#### Delivery and `retry_safe`

`delivery` says whether the request reached the server:

| `delivery` | Meaning |
|---|---|
| `not_sent` | No byte of the request could have been written to any connection: a pre-send failure (bad URL, signing, unreadable body file, a failed OAuth2 token fetch) or a connect, DNS or TLS-handshake failure on the first send. |
| `responded` | A response head arrived, even if the transfer then failed. |
| `unknown` | The request may have been written and nothing proves otherwise: a connection that broke after the send started, or a failure on a redirect hop or an authentication resend. |

The engine computes this fail-closed: anything it cannot prove `not_sent` is
`unknown`. `retry_safe` is true when `delivery` is `not_sent`, or the method
you sent (not a later redirect hop's) is GET, HEAD, OPTIONS, PUT or DELETE. A
POST that failed with `delivery: unknown` is never `retry_safe`.

### snapshot

`wisp-cli snapshot <collection> [--env <name>] [--request <request-path>] [--recent <n>] [--json]`
prints one bounded, secret-free picture of a workspace. It is built from
**persisted files only**: unsaved edits in the app are not included, and the
`source` field says so. It never writes a file; history is read through a
read-only API that never migrates, locks or creates anything.

JSON fields: `schema`, `command`, `wisp_cli`, `source`, `collection`,
`environments`, `tree` and `tree_truncated`, `validation` (`errors`,
`warnings`, `findings`, `findings_truncated`), `request` (only with
`--request`; an `explain` document), `recent` (only with `--recent`; per entry
`at`, `request`, `method`, `url`, `outcome`, `delivery`, `status`,
`environment`, `assertions` as label and state) and `truncated`. There is no
timestamp of the snapshot itself, so two snapshots of an unchanged collection
are byte-identical.

**Bounds.** The tree has at most 2,000 nodes, findings 500, `recent` 50,
environments 200, variable names per environment 500, and the `request`
document 500 variables and 500 headers. Names are cut at 200 characters and
descriptions at 1,000. The whole document is at most 1 MiB. Over that, parts
are dropped in this order, each drop named in `truncated`: `recent`,
`findings`, `tree` entries from the end, the `request` detail (replaced by
`{"omitted":true}`), then each environment's variable list (the count is kept).

The `truncated` list also names the bound that was hit: `environments`,
`environment_variables`, `request_variables` and `request_headers` for the
count limits above, plus `tree`, `findings`, `request` and `recent` for the
drops. An environment that cannot be read adds `environment:<name>` (the name
is cut at 200 characters) and a note on stderr. In the minimal fallback below
that entry is dropped and the stderr note stays.

**Fallback.** If it is still over 1 MiB, only `schema`, `command`,
`wisp_cli`, `source`, the collection info, a count per section and `truncated`
are emitted. The exit code stays 0, with a note on stderr.

## Request metadata

Two optional keys in a request file, both additive (`format = 1` is unchanged):

```toml
description = """
Creates an order. Idempotency key required in production.
"""
expected_status = 201          # or a list: expected_status = [200, 204]
```

- `description` is a TOML string of any length on disk. `explain` and `snapshot` show up to 1,000 characters, cut with an ellipsis; `inspect` only says whether one exists. It is printed as written and not expanded, so a `{{var}}` in it stays literal.
- `expected_status` is an integer from 100 to 599, or an array of 1 to 16 of them. Anything else is an invalid-key error that `validate` reports. It decides `run`'s exit 7 and is shown by `explain`. There is no GUI editor for either key: the GUI shows them read-only.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success. Also a closed output pipe, `--help` and `--version`, and `explain` of a request that exists even when it has unresolved variables (they are the report). |
| 1 | `validate` found at least one error, or writing the output failed. (`run` uses the table below.) |
| 2 | Usage error: no command, unknown command or option, missing or extra argument, an unknown environment name. Usage text goes to stderr. |
| 3 | The collection or request could not be read: not a collection, request not found, a refused path, an unreadable file. |

Errors go to stderr as `wisp-cli: <message>`; results go to stdout.

### `run` exit codes

`run` uses its own table, so that 1 means only "an assertion failed".

| Code | `result` | Meaning |
|---|---|---|
| 0 | `passed` | Response received, status acceptable, no assertion failed (skipped ones do not fail). |
| 1 | `assertion_failed` | Response received, status acceptable, at least one assertion failed. |
| 2 | (usage) | Bad arguments. |
| 3 | (unreadable) | The collection, request or environment cannot be read or parsed, or `--env` names no environment. |
| 4 | `not_prepared` | Resolution failed (unresolved variable, cycle, missing secret, keychain secret without `--keychain`, keychain store failed), or the auth needs interactive authorization. Nothing was sent. |
| 5 | `transport_failed` | The engine failed and `delivery` is `not_sent` or `responded`. |
| 6 | `outcome_unknown` | The engine failed and `delivery` is `unknown`: the request may have reached the server. |
| 7 | `status_unexpected` | Response received, status not acceptable. |
| 8 | `output_failed` | Writing the result failed (not a closed pipe). |

When several apply, the order is: 4, then 5 or 6, then 7, then 1. A response
with a bad status and failed assertions exits 7, and the JSON lists both.

## JSON schema version

Every JSON document carries `"schema": "wisp-cli/1"` (`crates/cli/src/dto.rs:19`).
Additive fields do not change the number; a removed or retyped field does
(spec `plan/specs/2026-11-14-wisp-0-3-0.md`, Feature 4). Output is pretty-printed, one document per run.
