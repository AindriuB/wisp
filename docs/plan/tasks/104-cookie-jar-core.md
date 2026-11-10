# 104 — In-memory cookie jar in the engine (reqwest cookie provider)

**Repo:** `.`
**Depends on:** 100, 103 (serial: both edit `engine.rs`; this task adds the `EngineConfig.cookies` seam on top of 103)
**Size impact:** UNCERTAIN, HIGH — spec estimate +80-150 KB on Windows. Turning on reqwest's `cookies` feature pulls `cookie_store` 0.22 and `cookie` 0.18 (and with them `time`, and possibly `publicsuffix`), none of which is in `Cargo.lock` today. Use `default-features = false` wherever the tree allows. If the Windows binary goes over 15,500,000 B, stop and report; do not hand-roll a replacement in this task.
**Owns:**
- Cargo.toml, crates/core/Cargo.toml, Cargo.lock, THIRD-PARTY-NOTICES, about.toml (only if a new license id must be accepted, and only a permissive one)
- crates/core/src/cookies.rs, crates/core/src/client.rs
- crates/core/src/engine.rs (only `EngineConfig`'s new `cookies` field, its Debug and Default, and handing it to `ClientCache` in `Engine::start`)
- crates/core/tests/cookies.rs (new)

## Goal
Every request the engine sends goes through its collection's in-memory cookie jar, so cookies set by a response are sent on later matching requests, as browsers and Postman do. PR #42 removed `EngineConfig.cookies` and hid the per-collection jars inside `ClientCache`; this task puts the seam back as `EngineConfig.cookies: CookieJars`, a shared store of jars keyed by collection id, so the app holds a clone and reaches each collection's jar (spec :97). The app can snapshot a jar, load it, delete one cookie and clear it, which is everything task 107 needs. Persistence is not done here.

## Context
- docs/plan/specs/2026-11-09-wisp-0-2-0.md#7-cookie-jar-core and #dependencies-and-size-estimates
- crates/core/src/client.rs:10-30 — `ClientCache::get` builds every client, keyed by `ClientSettings` (which carries `collection`). Its private `jars` map and `pub(crate) fn jar` are replaced by the `CookieJars` from `EngineConfig`. Set `.cookie_provider(jars.jar(settings.collection))` there. Every client of one collection (any TLS, redirect or proxy setting, and 103's token-endpoint clients) shares that collection's jar.
- crates/core/src/cookies.rs — the stub `CookieJar` from 100; crates/core/src/engine.rs:31-60, :141-160 — `EngineConfig` and `Engine::start`. The app builds `EngineConfig { .., ..Default::default() }` (crates/app/src/workspace.rs:623), so the new field needs no app change.
- reqwest 0.13.5 `ClientBuilder::cookie_provider`, trait `reqwest::cookie::CookieStore`
- docs/conventions.md#dependencies (justify the size, `default-features = false`, regenerate notices with cargo-about 0.9.3), #logging
- Task 103 merges first and owns the rest of `engine.rs`. Edit it only for the seam named in Owns; rebase on 103's merge before starting.
- Build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-104`. No app launch.

## Decisions
- `CookieRecord` is the core's plain data shape: name, value, domain, host_only, path, secure, http_only, and `expires: Option<SystemTime>`, where `None` means a session cookie. It mirrors storage's `StoredCookie` (task 102) apart from session support; the app maps between them.
- The jar is always on in memory. Turning it off is not a wave-2 feature.
- One jar per collection id, `None` (no collection) included. The key is the id string the app puts in `ClientSettings.collection`; this task does not interpret it.

## Acceptance
- [ ] AC1 `CookieJar` (cheap `Clone`, `Send + Sync`) implements reqwest's `CookieStore` and is set on every client `ClientCache` builds, as the jar of that client's `collection` (AC8).
- [ ] AC2 Against wiremock: a response with `Set-Cookie: sid=SECRETVAL; Path=/` makes the next request to the same origin send `Cookie: sid=SECRETVAL`. A request to a different host does not send it. A `Secure` cookie is not sent over `http`.
- [ ] AC3 `snapshot(now) -> Vec<CookieRecord>` returns unexpired cookies, session ones included, with `expires: None`. `load(Vec<CookieRecord>)` replaces the contents, and a loaded cookie is sent on a matching request (tested). `remove(domain, path, name) -> bool` and `clear()` work as named (tested).
- [ ] AC4 `generation() -> u64` increases on every change, from a response or an API call. Task 107 debounces saves on it. Tested.
- [ ] AC5 `CookieRecord` and `CookieJar` have hand-written Debug with no cookie value, plus `debug_redacts_cookie_record` and `debug_redacts_cookie_jar` tests on `{:?}` and `{:#?}`.
- [ ] AC6 No OpenSSL or native-tls enters the tree (`cargo tree -i openssl-sys` and `-i native-tls` find nothing), every new crate's license is in `about.toml`'s accepted list, and `THIRD-PARTY-NOTICES` is regenerated.
- [ ] AC7 The Windows release binary is at most 15,500,000 B. The PR states `binary_bytes`, its delta from 14,514,688, and the new lock entries with their versions.
- [ ] AC8 `wisp_core::cookies::CookieJars` (cheap `Clone`, `Send + Sync`, `Default`) has `pub fn jar(&self, collection: Option<&str>) -> CookieJar`, which returns the same jar for the same key and creates it on first use. `EngineConfig` has `pub cookies: CookieJars` (default: empty store), and `Engine::start` gives it to `ClientCache`, which keeps no jar map of its own. Tested through `Engine`: a test holds a clone of the config's `CookieJars`; a response with `Set-Cookie` to a request with `collection = Some("a")` appears in `jar(Some("a")).snapshot(..)` and not in `jar(Some("b"))` or `jar(None)`; a cookie the test `load`s into `jar(Some("b"))` is sent only on collection `b` requests.
- [ ] AC9 (42-8) `CookieJars` and `EngineConfig` Debug print no cookie value (`<redacted>`). `debug_redacts_cookie_jar` (AC5) and a new `debug_redacts_cookie_jars` assert on both `{:?}` and `{:#?}` with a loaded cookie whose value is `SECRETVAL`.

## Checks
cargo writes no JUnit XML here, so tests are `command` checks.
- `cookies` command — AC1-AC5, AC8, AC9
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-104; test "$(cargo test -p wisp-core --locked --test cookies -- --list 2>/dev/null | rg -c ': test$')" -ge 10 && cargo test -p wisp-core --locked`
- `seam` read — AC8
  how: `engine.rs` changes are limited to the `cookies` field, its Debug/Default and the `ClientCache` hand-off; `ClientCache` has no jar map of its own.
- `tls-clean` command — AC6
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-104; ! cargo tree --locked -e normal -i openssl-sys 2>/dev/null | rg -q . && ! cargo tree --locked -e normal -i native-tls 2>/dev/null | rg -q . && scripts/notices/generate.sh && git diff --exit-code THIRD-PARTY-NOTICES`
- `lint` command — all
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-104; cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `size` command — AC7
  run: `gh workflow run measure.yml --ref task/104-cookie-jar-core`; when it finishes, `gh run download <run-id> -n measure-x86_64-pc-windows-msvc -D <scratch> && jq -e '.binary_bytes <= 15500000' <scratch>/measure-x86_64-pc-windows-msvc.json`

## Out of scope
- Writing cookies to disk, the toggle, debounce and the exit flush (tasks 102, 107).
- Choosing the collection id or when a jar is loaded from or saved to disk (task 107).
- `engine.rs` beyond the seam in Owns, `timing.rs`, `auth/oauth2/**` (tasks 103, 105).
- The response-side `Set-Cookie` display in `crates/app/src/cookies.rs`.

## Amendment 1
User decision 2026-11-10: the jar enforces a bundled public-suffix list, so a response cannot set a cookie for a public suffix (`com`, `co.uk`, ...). Such `Set-Cookie` lines are rejected, as browsers do.
- Build every jar with `cookie_store::CookieStore::new_with_public_suffix` (or equivalent), including the jars `load` builds.
- Source of the list: prefer one already in the dependency tree, or a crate that embeds the list. If the only route is fetching `public_suffix_list.dat` from the network, STOP and report: downloads need the user's approval.
- Size: Windows was 14,736,384 B after this task's first commit. Measure again and report the delta from that figure; the 15,500,000 B gate (AC7) still holds, and if it is exceeded, stop and report.

- [ ] AC10 Against wiremock, in one collection: `Set-Cookie: x=1; Domain=com` from `https://evil.com` and `Set-Cookie: x=1; Domain=co.uk` from `https://evil.co.uk` are not stored (absent from `snapshot`) and not sent to `https://api.bank.com` or `https://bank.co.uk`. A `Domain=example.com` cookie from `https://www.example.com` is still stored and sent. The same holds for a jar populated through `load`.
- [ ] AC11 The public-suffix list is parsed once per process (e.g. a `static` `OnceLock`/`LazyLock`), not per jar or per request.
- [ ] AC12 The PR states the list's source (crate and version), the new lock entries, `binary_bytes` and its delta from 14,736,384.

Checks: AC10 is covered by the `cookies` command (its tests live in `crates/core/tests/cookies.rs`); AC11 and AC12 by a `psl` read (how: the list is held in one process-wide static that every jar shares, and no network fetch exists in code or build scripts); the size gate by the `size` command.
