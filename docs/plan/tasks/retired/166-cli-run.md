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
- [ ] AC2 `wisp_service::run::run_once(RunInput) -> Result<RunReport, RunError>` follows spec :268-269: read-only opening as `explain`, `Engine` with `worker_threads: 1`, `MemoryOnly` refresh tokens, a fresh cookie jar, default timeout; `prepare` with `Recording`; `submit`; blocking receive to the terminal event; at most `BODY_CAP + 1` body bytes; handle released before return; `finish`; `history::record` only with `--history` and only when `read_collection_id` returns an id (never `ensure_collection_id`), else `history: skipped (collection has no id)`; a run that never sent (not prepared) reports `history: skipped (not sent)`, and the report never starts as `Failed` (Amendment 1). History entries leave out response headers (Decision, Amendment 1). A body read error gives `None`, not an empty body, as the GUI does. `RunInput` carries a secret-store seam that tests set to `MemoryStore`; the CLI never sets it.
- [ ] AC3 Exit codes and `result` values exactly as spec :155-165 with precedence 4, then 5/6, then 7, then 1. Status acceptable = in `expected_status` when declared, else `< 400`. Exit 8 only when writing the result fails with an error other than broken pipe; other commands keep `EXIT_IO = 1`.
- [ ] AC4 `delivery` comes from `outcome::delivery`; `retry_safe = delivery == not_sent || original method ∈ {GET, HEAD, OPTIONS, PUT, DELETE}`, using the method the user sent. `run` never retries.
- [ ] AC5 Without `--keychain` a request using a keychain-mode secret exits 4 with "secret NAME is in the keychain; pass --keychain"; with it, secrets load through the store (in tests, `MemoryStore`). With `--keychain`, a declared keychain secret with no stored value (`loaded.unresolved` from `load_environment_with`) is problem kind "missing secret", message "secret `<name>` has no value in the keychain" (name only), exit 4; these names go through the same `display_for` shadowing check as the no-flag `unloaded` names, so no lower-scope variable of that name is ever sent (Amendment 1). A keychain store failure and OAuth2 authorization code both exit 4 with a fixed message. File-mode secrets load as in the GUI.
- [ ] AC6 Text output is one line per item as spec :231-238; the status line matches spec :233 exactly (reason phrase, size format), with no extra lines beyond the spec's (Amendment 1); JSON is schema `wisp-cli/1`, `command: "run"`, with exactly the fields of spec :243-256. `url` follows Explain's display rules with the query removed (secret parts as `{secret NAME}`, userinfo hidden). `error` is `{"kind": ...}` with a fixed text per kind, never the engine message. Assertions print label, state and skip reason only. `problems` uses `ProblemDoc` on exit 4. Exits 2 and 3 print to stderr only.
- [ ] AC7 Tests, one per exit code 0, 1, 4, 5, 6 (a server that accepts and never answers, short `timeout_ms`), 7; `expected_status` absent with 404 exits 7; bad status plus a failed assertion exits 7 with both in the JSON; a JSON golden test that also checks the schema and field set; `run_missing_keychain_secret`: collection var `token = "dev-default"`, a keychain env declaring `token` with no stored value in the `MemoryStore`, `--keychain` → exit 4, problem "missing secret", the mock server receives no request (Amendment 1); `--history` text for a not-prepared run says `history: skipped (not sent)` and for a no-id collection `history: skipped (collection has no id)`.
- [ ] AC8 Canary test (spec :274): file secret in URL path, query, header and auth; a plain variable named `api_token`; a keychain env via the `MemoryStore` seam; the server echoes every canary in headers and body; no canary in stdout, stderr, `index.jsonl` or `<id>.req.json`, in both formats, for exits 0, 1, 4, 5, 6, 7; the keychain canary (via `MemoryStore`) is present in the setup of every one of those exits, not only exit 0 (Amendment 1).
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
  how: the reviewer checks the exit mapping and precedence against spec :155-167, the JSON field set against :243-256, the text status line against :233, and that no engine message or resolved URL reaches output.
- `history-shape` read — AC2
  how: the reviewer checks that the `--history` report starts unset/skipped (never `Failed`), uses `skipped (not sent)` / `skipped (collection has no id)`, that the recorded entry carries no response headers, and that a body read error yields `None`.

## Out of scope
- `snapshot`, and `description`/`expected_status` in `explain`/`inspect` (173).
- Folder runs, `--body`, actual values, retries, the GUI send guard (spec non-goals).
- Changing `cli-budgets.json` (172 measures first).

## Wave stopped — step 2 — orchestrator-routed review findings at 2f0eebe
- [P2] crates/service/src/run.rs:236-262: with `--keychain`, a declared keychain secret with no stored value is ignored (`loaded.unresolved` unused) and the resolver falls through to a lower-scope var, so the request is sent instead of exit 4 "missing secret" (spec :159). Needs a problem kind and fixed message, and the names routed through `display_for`.
- [P3] run.rs:370-398: `--history` report starts as `Failed`; a not-prepared run says `history: failed`, and a no-id collection is not `skipped (collection has no id)` (spec :148). Vocabulary for a not-prepared run with an id is unspecified.
- [P3] run.rs:510-511: `--history` drops `response_headers` (canary rule :274) while spec :148 says "as the GUI would"; no Decision recorded.
In-spec findings held until the decision: [P2] AC8 keychain canary not covered on exits 1/4/5/6/7; [P3] body read error → empty body (GUI gives None); [P3] status line lacks reason phrase/size format and has extra lines vs spec :233.

## Amendment 1 (user, 2026-11-17)
Reason: the review findings under "Wave stopped" above. Changed lines: AC2, AC5, AC6, AC7, AC8, Checks (`contract`, new `history-shape`).
1. Missing keychain secret: with `--keychain`, a declared keychain secret with no stored value is problem "missing secret" (name only, never a value), exit 4 (spec :159), routed through `display_for` so no lower-scope variable of that name is sent. New test in AC7.
2. `--history` vocabulary: not-prepared run → `history: skipped (not sent)`; no-id collection → `history: skipped (collection has no id)` (spec :148). The report does not start as `Failed`.
3. Decision: CLI `--history` entries leave out response headers, stricter than the GUI. Reason: storage redacts headers by name only, so a secret value echoed in a response header would otherwise reach `<id>.req.json`, breaking the canary rule (spec :274).
4. Held in-spec findings, in this fix round: AC8 keychain canary on exits 1/4/5/6/7 via `MemoryStore`; body read error → `None` (match the GUI); text status line per spec :233 (reason phrase, size format, no extra lines).

## Wave resumed — user accepted the orchestrator recommendations (Amendment 1)

## Review round 1 — findings
Reviewed HEAD 2f0eebed0d35e2b59b3185efbcf087b4ce0dbb72 (full branch, after Amendment 1). The three P3s below are required by Amendment 1 (items 2 and 4), so they are in this round.
- [P2] crates/service/src/run.rs:236-263 — With `--keychain`, `loaded.unresolved` is ignored, so a declared keychain secret with no stored value is missing from `env.secrets` and resolves to a lower-scope variable of the same name. The request is then sent.
  route: implementer — AC5 and Amendment 1 item 1. Add a "missing secret" problem with the message "secret `<name>` has no value in the keychain", and pass the names through the same `display_for` shadowing as `unloaded`.
  input: collection.toml `[vars] token = "dev-default"`; environments/prod.toml `secrets = "keychain"`, `secret_names = ["token"]`; nothing stored in the MemoryStore for token; request uses `{{token}}`; `run … --env prod --keychain` → the request goes out with `dev-default` instead of exit 4 "missing secret" (vault.rs:442-446 leaves `unresolved` names out of `secrets`).
- [P2] crates/cli/tests/run.rs:622-656 and crates/service/src/run.rs (`run_keychain_secret_loads_through_the_store_and_never_leaks`) — the only keychain canary test covers exit 0. AC8 asks for the keychain canary on exits 0, 1, 4, 5, 6 and 7.
  route: implementer — AC8 and Amendment 1 item 4. Add `run_once` tests with the `MemoryStore` seam for each of those exits.
- [P2] crates/cli/tests/run.rs — the `run_missing_keychain_secret` test AC7 requires is missing, as is the `--history` text test for a not-prepared run (`history: skipped (not sent)`).
  route: implementer — AC7 as amended.
- [P3, required by Amendment 1 item 2] crates/service/src/run.rs:368-372, 80-87 and crates/cli/src/cmd/run.rs:81-88 — the `--history` report starts as `HistoryStatus::Failed`; not-prepared early returns (run.rs:382, 397, 427, 432) print `history: failed`, and a not-prepared no-id collection prints `failed` instead of `skipped (collection has no id)`. No `skipped (not sent)` variant.
  input: `run <coll> a.toml --history` where a.toml uses `{{missing}}` → `history: failed`; expected `history: skipped (not sent)`.
- [P3, required by Amendment 1 item 4] crates/service/src/run.rs:485 — `bodies.read(...).unwrap_or_default()` turns a body read error into an empty body; the GUI gives `None`.
  input: unreadable spill file → `body_prefix: Some(&[])`, so `json $.x exists` reports fail instead of skipped.
- [P3, required by Amendment 1 item 4] crates/cli/src/output.rs:302-336 — status line prints `status 201 (expected 201)   312 ms   1234 B`; spec :233 has `status 201 Created (expected 201)   312 ms   1.2 KB`. The extra `delivery:` line is not in spec :231-238.
Follow-up (not this round): [P3] crates/cli/tests/run.rs:150-154 `closed()` binds and drops a port; another process could take it (possible exit-5 flake).

## Wave verified — ff9207aa4e8e4f4b82e8a05b9d4e70d7b13a98fa

## Outcome
Merged 2026-11-17 as 0abafec (branch head ff9207a). `wisp-cli run <collection> <request-path> [--env] [--keychain] [--history] [--json]` sends one saved request through `wisp_service::run::run_once`, with the exit codes and `result` values of spec :155-165. Verified at ff9207a: workspace suite, clippy and fmt clean; reviewer APPROVE on AC1-AC11. Amendment 1 and review round 1 added the missing-keychain-secret problem (exit 4), the `skipped (not sent)` history vocabulary, no response headers in CLI history entries, `None` on a body read error, the spec status line, and the keychain canary on exits 0/1/4/5/6/7. The keychain AC7/AC8 tests live in `crates/service` because the CLI cannot set the `MemoryStore` seam (AC2).
Follow-ups are in PLAN under "Follow-ups from task 166".
