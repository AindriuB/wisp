# 99 — Hardening pass before the 0.2.0 release

**Repo:** `.`
**Depends on:** every 0.2.0 wave 3 task (ids not yet allocated), and PRs #34 (task 94) and #35 (task 93) merged. Runs after wave 3 and before the 0.2.0 release dry run.
**Owns:** (the union; each subtask owns one group below)
- A: crates/core/src/engine.rs, crates/core/src/auth/sigv4.rs, crates/core/tests/engine.rs, crates/core/tests/auth.rs
- B: crates/app/src/ui/request/auth.rs
- C: crates/storage/src/data_dir.rs, crates/storage/src/collection.rs
- D: crates/storage/src/history.rs, crates/storage/tests/history.rs
- E: crates/storage/src/import/curl.rs
- F: crates/storage/src/import/openapi.rs
- G: crates/storage/src/import/postman.rs
- H: crates/storage/src/import/mod.rs, crates/storage/src/import/write.rs, crates/storage/tests/import.rs
- K: .github/workflows/ci.yml

## Goal
Close every security-relevant review follow-up from 0.2.0 wave 1 in one pass rather than in PR fix rounds (user request 2026-11-10). That covers credential leakage, redaction gaps, path and file safety, unbounded work or memory, races and locking, and data loss, plus every P2 and a Windows CI job. **This file is a collection and will be split by the planner when scheduled.** Each lettered group has disjoint files, so each can become one subtask. H is one group because `write.rs` calls into `mod.rs` and both are exercised by `tests/import.rs`.

## Context
- Sources: the "Follow-ups from task 92/95" sections of `docs/plan/PLAN.md` at origin/main 2fa37e6, "task 94" at origin/task/94-importers 38467e2, and "task 93" at origin/task/93-history-store 23ea3a8. Items 34-40 and 34-41 come from the user's round-4 review of PR #34. Task 91 recorded no follow-ups.
- Line numbers are as recorded at those commits. Re-anchor them on the merged base before splitting.
- `docs/plan/specs/2026-11-09-wisp-0-2-0.md#size-gate`: the Windows release binary must stay at or under 15.5 MB.
- `docs/conventions.md#tests`: test names read as sentences, no sleeps, storage tests use temp dirs.
- At 23ea3a8, `history.rs:704` `clear` already takes `APPEND_LOCK` and the file lock, and `evict` (`history.rs:641-652`) rewrites the index before it deletes anything. AC8 and AC9 therefore only add regression tests, unless re-anchoring shows a gap.

## Acceptance
Every item that adds a test names it in the PR description.

**A — core engine and auth**
- [ ] AC1 [33-9, P2, security] `engine.rs:844`: a cross-origin hop keeps the user's `x-amz-*` and `Host` headers. A test shows that a redirect from origin A to origin B sends neither header to B, and that a same-origin hop still sends both.
- [ ] AC2 [95 note] `engine.rs:574` `hash_file`: the file is reopened by path between hashing and sending, and hashing ignores the timeout and cancel. Hash and send must use one open handle, or the send must fail when the hashed bytes differ from the bytes sent. A test shows that a cancel during hashing ends the request with the cancelled outcome.

**B — app**
- [ ] AC3 [33-13] `ui/request/auth.rs:219`: the SigV4 access key field is unmasked. It is masked with the same input mode as the secret key field.

**C — storage data dir and collection ID**
- [ ] AC4 [92] `collection.rs:243` `ensure_collection_id` and `collection.rs:177` `save_collection_defaults` race (read-modify-write; the last rename wins). Both serialise under a lock. A test runs two concurrent `ensure_collection_id` calls on one root and gets the same ID back from both.
- [ ] AC5 [92] `data_dir.rs:91` `ensure_private_dir`: a trailing slash (`/x/link/`) gets past the symlink refusal on macOS. The path is normalised before the lstat, and a unix test shows that `link/` pointing at a directory is refused.
- [ ] AC6 [92] Collection IDs that differ only in case share a history dir on case-insensitive file systems. Hand-written IDs with uppercase letters are rejected or folded to lowercase, as a test shows.
- [ ] AC7 [92] An existing data dir wider than 0700 is not tightened. `ensure_private_dir` sets it to 0700 on unix, as a test shows.

**D — history store**
- [ ] AC8 [93, P2] `clear` against a concurrent `append` can leave that append's files behind. A test runs both in parallel threads; afterwards the bodies dir holds no files the index does not list.
- [ ] AC9 [93] If the index rewrite fails, entries remain whose body files are gone. A test (unix: read-only dir) shows that `load` succeeds for every entry `list` returns.
- [ ] AC10 [93] A `Set-Cookie` with no `name=` before the first `;` (`abc123token`) is stored in plaintext. The whole value is redacted when no `=` precedes the first `;`, as a test shows.
- [ ] AC11 [93] `tests/history.rs` `evicts_oldest_over_the_byte_cap` does not check the cap. It asserts that total stored bytes are at most the cap and that the survivors are the newest contiguous run.
- [ ] AC12 [35-6] `history.rs:537`: the request body in `.req.json` is uncapped. It is cut to `BODY_CAP` with a `truncated` flag, and a test with a 2 × `BODY_CAP` request body shows the cut and the flag.
- [ ] AC13 [35-8] Redaction tests cover only Bearer and two headers. A table test covers every `Auth` variant, `Cookie`, mixed-case header names and `Bearer {{t}}`, and asserts that no secret value appears in `.req.json` or the index.
- [ ] AC14 [93] `history.rs:387` `redact_json_body` re-serialises compactly, so integers beyond i64/u64 become f64. A redacted body keeps such integers exactly (test with `12345678901234567890123`).
- [ ] AC15 [93] `history.rs:408`: restoring masked `{{refs}}` does one `replace` per ref, which is O(refs × body). It is a single pass, and a test with 10,000 refs in a 1 MiB body finishes without a wall-clock assert (proved by counting passes or by reading the code).
- [ ] AC16 [93] `history.rs:283` `redact_request`: path vars (`Request.path`) are not redacted. Secret-looking path vars get the same name heuristic as query params, as a test shows.
- [ ] AC17 [93 note] `history.rs:497` `lock_file` blocks with no timeout, so a stalled second process stalls every append. The lock is tried with a bounded wait, and on timeout the append returns an error (the entry is dropped and a notice is shown), as a test holding the lock shows.

**E — curl importer**
- [ ] AC18 [34-10, security] `curl.rs:477`: `--digest`, `--ntlm` and `--negotiate` with `-u` are imported as Basic. `--digest` maps to Digest auth. `--ntlm` and `--negotiate` keep the credential in secrets, add no auth and add a warning. Tested.
- [ ] AC19 [34-11, security] `curl.rs:401`: warnings echo raw headers, extra URLs and fields. A warning names the construct, never its value; a test feeds a `SECRETVAL` into each warning path and asserts that no warning contains it.
- [ ] AC20 [34-40] `curl.rs:1025` `expand_options_consume_their_argument_and_route_credentials` asserts only that the secret is absent. It also asserts where each value went (the secrets environment, or the auth or header ref).

**F — OpenAPI importer**
- [ ] AC21 [34-22] `openapi.rs:451-484`: the example budget is charged after `to_string_pretty`, so a 9.8 MiB example peaks near 728 MB. The budget is charged while serialising (a size-limited writer), and the pretty string is never built past the budget. Tested by a deep example rejected with `TooLarge`.
- [ ] AC22 [94] `openapi.rs:265`: a server `description` becomes the environment file name, so a credential typed into a description reaches a file name. The name comes from the URL host (userinfo stripped) unless the description passes the secret-name heuristic, as a test shows.
- [ ] AC23 [34-15] `openapi.rs:181` `substitute` is quadratic within the 64 KiB cap. It is linear: one pass building the output.
- [ ] AC24 [34-16] `openapi.rs:526`: the depth test never reaches `check_json_depth`. The test input passes every earlier gate and is rejected by `check_json_depth` (assert the specific error).
- [ ] AC25 [34-27] The capped-names test accepts both `Ok` and `TooLarge` and asserts a time under 900 ms. It pins one outcome and drops the timing assert.
- [ ] AC26 [34-41] `openapi.rs:251`: the userinfo warning re-parses the host with `split_once` and can name the wrong host. The host comes from the parsed URL (`url::Url::host_str`) after userinfo is stripped. A test with `https://u:p@h1/x?y=@h2` names `h1` and contains neither `u` nor `p`.

**G — Postman importer**
- [ ] AC27 [94] Postman folders count toward no cap (about 870k empty folders fit in 10 MiB). Folders count toward the node cap, as a test shows.
- [ ] AC28 [34-34] `postman.rs:65`: generated secret names can shadow collection variables. Collection variable keys are reserved in `SecretSink` before any secret is named, and a test shows that no generated name equals a collection var key.
- [ ] AC29 [34-35] `postman.rs:195`: an unsupported auth type falls back to Inherit and is then overwritten by URL userinfo. Userinfo becomes Basic only when the request has no auth object, as a test shows.

**H — import shared and writer**
- [ ] AC30 [94] `mod.rs:301` `secret_ref`: a value containing `{{` anywhere is kept literally (`Bearer live-tok {{x}}`). Only a value that is exactly one `{{name}}` is kept; anything else goes to secrets. Tested.
- [ ] AC31 [94] `mod.rs:268` `split_userinfo`: a password containing raw `/`, `?` or `#` stays in the URL. The authority ends at the last `@` before the first `/` that follows it, or the URL is refused with a warning. Either way the TOML holds no password, as a test shows.
- [ ] AC32 [34-13] `write.rs:46`: `folder_rel` is validated during the write. It is validated before the first file is created; a test with a bad `folder_rel` leaves the collection dir unchanged.
- [ ] AC33 [94] An environment name that is a reserved device name (`con`) aborts the import partway. Environment names are validated or suffixed before any write, and a test imports a server description `con` successfully.
- [ ] AC34 [94] `write.rs:169` `unique` is quadratic in colliding names. It uses a HashSet and a per-base counter.
- [ ] AC35 [34-20] `write.rs:207` `unique_stem` is quadratic when names share a slug. It uses a per-slug counter.
- [ ] AC36 [34-21, security] `write.rs:122`: a `{{var}}` that is used but not defined by the import produces no warning. The import adds one warning per undefined name (the name only), as a test shows.
- [ ] AC37 [34-36] `tests/import.rs:199`: the credential end-to-end test skips URL userinfo and the secrets `.gitignore`. It covers both: no userinfo in any TOML, and `.gitignore` lists the secrets file.

**K — CI and release gate**
- [ ] AC38 [93, P2] CI builds only on ubuntu and macos. `ci.yml` has a `windows-latest` job running `cargo test --workspace --locked` (or at least `cargo test -p wisp-core -p wisp-storage` plus `cargo check -p wisp-app`), and that job is green on the PR.
- [ ] AC39 Size gate: the Windows release binary from the measure run on the final merged head is at most 15,500,000 B.

## Checks
Test counts: the scheduling planner sets `min-passing` per subtask against the base it measures then (`docs/conventions.md#task-file-baselines`). cargo writes no JUnit XML here, so the checks below are commands that count `passed`.
- `core` command — AC1, AC2
  run: `cargo test -p wisp-core --test engine --test auth`
- `storage` command — AC4-AC37
  run: `cargo test -p wisp-storage`
- `suite-and-lint` command — all
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `windows-ci` command — AC38
  run: `gh pr checks <pr> | rg -i 'windows.*pass'`
- `size-gate` read — AC39
  how: the measure run's Windows `binary_bytes` is at most 15,500,000.
- `masking` read — AC3, AC15, AC23, AC34, AC35
  how: the access key input uses the masked mode; each named loop is single-pass or uses a counter map.

## Out of scope
- The task-86 follow-ups (custom-header cross-origin, same-size file resend, URL in `EngineError`). They are not among this task's sources; AC1 must not widen into them unless the scheduler adds them.
- Plain P3s, style, NOTEs and test-only nits left in PLAN (the list is in the PR for this file).
- Making `import::write` transactional (rollback). AC32 and AC33 only validate before the first write.
- The opt-in setting for unredacted `Set-Cookie` (user IDEA, 2026-11-10).
