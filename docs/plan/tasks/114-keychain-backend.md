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
- [ ] AC8 Dependencies: every `name = ` that `Cargo.lock` adds is reachable from `keyring` (`cargo tree -i <name>` lists `keyring`), and the PR lists each one. Because `Cargo.lock` now holds two versions of `aes`, `cipher`, `hmac` and `inout`, each added package is queried as `name@version` (`aes@0.9.4`, `cipher@0.5.2`, `hmac@0.13.0`, `inout@0.2.6`, and likewise `cmov@0.5.4`, `cpubits@0.1.1`, `ctutils@0.4.8` and the rest of the added set). `cargo tree -i` for `openssl`, `native-tls` and `libdbus-sys` is empty for every target. This task adds no `oo7`: `oo7` 0.6 already comes from `gpui-pre-linux` on main, so the check asserts its package count and its reverse deps (only `gpui-pre-linux`) are unchanged vs main and that no path runs from `keyring`. Its lock entry's dependency list may gain version suffixes (`"aes 0.8.4"`) now that two versions coexist; that is not a change. For the Linux target, `Cargo.lock` holds exactly one `zbus` package and it is 5.x, so `keyring` reuses GPUI's zbus. The PR lists `keyring`'s own crypto and Secret Service crates by name (the `aes`, `cbc`, `hkdf` and `secret-service` family it adds) before quoting 109's +290,960 B Linux figure. THIRD-PARTY-NOTICES is regenerated.
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
  run: `test "$(rg -c '^name = "zbus"$' Cargo.lock)" = 1 && cargo tree --locked --target x86_64-unknown-linux-gnu -i zbus | rg -q '^zbus v5\.' && for n in $(git diff main...HEAD -- Cargo.lock | awk -F'"' '/^\+name = /{n=$2} /^\+version = /{print n "@" $2}'); do cargo tree --locked --target all -i "$n" | rg -q 'keyring v4\.' || { echo "not from keyring: $n"; exit 1; }; done && for t in x86_64-pc-windows-msvc x86_64-unknown-linux-gnu aarch64-apple-darwin; do for c in openssl native-tls libdbus-sys; do ! cargo tree --locked --target $t -i $c 2>/dev/null | rg -q "$c"; done; done && test "$(git show main:Cargo.lock | rg -c '^name = "oo7"$')" = "$(rg -c '^name = "oo7"$' Cargo.lock)" && test "$(cargo tree --locked --target all -i oo7 --depth 1 -e normal --prefix none | sort -u | tr '\n' ' ')" = "gpui-pre-linux v0.3.10 oo7 v0.6.0 " && ! cargo tree --locked --target all -i oo7 | rg -q keyring && scripts/notices/generate.sh && git diff --exit-code THIRD-PARTY-NOTICES`
- `headless-locked` command — AC7 (best effort; a docker failure to set up the keyring is reported, not a task failure)
  run (from the task worktree): `docker run --rm -e CARGO_TARGET_DIR=/tmp/target -v "$PWD":/src -w /src rust:1.99.1 sh -c 'apt-get update -qq && apt-get install -y -qq dbus gnome-keyring libsecret-tools >/dev/null && dbus-run-session -- sh -c "echo pw | gnome-keyring-daemon --unlock --components=secrets && echo seed | secret-tool store --label=wisp-test-locked service wisp-test-locked username wisp-test-locked && secret-tool lock --collection=login && cargo test -p wisp-storage --locked --test keychain -- --ignored locked_keyring_get_is_refused"'`
- `windows-build` command — AC9
  run: `gh workflow run measure.yml --ref task/114-keychain-backend`, then check the Windows job conclusion is `success`.
- `lint` command — all
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-114; cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `mapping` read — AC2, AC3, AC10
  how: one mapping function; every `keyring` error reaches a table row; `BadEncoding` and `BadDataFormat` payloads are never formatted; only `set_secret` and `get_secret` are called.

## Untested risks (PR #51 review 51-2)
- No session bus at all on Linux: 109 never isolated it. AC7 records the actual error; until it runs, the `NoService` mapping for it is assumed.
- A locked Secret Service collection: 109 approximated "locked" with an uninitialised keyring. Unless `headless-locked` runs, the `Locked` mapping on Linux is untested.
- `keyring` 4's platform errors are opaque (they wrap boxed platform errors with no stable status-code accessor), so the error mapping classifies them by their text. The Linux and Windows rows of the table stay unverified until the `headless` and Windows runs record the real errors.
- The probe on a rebuilt macOS binary (51-1): whether a read of a missing sentinel prompts or blocks is unknown. 115 AC9 tests it by hand.

## Out of scope
- App wiring, the refresh-token adapter, the TooLarge banner and UI (task 115).
- Chunking or splitting values over 2,560 B (refused by user decision).
- The GPUI credentials API, `oo7` and hand-written per-platform keychain code: rejected after 109. GPUI's read returned None after a write on Windows, and it labels entries for Zed.
- Migrating or reading any existing entry under service `wisp`.
- Signed-release macOS keychain behaviour. Task 115's hand check covers a dev build.
- Any KeePassXC-specific store, pairing or dependency (a later version).

## Amendment (orchestrator, 2026-11-12)

AC7 `headless-locked`: the decided probe (read one fixed account that does not exist) cannot see a lock on Linux — searching for a missing account triggers no unlock — so a probe on a locked collection returns `Ok(())`. Expected result becomes: probe `Ok(())`; `get` on an existing item in the locked collection returns `Err(Locked)` (or `Denied` if the unlock prompt is dismissed). Update `headless_locked_probe` accordingly.

## Review round 1 — findings

Reviewed HEAD: b062d0ff5fdefe26c7a3da34ed243ea26c7a1661

- [P2] crates/storage/src/keychain.rs:341-365 — On Windows the status code is never extracted: windows-native-keyring-store wraps it as `#[derive(Debug)] struct PlatformError(u32)` (Debug `PlatformError(5)`, decimal), which `extract_code` doesn't parse, so ERROR_ACCESS_DENIED (5), ERROR_CANCELLED (1223) and the too-large code (decimal of 0x800706F7) all map to `Other { code: None }` instead of Denied/Denied/TooLarge, losing the code (AC3). Parse the `PlatformError(<decimal>)` shape.
- [P2] crates/storage/src/keychain.rs:288-290,318-324 — On Linux, zbus-secret-service-keyring-store puts `secret_service::Error::{Locked, Prompt}` inside `NoStorageAccess`, whose Debug text is the bare variant name (`Locked`, `Prompt`); `classify` doesn't match those and falls back to `Unavailable{NoService}`. Map `Locked` → Locked and `Prompt` (dismissed) → Denied; keep the existing D-Bus error-name keywords.
- [P2] crates/storage/tests/keychain.rs:15-34,36-89 — `mapping_covers_every_row` builds error text no real store produces (`code: 5`, `os error 0x800706F7`, `collection IsLocked`, `prompt dismissed`), so it passed with both bugs live. Use the real Debug shapes: Windows `PlatformError(5)`, `PlatformError(1223)`, the decimal too-large code; Secret Service `NoStorageAccess` wrapping bare `Locked` / `Prompt`; keep macOS `code: -25308` style only if that matches apple-native-keyring-store's Debug (check its source in ~/.cargo/registry).
- [P3, optional] crates/storage/src/keychain.rs:210-214 — after a worker panic the `Disconnected` arm clears `busy`/`tx` late and can wipe another caller's claim, allowing two OS calls at once. Only a panicking backend triggers it.
- [P3, optional] crates/storage/tests/keychain.rs:277-288 — `debug_redacts_stores` never formats either store with `{:?}`; make it format both and assert no sentinel.
- Also: update `headless_locked_probe` per the amendment above. Consider `#[doc(hidden)]` on the public test seam (Backend, open_with, map_error).

## Amendment 2 (orchestrator, 2026-11-12)

The `headless-locked` check seeds an item before locking: after unlocking the test keyring, run `secret-tool store` for a fixed `wisp-test-locked` service/account, then `secret-tool lock`, then run the ignored test. `headless_locked_probe` asserts probe `Ok(())` and `get` on that seeded item returns `Err(Locked)` (or `Denied` if a prompt is dismissed) — `get` and `set` take different code paths in zbus-secret-service-keyring-store, and the amendment names `get`. The implementer owns the `headless-locked` command text in this file for that change.

## Review round 2 — findings

Reviewed HEAD: b3bc086d4b09a300312194be28c7e28e53b39eb3

- [P2] crates/storage/src/keychain.rs:186-217 — after a backend panic, an immediate next call can claim `busy`, send into the dying worker's still-open channel, and have its job dropped unrun; its `Idle` was never built, so `busy` stays set and every later call returns `TimedOut` for the life of the store (reviewer reproduced 2972/3000). Build `Idle(busy)` before `Box::new` and move it into the job closure so a dropped job still clears `busy`. Add a test: backend whose probe panics; probe, immediate get, short wait, get again → not TimedOut (recovers).
- [DRIFT → Amendment 2] crates/storage/tests/keychain.rs:364-376 `headless_locked_probe` checks `set` on a new account; per Amendment 2 seed an item and check `get`.

## Attempt 1 — failed (/verify, 2026-11-12)

Tester at 4e15c69: unit, lint, deps (incl. notices) and the workspace suite pass. `headless` fails as written: the filter `headless` also matches `headless_locked_probe`, which has no keyring in that container. Results to record in the PR: no-bus probe → `Err(Unavailable { reason: NoService })` in ~0.8 ms; headless-locked (seeded item, locked, no display so gcr-prompter can't show) → probe `Ok(())`, `get` → `Err(Denied)`.
Reviewer: AC1–AC6 met; AC10 `debug_redacts_stores` never formats either store type.
Fix only these:
1. Rename `headless_locked_probe` so the `headless` check's filter no longer matches it (e.g. `locked_keyring_get_is_refused`) and update the `headless-locked` command filter to the new name.
2. AC10: add an in-crate `#[cfg(test)]` test in keychain.rs that formats `KeychainStore` and `PortableStore` with `{:?}` and `{:#?}` and asserts no service/account/secret sentinel appears.
3. `panicking_backend_does_not_wedge_the_store`: cut iterations so the test's total sleep stays under 1 s (AC5 "no test sleeps longer than 3 s").
