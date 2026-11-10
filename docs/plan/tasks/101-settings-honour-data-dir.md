# 101 — settings.toml honours WISP_DATA_DIR so test launches never touch real settings

**Repo:** `.`
**Depends on:** none
**Size impact:** none
**Owns:**
- crates/app/src/settings.rs (including its inline tests)

## Goal
When `WISP_DATA_DIR` is set to an absolute path, `settings.toml` is read from and written to that directory, so an agent or hand-check launching Wisp with a temp `WISP_DATA_DIR` cannot change the user's real settings. This closes the P2 "settings.toml location ignores WISP_DATA_DIR" follow-up from task 97 (PLAN.md). Every later wave-2 task that launches the app relies on it.

## Context
- crates/app/src/settings.rs:44-78 — `location()` (pure, tested) and `settings_path()` (reads the environment)
- crates/storage/src/data_dir.rs:9-24 — `OVERRIDE_ENV` and the same rule for the data dir: an absolute override wins, a relative one is ignored. Reuse `wisp_storage::data_dir::OVERRIDE_ENV`; do not define a second name.
- docs/architecture.md#settings-location — the scribe updates it after merge; do not edit it here.
- Build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-101`.

## Acceptance
- [ ] AC1 `location()` takes the override as a new argument. Its order is: an absolute override gives `<override>/settings.toml`; otherwise the portable marker gives `<exe-dir>/settings.toml`; otherwise `<config-dir>/wisp/settings.toml`. A relative or empty override is ignored.
- [ ] AC2 `settings_path()` reads the override from `wisp_storage::data_dir::OVERRIDE_ENV`.
- [ ] AC3 Unit tests cover: an absolute override beats the portable marker and the config dir, a relative override is ignored, and no override keeps the old behaviour. The existing `location_prefers_portable_then_config_dir_then_none` still passes, updated only for the new argument.
- [ ] AC4 No test reads or sets the process environment. Tests call `location()` with explicit arguments.

## Checks
cargo writes no JUnit XML here, so tests are `command` checks.
- `settings-unit` command — AC1, AC3, AC4
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-101; test "$(cargo test -p wisp-app --locked settings:: -- --list 2>/dev/null | rg -c ': test$')" -ge 15 && cargo test -p wisp-app --locked settings::`
- `lint` command — all
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-101; cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `env-read` read — AC2, AC4
  how: `settings_path` uses the storage constant; no test touches `std::env`.

## Out of scope
- A separate `WISP_CONFIG_DIR` variable.
- Launching the app. This task needs no launch; the unit tests prove it.
- docs/architecture.md (scribe).
