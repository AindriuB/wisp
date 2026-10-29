# 11 — Implement resolve(): variables, path params, inheritance and encoding

**Repo:** .
**Depends on:** 10
**Owns:**
- crates/core/src/resolve/**

## Goal
Add `pub fn resolve(req: &Request, scope: &Scope) -> Result<PreparedRequest, ResolveError>`.
It turns an authoring `Request` plus its scope into a fully resolved
`PreparedRequest`. It performs no I/O. The UI will call it on every keystroke,
so it must stay cheap and side-effect free. A send is never made with a literal
`{{var}}` in it.

## Context
- crates/core/src/resolve/mod.rs — the frozen `Scope`, `Problem`, `Location`, `ResolveError`, `ResolveWarning` (from 10)
- crates/core/src/request.rs, crates/core/src/model.rs — the input and output types (from 10)
- docs/plan/specs/2026-10-29-http-client-v1.md — section 2, "Rules" and "Path parameters"
- docs/architecture.md — "resolve() is pure"

## Rules to implement
- **Variables.** `{{name}}` is substituted, and surrounding whitespace in the name is trimmed. Lookup precedence is: request `vars`, then folders from nearest to outermost, then environment `secrets`, then environment `vars`, then collection `vars`. Variable values may themselves contain `{{...}}`, which is resolved recursively up to depth 8. A cycle produces `Problem::VarCycle`.
- **Dynamic variables.** `{{$uuid}}` gives a fresh lowercase hyphenated v4 UUID for each occurrence. `{{$timestamp}}` gives Unix seconds as a decimal string. No other `$` names exist. An unknown `$name` is `UnresolvedVar`.
- **Where substitution happens.** Variables are substituted in the URL, path values, enabled param and header names and values, auth fields, `TextBody.content`, enabled form fields and enabled multipart text values. Disabled entries are skipped entirely, and unresolved variables in them are not errors.
- **Path params.** Variables are resolved first, then `{...}`. A `{...}` is a path param only in the path portion, meaning after `scheme://authority` and before `?` or `#`. One segment may hold several params (`{id}-{id2}`, `{name}.{ext}`). `{name}` takes the `path` value with that name, percent-encoded so that only RFC 3986 unreserved characters stay literal (`/` becomes `%2F`). `{*name}` is a catch-all: each `/`-separated piece is encoded and the `/` characters are kept. `{name:regex}` is matched against the whole value. A mismatch adds `ResolveWarning::PatternMismatch` and an uncompilable pattern adds `InvalidPattern`. Neither blocks the request. A param with no `path` entry is `Problem::MissingPathParam`. An unbalanced `{` or `}` in the path is `Problem::Syntax`.
- **Query.** Enabled `params` are appended in order, percent-encoded, with `?` added or `&` if the URL already has a query. Then an `ApiKey` with `Query` location is appended.
- **Headers.** The merge order is collection, then outermost to nearest folder, then request, and the nearer level wins. Names are compared case-insensitively. Disabled entries never override.
- **Auth.** `Inherit` walks from the nearest folder up to the collection. If everything is `Inherit`, or the collection is `None`, no auth is applied. `Basic` adds `Authorization: Basic base64(user:pass)`. `Bearer` adds `Authorization: Bearer <token>`. `ApiKey` adds a header or a query param. A header produced by auth replaces any merged header with the same name.
- **Body.** `Json`, `Text` and `Xml` become `Bytes` with a `Content-Type` of `application/json`, `text/plain; charset=utf-8` or `application/xml` respectively. `Form` becomes urlencoded `Bytes` with `application/x-www-form-urlencoded`. `Multipart` becomes `PreparedBody::Multipart`. `File` becomes `PreparedBody::File`. A `Content-Type` is added only if no merged header already sets one.
- **Settings.** `RequestSettings` maps to `timeout` and to `ClientSettings` with `proxy: None`.
- **Error collection.** Every problem is collected, not only the first, so the UI can mark them all.

## Acceptance
- [ ] Unit tests in `crates/core/src/resolve/**` cover each of the following. Each one is checked by an exact `assert_eq!` on the URL, the headers or the body:
  - The spec's `url = "{{baseUrl}}/orgs/{orgId}/files/{name}.{ext}"`, with `orgId = "{{orgId}}"` and enabled `notify=true`, resolves to `https://api.test/orgs/acme/files/report.pdf?notify=true`.
  - A path value of `a/b` under `{id}` becomes `a%2Fb`. Under `{*path}` it stays `a/b`. A value of `a b` becomes `a%20b`.
  - `{id:\d+}` with value `abc` resolves and has exactly one `PatternMismatch` warning. With value `42` it has none.
  - A variable defined at the request, the nearest folder, an outer folder, the env secrets, the env vars and the collection resolves to the nearest definition, checked by removing levels one at a time.
  - `{{missing}}` in the URL and in an enabled header yields two `UnresolvedVar` problems with locations `Url` and `Header("X-...")`. The same variable in a disabled header yields no problem.
  - `a = "{{b}}"` with `b = "{{a}}"` yields `VarCycle`.
  - `{{$uuid}}` used twice yields two different strings, each matching `^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$`.
  - A request with `Inherit`, a folder with `Inherit` and a collection with `Bearer{token="{{t}}"}` produces `authorization: Bearer <t>`. With all levels `Inherit`, no auth header is produced.
  - `Basic{"user","pass"}` produces `Basic dXNlcjpwYXNz`.
  - Collection header `X-A: 1` with request header `x-a: 2` produces exactly one `x-a: 2`.
  - `{param}` text in the query or the authority is left untouched.
- [ ] `rg -n 'std::fs|tokio|std::net|reqwest' crates/core/src/resolve` finds nothing.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` pass.

## Out of scope
- Changing any type frozen in 10. If a shape is wrong, stop and report.
- Building multipart bytes or reading body files (16)
- Validating the final URL. The engine reports `InvalidUrl`.
- Cookie handling, global variables, and dynamic variables other than `$uuid` and `$timestamp`
