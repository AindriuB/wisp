# 166 — wisp-cli run: send a saved request through the service (2A, C2b, E4, D-F)

**Repo:** .
**Round:** 0
**Depends on:** 159, 161, 162, 165
**Owns:**
- crates/cli/**
- crates/service/src/run.rs (new)
- crates/service/src/lib.rs (insert-only: `pub mod run;`)
- Cargo.lock (the `wisp-cli` entry gains `wisp-service`; no other change)

## Goal
`wisp-cli run <collection> <request-path>` sends one saved request through the
same service path as the GUI, evaluates its assertions, and exits with a code
that tells CI exactly what happened, including when the outcome is unknown. It
never retries, never prompts, and prints no secret.

## Context
- docs/plan/specs/2026-11-17-wisp-0-4-0.md "Feature 2" (:140-284) — binding: command :144-151, exit codes and precedence :153-169, delivery and `retry_safe` :171-193, output :227-264, implementation :266-270, security :272-276, tests :278-280; invariants :430-437; Non-goals :32-44 (no retries, no body output, no actual values, OAuth2 auth code fails with exit 4, fresh in-memory cookie jar); Waves 2A (:467); Risk 5 (:498).
- Service API from 160/161: `plan::prepare`, `send::submit`, `outcome::{finish, delivery, status_ok, Terminal, Response}`, `history::record`, `secrets::KeychainStores`; `resolve::Recording` (159); `Request.expected_status` (162).
- crates/cli/src/args.rs (parser, `USAGE`), main.rs:16-18 (exit constants), cmd/explain.rs:93 (read-only opening to copy), dto.rs:175 `ProblemDoc`; crates/core/src/resolve/report.rs (URL display rules); crates/core/src/engine.rs:32 `EngineConfig`, :62 `MemoryOnly`; body cap read `crates/app/src/workspace.rs:2864`; spill `crates/core/src/body.rs:14,162`.
- `scripts/check-boundaries.sh:23-26`: `crates/cli/src` must never contain `Engine`, `tokio`, `reqwest`, `keychain::`, `net::` etc. (comments included); the deny-list does not change.
- Tests use a `std::net::TcpListener` mock server (no new dev-dependency); keychain tests use `MemoryStore`/`NoStores` (`storage/src/vault.rs:100-125`), never the real keychain.
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 166 cli-run`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-166`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 `args.rs` parses `run <collection> <request-path> [--env <name>] [--keychain] [--history] [--json]`; the parser table test gains rows for each flag, a missing request path, an unknown flag and `--keychain` on another command (usage error). `USAGE` lists `run`. Parsing stays hand-rolled.
- [ ] AC2 `wisp_service::run::run_once(RunInput) -> Result<RunReport, RunError>` follows spec :268-269: read-only opening as `explain`, `Engine` with `worker_threads: 1`, `MemoryOnly` refresh tokens, a fresh cookie jar, default timeout; `prepare` with `Recording`; `submit`; blocking receive to the terminal event; at most `BODY_CAP + 1` body bytes; handle released before return; `finish`; `history::record` only with `--history` and only when `read_collection_id` returns an id (never `ensure_collection_id`), else `history: skipped (collection has no id)`. `RunInput` carries a secret-store seam that tests set to `MemoryStore`; the CLI never sets it.
- [ ] AC3 Exit codes and `result` values exactly as spec :155-165 with precedence 4, then 5/6, then 7, then 1. Status acceptable = in `expected_status` when declared, else `< 400`. Exit 8 only when writing the result fails with an error other than broken pipe; other commands keep `EXIT_IO = 1`.
- [ ] AC4 `delivery` comes from `outcome::delivery`; `retry_safe = delivery == not_sent || original method ∈ {GET, HEAD, OPTIONS, PUT, DELETE}`, using the method the user sent. `run` never retries.
- [ ] AC5 Without `--keychain` a request using a keychain-mode secret exits 4 with "secret NAME is in the keychain; pass --keychain"; with it, secrets load through the store (in tests, `MemoryStore`). A keychain store failure and OAuth2 authorization code both exit 4 with a fixed message. File-mode secrets load as in the GUI.
- [ ] AC6 Text output is one line per item as spec :231-238; JSON is schema `wisp-cli/1`, `command: "run"`, with exactly the fields of spec :243-256. `url` follows Explain's display rules with the query removed (secret parts as `{secret NAME}`, userinfo hidden). `error` is `{"kind": ...}` with a fixed text per kind, never the engine message. Assertions print label, state and skip reason only. `problems` uses `ProblemDoc` on exit 4. Exits 2 and 3 print to stderr only.
- [ ] AC7 Tests, one per exit code 0, 1, 4, 5, 6 (a server that accepts and never answers, short `timeout_ms`), 7; `expected_status` absent with 404 exits 7; bad status plus a failed assertion exits 7 with both in the JSON; a JSON golden test that also checks the schema and field set.
- [ ] AC8 Canary test (spec :274): file secret in URL path, query, header and auth; a plain variable named `api_token`; a keychain env via the `MemoryStore` seam; the server echoes every canary in headers and body; no canary in stdout, stderr, `index.jsonl` or `<id>.req.json`, in both formats, for exits 0, 1, 4, 5, 6, 7.
- [ ] AC9 Read-only test (spec :275): without `--history`, the collection tree (listing, sizes, mtimes), a temp `WISP_DATA_DIR` and the spill dir are unchanged; with `--history` the only new files are under `history/<id>/`.
- [ ] AC10 Two-process test: two concurrent `wisp-cli run --history` processes on one collection both append, and the index holds two parseable entries.
- [ ] AC11 `scripts/check-boundaries.sh` passes unchanged; `crates/cli/src` names no `Engine`, `tokio`, `reqwest` or `keychain::`. No file outside Owns changes; fmt, clippy and the workspace suite pass.

## Checks
- `cli-run` command — AC1, AC3, AC5, AC6, AC7, AC8, AC9, AC10
  run: `cargo test -p wisp-cli --locked run 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([2-9][0-9]|[1-9][0-9]{2,}) passed; 0 failed; 0 ignored'`
  (put `run` in every new test name; at least 20 tests.)
- `service-run` command — AC2, AC4, AC8
  run: `cargo test -p wisp-service --locked run:: 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9][0-9]* passed; 0 failed; 0 ignored'`
- `boundaries` command — AC11
  run: `scripts/check-boundaries.sh && git diff --exit-code main...HEAD -- scripts crates/core crates/storage crates/app crates/service/Cargo.toml crates/service/src/plan.rs crates/service/src/send.rs crates/service/src/outcome.rs crates/service/src/history.rs`
- `suite-and-lint` command — AC11
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `contract` read — AC3, AC4, AC6
  how: the reviewer checks the exit mapping and precedence against spec :155-167, the JSON field set against :243-256, and that no engine message or resolved URL reaches output.

## Out of scope
- `snapshot`, and `description`/`expected_status` in `explain`/`inspect` (173).
- Folder runs, `--body`, actual values, retries, the GUI send guard (spec non-goals).
- Changing `cli-budgets.json` (172 measures first).
