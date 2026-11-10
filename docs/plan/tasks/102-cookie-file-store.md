# 102 — Persisted cookie file in the per-user data dir (storage)

**Repo:** `.`
**Depends on:** none
**Size impact:** low (uses `serde_json`, already linked; no new dependency)
**Owns:**
- crates/storage/src/cookies.rs (new)
- crates/storage/src/lib.rs (one `pub mod cookies;` line only)
- crates/storage/tests/cookies.rs (new)

## Goal
`wisp-storage` can load, save and switch off one collection's persisted cookie file at `cookies/<collection-id>.json` in the per-user data dir. Persistence is off by default. Session cookies and expired cookies never reach disk. A hostile or corrupt file cannot exhaust memory or break startup. The app wires it in task 107.

## Context
- docs/plan/specs/2026-11-09-wisp-0-2-0.md#7-cookie-jar-core and #per-user-data-dir
- crates/storage/src/data_dir.rs:47-110 — `DataDir::cookies_file(id)`, `ensure_private_dir` (0700, refuses a symlinked final component)
- crates/storage/src/fs.rs:161 — `atomic_write_private` (0600)
- crates/storage/src/history.rs — the pattern for a per-user store and its tests (temp dirs, `DataDir::at`, never `DataDir::resolve`)
- docs/conventions.md#logging — `StoredCookie` holds a secret value: hand-written Debug and a `debug_redacts_*` test
- Build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-102`.

## Decisions
- **Toggle state is the file's existence.** On means `cookies/<id>.json` exists, even with an empty list. Off means no file. Nothing goes into `collection.toml` (committed) or `settings.toml` (not per collection).
- Storage defines its own plain `StoredCookie { name, value, domain, host_only, path, secure, http_only, expires: SystemTime }`. It does not use a core type: storage stays free of core runtime types, and the app maps between the two (task 107).
- File shape: `{"version":1,"cookies":[...]}` with `expires` as Unix seconds. Any other `version` is a load error.

## Acceptance
- [ ] AC1 `CookieFile::new(&DataDir, &CollectionId)` has `is_enabled() -> bool`, `enable() -> Result<()>` (writes an empty list if absent), `disable() -> Result<()>` (removes the file; succeeds if absent), `load(now) -> Result<Vec<StoredCookie>, StorageError>` and `save(&[StoredCookie], now) -> Result<()>`.
- [ ] AC2 `save` writes through `atomic_write_private`, so the file is 0600, inside a directory made by `ensure_private_dir` (0700). Tested on unix by mode bits.
- [ ] AC3 `save` drops cookies whose `expires` is at or before `now`. Session cookies cannot be represented, because `expires` is not optional, so the caller must leave them out. `load` drops expired entries too. Tested.
- [ ] AC4 `save` while disabled is a no-op that creates no file. Tested.
- [ ] AC5 Limits: `load` refuses a file over 4 MiB without reading it all, and keeps at most 3,000 cookies. Entries with a name+value over 4,096 bytes or an empty domain are skipped. Each case returns or keeps what the doc comment states, and is tested.
- [ ] AC6 A corrupt or wrong-version file makes `load` return `StorageError` and leaves the file untouched. A symlinked `cookies/<id>.json` is refused by `load` and `save` (unix test).
- [ ] AC7 `StoredCookie`'s Debug prints `<redacted>` for `value`; `debug_redacts_stored_cookie` checks `{:?}` and `{:#?}`. No `tracing` call logs a cookie value.
- [ ] AC8 Tests use temp dirs with `DataDir::at`; none calls `DataDir::resolve()` or reads the environment.

## Checks
cargo writes no JUnit XML here, so tests are `command` checks.
- `cookies` command — AC1-AC8
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-102; test "$(cargo test -p wisp-storage --locked --test cookies -- --list 2>/dev/null | rg -c ': test$')" -ge 10 && cargo test -p wisp-storage --locked`
- `lint` command — all
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-102; cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `no-deps` command — size
  run: `git diff --exit-code main...HEAD -- Cargo.lock crates/storage/Cargo.toml`
- `secrets` read — AC7
  how: no log line or error message includes a cookie value.

## Out of scope
- The in-memory jar and reqwest's cookie provider (task 104); debounce, exit flush and UI (task 107).
- Importing curl `-b` cookies into the jar.
- History-store hardening (task 99).

## Amendment 1
User decision 2026-11-10: `save` never writes a file `load` would refuse.
- `save` caps `domain` at 253 bytes and `path` at 1024 bytes; a cookie over either cap is skipped.
- If the serialized file would exceed the 4 MiB load cap, `save` drops the cookies that expire soonest first until it fits, and keeps the rest.
- [ ] AC9 Saving 1,200 cookies with 4,000-byte values, then `load`, returns Ok with the latest-expiring cookies, and the file is at most 4 MiB. Tested.

## Review round 1 — findings
Reviewed HEAD 4954ed2.
- [P2] `cookies.rs:186` `load` panics on a large `expires`: `UNIX_EPOCH + Duration::from_secs(u64::MAX)` overflows `SystemTime` (on Windows about 1e13 also panics). Use `UNIX_EPOCH.checked_add` and skip the entry when it returns `None`. Test with `expires = u64::MAX` and `10^13`.
- [P2] `cookies.rs:205-228` `save` can write a file `load` refuses. Resolved by Amendment 1.
- [P3, optional] `tests/cookies.rs:136-158` test the exact 4,096 / 4,097 name+value boundary.
