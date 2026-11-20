# 191 — Configure TLS trust for `wisp-cli run`

**Repo:** .
**Round:** 0
**Depends on:** 183, 184, 188
**Owns:**
- crates/cli/**
- crates/service/src/run.rs (run options and the `EngineConfig` it builds)
- Cargo.lock (the `wisp-cli` dev-dependency entries only)

## Goal
`wisp-cli run` can verify a server signed by a private CA, through
`--ca-bundle <file>` and/or the OS store. `--no-system-trust` turns the OS
store off. The CLI never reads the GUI's `settings.toml`, so CI stays
explicit.

## Context
- docs/plan/specs/2026-11-19-wisp-0-5-0.md "Feature 5". This brief assumes open questions 2 and 3 are answered as recommended: system trust on by default, flags only. The orchestrator amends it if not.
- Task 184 API: `TlsTrust`, `load_pem_file`, `EngineConfig.tls`.
- crates/service/src/run.rs:497 `Engine::start(EngineConfig { .. })`.
- crates/cli/src/args.rs: the run flags; crates/cli/tests/run.rs: harness and servers.
- The exit-code table is in `docs/plan/specs/2026-11-17-wisp-0-4-0.md:153-166`; exit 3 is "cannot be read".
- The cli dev-dependencies hold only `tempfile`. Add `rcgen` and `rustls` as dev-dependencies from the workspace (`Cargo.toml:36,47`). 188 changed `Cargo.lock` before this task, which is why it is a dependency.
- Worktree: `./.claude/scripts/wt-new.sh . 191 tls-trust-cli`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-191`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 `run` accepts `--ca-bundle <file>` (once) and `--no-system-trust`, and `run --help` lists both. Any other subcommand given either flag exits 2.
- [ ] AC2 The trust built from the flags reaches the engine for the request and its OAuth2 token requests. With no flags, `run` uses the default trust (system on, no file).
- [ ] AC3 If the `--ca-bundle` file fails to load, `run` exits 3 before any send with a message naming the file (escaped) and the reason. With `--json` it emits the existing exit-3 error shape. No file content is printed.
- [ ] AC4 Tests in `crates/cli/tests/run.rs`, prefix `run_trust_`, against a private-CA TLS server. The leaf has SAN `127.0.0.1`, the serverAuth EKU, and is valid from now−1h to now+30d. Cases:
  - no flags gives exit 5 (`transport_failed`);
  - `--ca-bundle ca.pem` gives exit 0;
  - `--no-system-trust --ca-bundle ca.pem` gives exit 0;
  - `--no-system-trust` with no file gives exit 5;
  - a bad bundle (garbage) gives exit 3, with no request reaching the server;
  - `--ca-bundle` on `inspect` gives exit 2.
- [ ] AC5 `run` reads no `settings.toml` (a grep shows no settings path lookup in `crates/cli` or `crates/service/src/run.rs`).
- [ ] AC6 No file outside Owns changes. The only `Cargo.lock` change is the cli dev-dependency entries. fmt, clippy and the workspace suite pass.

## Checks
- `run-trust` command — AC1, AC2, AC3, AC4
  run: `cargo test -p wisp-cli --locked --test run run_trust_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([6-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `no-settings` command — AC5
  run: `! rg -n 'settings\.toml' crates/cli/src crates/service/src/run.rs`
- `lock-scope` command — AC6
  run: `test "$(git diff main...HEAD -- Cargo.lock | grep -c '^+name = ')" = 0`
- `scope` command — AC6
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/cli/' -e '^crates/service/src/run.rs$' -e '^Cargo.lock$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC6
  run: `cargo test --workspace --locked && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings`

## Out of scope
- The task 196 carry-overs (snapshot, the keychain canary flake, `closed()`), even when they sit in the same files.
- `docs/cli.md` (scribe, task 197).
- Trust for any command other than `run`.
