# 114 — OS keychain SecretStore backend over `keyring` 4.x

**Repo:** `.`
**Depends on:** 109 (done: draft PR #51), 111
**Size impact:** estimated by 109 (PR #51), flagged. With the `keyring` path linked, Windows was +26,112 B and Linux (zbus Secret Service store) +290,960 B. Those figures came from `stat` in a custom workflow, not `measure.yml`, on a base already carrying +5,632 B of spike code, so they are not comparable with `measure.yml` figures (PR #51 review 51-4). The spike did not measure macOS. Nothing in the app calls the backend in this task, so LTO may strip it here. Task 115 is the first to link it into `wisp` and re-measures with `measure.yml` (115 AC10).
**Owns:**
- crates/storage/src/keychain.rs (new; submodules under crates/storage/src/keychain/ allowed)
- crates/storage/src/lib.rs (one `pub mod keychain;` line)
- Cargo.toml (one `keyring` line under `[workspace.dependencies]`)
- crates/storage/Cargo.toml (one `keyring.workspace = true` line)
- crates/storage/tests/keychain.rs (new)
- Cargo.lock
- THIRD-PARTY-NOTICES

## Goal
Implement `SecretStore` (task 111) over the OS keychain on macOS, Windows and Linux with `keyring` 4.x, called blocking. The probe never prompts. A keychain call that hangs never stalls its caller beyond a fixed bound. A store that is unavailable, has failed or has timed out says so; it never pretends to succeed. Values over 2,560 B are refused on every platform. Tests never touch entries they did not create, and never use the production service name.

## Context
- crates/storage/src/vault.rs (from 111): the trait, `SecretStoreError` (Unavailable / Locked / Denied / TooLarge / Other), its `UnavailableReason`, and `SecretValue`
- PR #51 body and `crates/storage/src/keychain_spike.rs:48-80` on branch `task/109-keychain-spike`: the `keyring` calls that worked on all three platforms, and the measured errors
- `keyring` 4.2 `src/v1.rs`: `Entry::new`, `set_secret`, `get_secret`, `delete_credential`, `store_status`. Errors are `keyring_core::Error` (`NoEntry`, `NoStorageAccess`, `PlatformFailure`, `TooLong`, `BadEncoding`, `Ambiguous`, ...).
- docs/conventions.md#dependencies: `default-features = false`, rustls only, notices regenerated after any lock change
- User decisions, 2026-11-12: `keyring` 4.x on all three platforms, not oo7, not the GPUI credentials API and not hand-written per-platform code. A value over the Windows cap is refused as `TooLarge` with no chunking.

## Design (decided)
- **Dependency.** `keyring = { version = "4.2", default-features = false, features = ["v1"] }`. `v1` selects, by target inside `keyring`: macOS `apple-native-keyring-store/keychain`, Windows `windows-native-keyring-store`, Linux `zbus-secret-service-keyring-store` with `crypto-rust`. Never `cli`, never `dbus-secret-service-keyring-store` (it vendors libdbus), never `oo7`.
- **Calls.** Use only `set_secret(&[u8])` and `get_secret()` on the UTF-8 bytes, never `set_password` or `get_password`, so the stored blob is exactly the value's UTF-8 bytes. Non-UTF-8 bytes read back give `Other { code: None }`. The bytes are dropped unformatted. `BadEncoding` and `BadDataFormat` carry the value and are never Debug- or Display-formatted.
- **Linux.** Speak Secret Service generically, as `keyring`'s zbus store does. Do not assume GNOME Keyring: KeePassXC or another provider may own the bus name.
- **Size guard.** `MAX_SECRET_BYTES = 2560`, Windows' confirmed blob cap (109: 2,561 B fails with 0x800706F7). `set` checks it on every platform before any OS call. Over the cap returns `TooLarge`, and nothing is stored or deleted.
- **Timeout bound.** This is how the keychain store meets 111's `SecretStore` trait contract (PR #52, user decision 2026-11-12): an implementation bounds each call and maps a timeout to `Unavailable { reason: TimedOut }`. The whole-call deadline on loads and migrations belongs to 111's loader, not to this store. Each store owns one worker thread. Every OS call goes to it and the caller waits at most `PROBE_TIMEOUT = 5 s` for `probe` and `CALL_TIMEOUT = 30 s` for `get`, `set` and `delete`. 30 s leaves time to answer a macOS keychain access prompt. Both constants are in one place, and tests can set shorter values.

  | worker state | new call | on expiry |
  |---|---|---|
  | idle | runs | caller gets `Unavailable { reason: TimedOut }` and the worker becomes stuck |
  | stuck (an earlier call is still blocked) | `Unavailable { reason: TimedOut }` at once, never queued | — |
  | stuck call returns | its result is discarded and the worker is idle again | — |

  There is never more than one blocked OS call per store, and no thread is spawned per call. Under 111's D1 table, a timed-out probe or `get` fails closed: secrets unresolved, file not read.

  A probe that expires leaves the worker stuck exactly like any other call (PR #51 review 51-1: a rebuilt macOS binary blocked about 20 s on a keychain ACL prompt). A later `get`, `set`, `delete` or `probe` never waits on the stuck probe: it returns `TimedOut` at once, so no caller ever waits past its own bound. Once the probe's OS call returns, the next call runs normally. The probe holds no lock and no state that outlives its OS call.
- **macOS dev builds.** An ad-hoc re-signed rebuild can prompt, or hang about 20 s, when it reads an item a previous build wrote: the keychain ACL names the signing identity. Tests create a fresh `wisp-test-<uuid v4>` service on every run, so they never read an earlier build's item. A signed release has not been tested (109).

## Error mapping (one function, unit-tested)
| `keyring` outcome | `SecretStoreError` |
|---|---|
| `NoEntry` | not an error: `get` → `Ok(None)`, probe → `Ok(())`, delete → `Ok(())` |
| `NoStorageAccess`, a zbus connection or bus error, no Secret Service provider, keychain absent, Windows no logon session | `Unavailable { reason: NoService }` |
| locked collection or keychain (macOS errSecInteractionNotAllowed, Secret Service `IsLocked`) | `Locked` |
| user cancel, prompt dismissed, access refused (macOS errSecUserCanceled / errSecAuthFailed, Windows ERROR_CANCELLED / ERROR_ACCESS_DENIED) | `Denied` |
| `TooLong`, or Windows 0x800706F7 if the guard is ever bypassed | `TooLarge` |
| anything else, including `Ambiguous` and bad encodings | `Other { code }`, carrying the OS status where there is one |

## Acceptance
- [ ] AC1 `keychain::open(service: &str, portable: bool) -> Box<dyn SecretStore>`. When `portable` is true, the store's `probe()` is `Err(SecretStoreError::Unavailable { reason: Portable })` and it makes no OS call.
- [ ] AC2 `probe()` reads one fixed account that does not exist under the service, and makes no write. It maps the outcome through the error-mapping table. A unit test covers every row of that table through the mapping function, using constructed `keyring_core::Error` values and status codes.
- [ ] AC3 `get` on a missing account returns `Ok(None)`. Any other OS error returns `Err` with the variant from the table, carrying a fixed message and the OS status code. It never returns `Ok(None)` for an error.
- [ ] AC4 Size guard, unit-level on the guard on every platform: a 2,560-byte value passes the guard and reaches the backend; a 2,561-byte value returns `TooLarge` without any backend call. A multi-byte UTF-8 value is counted in bytes, not chars. The real Windows store test at 2,560 / 2,561 B is `#[ignore]`.
- [ ] AC5 Timeout, unit-level with a blocking fake backend and short test timeouts: a blocked `probe` returns `Unavailable { reason: TimedOut }` within its bound plus 1 s; while that probe is still blocked, a `get`, a `set` and a second `probe` each return `TimedOut` within 100 ms, never waiting for the probe; after the fake unblocks, the next `get` succeeds. No test sleeps longer than 3 s.
- [ ] AC6 Real round-trip tests, `#[ignore]` so CI skips them, under service `wisp-test-<uuid v4>`: set, get, overwrite, delete, then get returns none. A drop guard deletes only the accounts the test created. No test uses service `wisp`.
- [ ] AC7 Headless Linux: in a container with `DBUS_SESSION_BUS_ADDRESS` unset, `probe()` returns `Err(Unavailable)` within 5 s. This test is `#[ignore]` and run by the `headless` check. The PR records the error `keyring` returned, which settles 109's inconclusive "no bus" case. The `headless-locked` check also tries a password-locked keyring in docker (a session bus with `gnome-keyring-daemon` unlocked with a password, then locked); if it runs, the PR records the error and its mapped variant (expected `Locked`); if docker cannot set it up, the PR says why and the case stays an untested risk.
- [ ] AC8 Dependencies: every `name = ` that `Cargo.lock` adds is reachable from `keyring` (`cargo tree -i <name>` lists `keyring`), and the PR lists each one. `cargo tree -i` for `openssl`, `native-tls`, `libdbus-sys` and `oo7` is empty for every target. For the Linux target, `Cargo.lock` holds exactly one `zbus` package and it is 5.x, so `keyring` reuses GPUI's zbus. The PR lists `keyring`'s own crypto and Secret Service crates by name (the `aes`, `cbc`, `hkdf` and `secret-service` family it adds) before quoting 109's +290,960 B Linux figure. THIRD-PARTY-NOTICES is regenerated.
- [ ] AC9 The Windows build compiles: a `measure.yml` run on this branch succeeds for `x86_64-pc-windows-msvc`. The PR records `binary_bytes` for every target, with the note that the backend is not linked yet, beside 109's figures (Windows +26,112 B, Linux +290,960 B) and the note that those came from `stat` on a spike base, not `measure.yml`.
- [ ] AC10 No value appears in any error, log or Debug output. `debug_redacts_*` tests cover each store type, and a test checks that a sentinel inside a `BadEncoding` error is absent from the mapped error's Debug and Display.

## Checks
cargo writes no JUnit XML here, so tests are `command` checks.
- `unit` command — AC1, AC2, AC4, AC5, AC10
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-114; test "$(cargo test -p wisp-storage --locked --test keychain -- --list 2>/dev/null | rg -c ': test$')" -ge 10 && cargo test -p wisp-storage --locked`
- `real-macos` command — AC3, AC6
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-114; cargo test -p wisp-storage --locked --test keychain -- --ignored roundtrip`
  then confirm no `wisp-test-` item is left: `security find-generic-password -s <the test service printed by the run>` exits non-zero.
- `headless` command — AC7
  run (from the task worktree): `docker run --rm -e DBUS_SESSION_BUS_ADDRESS= -e CARGO_TARGET_DIR=/tmp/target -v "$PWD":/src -w /src rust:1.99.1 cargo test -p wisp-storage --locked --test keychain -- --ignored headless`
- `deps` command — AC8
  run: `test "$(rg -c '^name = "zbus"$' Cargo.lock)" = 1 && cargo tree --locked --target x86_64-unknown-linux-gnu -i zbus | rg -q '^zbus v5\.' && for n in $(git diff main...HEAD -- Cargo.lock | rg -o '^\+name = "([^"]+)"' -r '$1'); do cargo tree --locked --target all -i "$n" | rg -q 'keyring v4\.' || { echo "not from keyring: $n"; exit 1; }; done && for t in x86_64-pc-windows-msvc x86_64-unknown-linux-gnu aarch64-apple-darwin; do for c in openssl native-tls libdbus-sys oo7; do ! cargo tree --locked --target $t -i $c 2>/dev/null | rg -q "$c"; done; done && scripts/notices/generate.sh && git diff --exit-code THIRD-PARTY-NOTICES`
- `headless-locked` command — AC7 (best effort; a docker failure to set up the keyring is reported, not a task failure)
  run (from the task worktree): `docker run --rm -e CARGO_TARGET_DIR=/tmp/target -v "$PWD":/src -w /src rust:1.99.1 sh -c 'apt-get update -qq && apt-get install -y -qq dbus gnome-keyring libsecret-tools >/dev/null && dbus-run-session -- sh -c "echo pw | gnome-keyring-daemon --unlock --components=secrets && secret-tool lock --collection=login && cargo test -p wisp-storage --locked --test keychain -- --ignored headless_locked"'`
- `windows-build` command — AC9
  run: `gh workflow run measure.yml --ref task/114-keychain-backend`, then check the Windows job conclusion is `success`.
- `lint` command — all
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-114; cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `mapping` read — AC2, AC3, AC10
  how: one mapping function; every `keyring` error reaches a table row; `BadEncoding` and `BadDataFormat` payloads are never formatted; only `set_secret` and `get_secret` are called.

## Untested risks (PR #51 review 51-2)
- No session bus at all on Linux: 109 never isolated it. AC7 records the actual error; until it runs, the `NoService` mapping for it is assumed.
- A locked Secret Service collection: 109 approximated "locked" with an uninitialised keyring. Unless `headless-locked` runs, the `Locked` mapping on Linux is untested.
- The probe on a rebuilt macOS binary (51-1): whether a read of a missing sentinel prompts or blocks is unknown. 115 AC9 tests it by hand.

## Out of scope
- App wiring, the refresh-token adapter, the TooLarge banner and UI (task 115).
- Chunking or splitting values over 2,560 B (refused by user decision).
- The GPUI credentials API, `oo7` and hand-written per-platform keychain code: rejected after 109. GPUI's read returned None after a write on Windows, and it labels entries for Zed.
- Migrating or reading any existing entry under service `wisp`.
- Signed-release macOS keychain behaviour. Task 115's hand check covers a dev build.
- Any KeePassXC-specific store, pairing or dependency (a later version).
