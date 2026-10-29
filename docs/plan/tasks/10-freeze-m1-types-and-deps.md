# 10 — Freeze the M1 core types, add M1 dependencies and scaffold modules

**Repo:** .
**Depends on:** none
**Owns:**
- Cargo.toml
- Cargo.lock
- crates/core/Cargo.toml
- crates/storage/Cargo.toml
- crates/core/src/lib.rs
- crates/core/src/model.rs
- crates/core/src/request.rs (new)
- crates/core/src/resolve/mod.rs (new; types only)
- crates/core/src/body.rs (new; module doc comment only)
- crates/core/src/engine.rs (adaptation to the new `PreparedRequest` only)
- crates/core/tests/engine.rs (adaptation only)
- crates/app/src/** (adaptation only)
- crates/storage/src/lib.rs
- crates/storage/src/error.rs (new)
- crates/storage/src/{codec,fs,request,collection,environment,watcher,history}.rs (new; module doc comment only)

## Goal
Every other M1 task runs in parallel against the types this task defines. It
adds all M1 dependencies in one place so no later task touches a manifest or
`Cargo.lock`. It also creates empty module files so no later task edits a
`lib.rs`. The authoring model, the final `PreparedRequest`, the `resolve` input
and error types, and the storage error type are frozen here. Later tasks may
add private items but must not change the shapes listed below. `Event`,
`Command` and `BodyHandle` are not frozen here. Task 16 owns them.

## Context
- crates/core/src/model.rs:1-86 — current M0 types; `Method`, `Header`, `RequestId` stay as they are
- crates/core/src/engine.rs:160-215 — `perform`, which must map the new `PreparedBody`
- crates/app/src/view.rs:88-96 — the one `PreparedRequest` literal in the app
- docs/plan/specs/2026-10-29-http-client-v1.md — section 2 (file format, path params) and section 3 (API)
- docs/conventions.md#dependencies, #errors, #logging

## Types to define (field lists are the contract)

`crates/core/src/request.rs`, the authoring model. All types derive `Debug, Clone, PartialEq, Serialize, Deserialize`, and `Default` where it makes sense. M2 diffs a draft against its saved snapshot with `PartialEq`.
- `Var { name: String, value: String }`
- `KeyValue { name: String, value: String, enabled: bool }`, used for params, headers and form fields
- `Auth`: `None | Inherit | Basic { username, password } | Bearer { token } | ApiKey { name, value, location: ApiKeyLocation }`, with `ApiKeyLocation`: `Header | Query`
- `TextBody { content: String, sidecar: Option<String> }`. `sidecar` is the file name beside the request file, when the body is stored there.
- `Part { name: String, value: PartValue, enabled: bool }`, with `PartValue`: `Text(String) | File(PathBuf)`
- `Body`: `None | Json(TextBody) | Text(TextBody) | Xml(TextBody) | Form(Vec<KeyValue>) | Multipart(Vec<Part>) | File(PathBuf)`. Paths are absolute in memory. Task 17 converts them to and from file-relative paths.
- `RequestSettings { timeout: Option<Duration>, follow_redirects: bool, verify_tls: bool }`. Its `Default` is `(None, true, true)`.
- `Request { name, method: Method, url: String, path: Vec<Var>, params: Vec<KeyValue>, headers: Vec<KeyValue>, auth: Auth, body: Body, vars: Vec<Var>, settings: RequestSettings }`. The default `auth` is `Inherit`.
- `FolderDefaults { vars: Vec<Var>, headers: Vec<KeyValue>, auth: Auth, order: Vec<String> }`. The default `auth` is `Inherit`.
- `CollectionDefaults { format: u32, vars: Vec<Var>, headers: Vec<KeyValue>, auth: Auth, order: Vec<String> }`. The default `format` is `1` and the default `auth` is `None`.
- `Environment { name: String, vars: Vec<Var>, secrets: Vec<Var> }`. It has a hand-written `Debug` that prints secret names but never secret values.

`crates/core/src/model.rs`, which changes `PreparedRequest` and adds types:
- `PreparedRequest { method, url: String, headers: Vec<Header>, body: PreparedBody, timeout: Option<Duration>, client: ClientSettings, warnings: Vec<ResolveWarning> }`. The `url` is fully resolved and encoded and includes the query.
- `PreparedBody`: `None | Bytes(Vec<u8>) | File(PathBuf) | Multipart(Vec<PreparedPart>)`
- `PreparedPart { name: String, value: PreparedPartValue }`, with `PreparedPartValue`: `Text(String) | File(PathBuf)`
- `ClientSettings { verify_tls: bool, follow_redirects: bool, proxy: Option<String> }`. It derives `Hash, Eq` because task 16 keys reqwest clients by it.
- `ErrorKind` gains `BodyFile`. Leave the other variants, including `Cancelled`, for task 16.

`crates/core/src/resolve/mod.rs`, types only. There is no `resolve` function yet:
- `Scope<'a> { collection: &'a CollectionDefaults, folders: Vec<&'a FolderDefaults>, environment: Option<&'a Environment> }`. `folders` runs from outermost to nearest.
- `Location`: `Url | PathParam(String) | Param(String) | Header(String) | Auth | Body | Var(String)`
- `Problem`: `UnresolvedVar { name, location } | MissingPathParam { name } | Syntax { location, message } | VarCycle { name }`
- `ResolveError { problems: Vec<Problem> }`, with a `thiserror` `Display`
- `ResolveWarning`: `PatternMismatch { name, pattern, value } | InvalidPattern { name, pattern, message }`. It is serde-serialisable.

`crates/storage/src/error.rs`: `StorageError` (`thiserror`). Its variants are `Io { path: PathBuf, source: std::io::Error }`, `Parse { path, message }` (TOML syntax, including the line), `Invalid { path, key: String, message }` (wrong type, unknown method or auth type), `UnsupportedFormat { path, found: i64 }`, `NotACollection { path }`, `Watch { message }` and `History { message }`.

`crates/storage/src/lib.rs` declares every storage module as `pub mod` and re-exports `StorageError`. It also defines `pub const TEMP_PREFIX: &str = ".wisp-tmp-";` (atomic-write temp files, which the watcher ignores) and `pub const STATE_DIR: &str = ".wisp";` (the history directory). `crates/core/src/lib.rs` declares `pub mod request; pub mod resolve; pub mod body;` and keeps the existing root re-exports.

## Dependencies to add
Pin these in `[workspace.dependencies]` with `default-features = false`. Each one is already in `Cargo.lock` and already linked into `wisp` with these features through GPUI, gpui-component or reqwest. Adding them should change only the dependency lists of `wisp-core` and `wisp-storage`. Expect no new packages and no change in binary size.
- core: `regex` 1 (`std`, `unicode-perl`), `uuid` 1 (`v4`), `base64` 0.22 (`std`), `percent-encoding` 2 (`std`)
- storage: `wisp-core`, `toml_edit` 0.22 (`parse`, `display`; use 0.22.27, which is already linked at runtime, not 0.25, which is build-time only), `notify` 8 (`macos_fsevent`), `async-channel`, `serde`, `serde_json`, `thiserror`, `tracing`; dev: `tempfile` 3

## Acceptance
- [ ] `grep -c '^\[\[package\]\]' Cargo.lock` gives the same count before and after this change. Otherwise the close-out names each added package and its justification.
- [ ] Every added workspace dependency sets `default-features = false`, and `cargo tree -i openssl` and `cargo tree -i native-tls` both report nothing.
- [ ] Every type and field listed above exists with the stated name. `cargo doc -p wisp-core -p wisp-storage --no-deps` builds.
- [ ] `format!("{:?}", env)` for an `Environment` with a secret `token = "s3cret"` does not contain `s3cret`. A unit test in `request.rs` checks this.
- [ ] `PreparedBody::None` and `Bytes` are sent as before. `File` and `Multipart` produce `Event::Failed` with kind `BodyFile` or `Other` and a message saying they are unsupported until task 16. `client` is ignored apart from what M0 already did.
- [ ] Round-trip serde tests in `crates/core/tests/engine.rs` cover the new `PreparedRequest`.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` pass.
- [ ] `cargo build --release -p wisp-app` succeeds.

## Out of scope
- The `resolve` function body (11), the body store (12) and any storage logic (13–19)
- Changing `Event`, `Command` or the cancellation path (16)
- Any UI change beyond building the new `PreparedRequest` literal
- The PLAN.md open items from 06 and 09 (budgets, measure script P3s, chevron)
