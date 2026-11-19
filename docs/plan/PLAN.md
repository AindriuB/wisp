# Plan

What is open, in priority order. Only `scribe` edits this file.

Each item: one line of what, one line of why it matters, and its blocker if it
has one. When an item is in flight, mark it with its task ids. Delete items you
no longer want rather than letting them rot — a plan nobody trusts is worse
than no plan.

---

## Now

<!--
### <title>
<What it is.> <Why it matters now.>
**Blocked by:** <or omit>
**In flight:** 01, 02
-->

### v0.4.0 (spec agreed)

Spec: `docs/plan/specs/2026-11-17-wisp-0-4-0.md` (service layer, `wisp-cli run`, snapshot, metadata, hardening). Wave 1 tasks 159, 160, 161, 162, 163, 164 and the 165 size measurement are recorded, and wave-2 tasks 175 (Explain metadata, placeholder URL-bar check), 168 (history hardening, read-only history reader), 170 (importer hardening, descriptions), 166 (`wisp-cli run`), 169 (secret store and binding hardening), 171 (Windows hardening: owner-only data dir, real 8.3 test, file identity), 167 (hash file body through one handle) 174 (per-request variables editor) 172 (CLI budgets measurement), 178 (refuse to resend a truncated history body) and 179 (OpenAPI environment names without URL credentials) are recorded; next is 173 (CLI snapshot and metadata) once its dependencies land. 178 must ship in the same release as 168. Task 182 (history scrub no longer empties truncated JSON request bodies; marker written when `append` creates the folder) is recorded; it is a must-ship-before-0.4.0 item (user decision 2026-11-18).

**Size, decided 2026-11-18 (task 172):** every `wisp-cli` limit in `scripts/measure/cli-budgets.json` is 7,340,032 B (7 MiB), the user's decision after Linux measured 45,048 B over the earlier 6 MiB. Measured `wisp-cli`: macOS 3,485,408 B, Windows 4,201,472 B, Linux 6,336,504 B (run 44293903312, main 910d681). The Windows GUI is 15,609,344 B, 290,656 B under the 15,900,000 B soft line and 390,656 B under the 16,000,000 B gate; this is the tightest margin, so 177 re-measures it. The `docs/architecture.md` limits stay with task 176. The wave-1 overage (+26,624 B against +10,240 B allowed) was not cut. Figures: `scripts/measure/results/v0.4.0-wave-2-cli-size.json`.

### Follow-ups from task 182 (history scrub and truncated bodies; not scheduled)

- NOTE: the scrub skip covers every truncated body type, not only JSON. The only gap is truncated files written by a dev build of e78aca2 before 0e4325e's `redact.rs` rule change (dev-only data).
- NOTE: `scrub_truncated_append_keeps_json_body_across_reopen` does not guard the restore (it never scrubs an existing file); `scrub_truncated_file_in_unmarked_folder_keeps_body` does.

### Follow-ups from task 171 (Windows hardening; not scheduled)
- [P3] `is_owner_only` ignores the ACE access mask (`crates/storage/src/win_acl.rs:103-107`), so a single owner ACE with reduced rights counts as owner-only.
- [Note] No test proves `restrict_to_owner` is skipped on the already-owner-only path.
- [P3] If `restrict_to_owner` fails after `create_dir` made an ancestor, that ancestor keeps its inherited ACLs and later calls never fix it (`crates/storage/src/data_dir.rs` ~148); e.g. remove the just-created dir on failure.
- [P3] 139-3: the `windows-sys` entry in `crates/storage/Cargo.toml` lacks `default-features = false`, which `docs/conventions.md:83-84` requires (no build effect; PR #139's description wrongly says it is set).
- [Note] ReFS has 128-bit file IDs; `same_file_entry` compares the 64-bit `nFileIndex`.
- [Note] The 8.3 test could read the short name inside the setup dir instead of a sibling file.
- [Flake] `run_keychain_canary_never_leaks_on_exits_0_1_4_5_6_7` is in `known-flakes.md`; find and fix the cause.

### Follow-ups from task 180 (no-ACL volumes; not scheduled)
- [Note] `volume_flags` in `crates/storage/src/win_acl.rs`: a relative path whose first component does not exist walks to an empty parent and returns `None`, so the DACL is applied and fails closed; unreachable today because the data dir is always absolute.
- [Note] `GetVolumePathNameW` uses a fixed 1024-unit buffer; a longer path makes the query fail, so the DACL is applied (fails closed).

### Follow-ups from task 174 (per-request variables editor; not scheduled)
- [P3] No test that `VarsEditor` wires `set_row_mask`; the pure decision test would pass without the call.
- [Low] `vars_save_autosave_refuses_duplicate_names` duplicates the Ctrl+S test because no autosave caller exists, so the name is misleading; AC3 and Amendment 2 of task 174 also mention an autosave that Wisp does not have. Rename the test and fix the wording, or give autosave a real-entry-point test if one is added.
- [P3] The per-row masking gpui test (135-1) is an accepted residual and a follow-up: only the pure decision test exists.
- NOTE: `write_vars` (`crates/storage/src/codec.rs`) now rewrites `[vars]` in caller order for every caller (request path, environments, collection defaults); all pass file order today.

### Follow-ups from task 167 (hash file through one handle; not scheduled)
- [P3] A same-origin 307/308 SigV4 resend reopens the file by path with `digest: None`, so it is not re-hash-checked (redirect area, task 164).
- [P3] The post-send deadline check covers the response head, not the body read (`crates/core/src/engine.rs:1176-1194`); same late-timer class under engine-thread starvation.
- [NOTE] A head that arrives past the deadline now reports Timeout with delivery `unknown` (exit 6) where `responded` would be accurate.
- [NOTE] No test proves the post-send check is needed (it needs a stalled runtime). `StopOnDrop` has no test of its own.
- [NOTE] The `sparse_file` 4 GiB tests may reserve real disk on NTFS (Windows CI).
- Root cause of the Attempt 2 failure: the engine runs one worker thread, so under load its timer fires late.

### Follow-ups from task 166 (wisp-cli run; not scheduled)
- [P3] `crates/cli/tests/run.rs:150-154` `closed()` binds and drops a port; another process could take it, a possible exit-5 flake.
- [P3] The service keychain-canary test asserts only `Completed` for exits 0/1/7; pin the failed check or bad status.
- [P3] No CLI-level test of `missing-secret` rendering (text and JSON).
- [Low] `crates/service/src/run.rs:978` uses a 200 ms sleep against the "no sleeps" rule.
- [Low] In `run_missing_keychain_secret_is_not_prepared_and_sends_nothing` (crates/service/src/run.rs) the `if carries_id` guard can never be false; bind unconditionally there so a fixture change fails loudly.
- [NOTE] `crates/cli/src/output.rs` `size_text` duplicates `crates/app/src/format.rs`.
- [NOTE] Accepted placement: keychain AC7/AC8 tests live in `crates/service`, because the CLI cannot set the `MemoryStore` seam (AC2).
- [P3] The task 166 HISTORY entry misstates the exit codes: 1 is a failed assertion, 7 an unexpected status, 0 a pass.
- [NOTE] The fallback problem text for exit 4 could read as a secret problem if `prepare` ever fails on something Explain does not model.

### Follow-ups from task 169 (secret store and binding hardening; not scheduled)
- [P3] `collection.toml` becomes 0600 when an id is written (`write_nofollow` ignores the old mode).
- [P3] Windows `open_nofollow` has no file-identity check after open (std has no file-index API).
- [P3] `open_nofollow` lacks O_NONBLOCK; a FIFO swapped in can hang the read.
- [P3] `is_git_tracked` (`crates/storage/src/vault.rs`) has no timeout and fails open on any git error.
- [P3] `gains_moved_name` (`crates/storage/src/environment.rs`) reads via `load_vars`, which follows symlinks.
- [Low] `keyring-core = "1"` is not in `[workspace.dependencies]`.
- [Low] The no-data-dir refusal is only `Invalid{key:"secrets"}`.
- NOTE: the `cfg(windows)` keychain path is unbuilt locally; Windows CI is the proof.
- [Low] On Windows `h3_tracked_check_works_with_a_relative_root` does not test a relative root (the leading `/` is never stripped).
- [P3] Fix the flaky `h3_load_without_a_data_dir_refuses_a_keychain_environment`: change the secret value `t1` to one a random temp path cannot contain (see known-flakes.md).

### Follow-ups from task 178 (truncated history body; not scheduled)
- [P3] `truncated_sidecar_history_body_stays_equal_to_the_stored_body` (`crates/app/src/workspace.rs:~6380`) uses a JSON fixture that the history scrub empties, so it compares empty bodies. A Text sidecar body would hold a real 1 MiB cut body; task 182 fixes the scrub, after which a JSON fixture also works.
- NOTE: a body both redacted and truncated is refused for redaction first; filling the redacted leaf counts as the edit that clears the truncation refusal (the task's Decision).
- NOTE: the doc comment on `stored_body` at `crates/service/src/plan.rs:33-34` still says it exists for redaction; it is now read only by the truncation refusal.

### Follow-ups from task 168 (history hardening; not scheduled)
- [Low] The 0.3.0-index test lists an index written by the current build, not a hand-written 0.3.0 line, so it does not prove an old line loads.
- NOTE: substring matching clears whole Text/XML bodies that contain a fragment inside any word, such as monkey, hockey or surpass (fail safe, accepted under Amendment 4). Over-redaction is also why `X-OAuth-Scopes` is redacted (`auth` substring).
- [Low] AC5's test shows the request/response header separation with `X-Foo-Token` rather than a redacted response `X-Foo`.

### Follow-ups from task 170 (importer hardening and descriptions; not scheduled)
- [P3] `mod.rs:298` `url_host` returns `user:digits` for the host:digits userinfo shape `may_hold_credentials` flags (`https://bob:1234?rest@h2/y` gives host `bob:1234`). Task 179 keeps it out of OpenAPI environment names; the shared function is unchanged.
- [P3] `split_userinfo` carve-outs (scheme-name user, `{{` template, `[`) still let a non-digit password into a host-derived env name with no warning, e.g. `https://http:hunter2?x@h/y` gives `http-hunter2`; closing it needs a change in `crates/storage/src/import/mod.rs`.
- [P3] Task 179: `openapi.rs:389` `suspect ||` is redundant when the URL had no userinfo.
- [NOTE] Needs a user decision before it becomes a task: `openapi.rs:377-381` the "credentials in the URL were dropped" warning interpolates `url_host` of the stripped URL, so `https://u:p@bob:1234?rest@h2/y` prints `bob:1234` (a possible password) in the warning text, though no longer in the file name. Pre-existing; 179 AC3 froze the warning text. For that input no "may contain credentials" warning fires either, because `suspect` is computed before the strip.
- [P3] `postman.rs:186-189` the unsupported-method warning still echoes the raw method text.
- [P3] `write.rs:134` `check_folder_rel` reports a missing folder as `ImportError::Parse` ("cannot parse input").
- NOTE: `curl.rs:417` after an unknown option, a single positional without `://` is still taken as the URL (`curl --frobnicate SECRETVAL` writes it as the URL).
- NOTE: `openapi.rs:222` `desc_is_safe` cannot spot a short opaque token used as an environment name.
- [NOTE] `h4_unknown_option_still_never_takes_secret_as_url` passes on any Err, so it adds nothing beyond the AC2 test.
- NOTE: assertion text is no longer scanned for undefined variables (the engine does not expand it).
- NOTE: OpenAPI description bytes still count against the import budget.

### Follow-ups from task 175 (Explain metadata and preview; not scheduled)
- [P3] `crates/app/src/ui/url_bar.rs:586` `metadata_preview_is_deterministic_and_uses_placeholder` would pass with a Fresh source too (`Problem` has no field for a generated value); the task's no-fresh `rg` check is the real proof.
- [P3] `crates/app/src/ui/explain.rs:403` the capped-description test asserts the trailing "…" but not the 1,000-character length.

### Follow-ups from task 165 (wave-1 size measurement; not scheduled)
- NOTE: spec `docs/plan/specs/2026-11-17-wisp-0-4-0.md:461` expects "CLI unchanged" for wave 1, but `wisp-cli` grew +2,560 B on Windows (1,633,792 B) and +3,312 B on Linux (1,970,056 B); macOS +0. All are far under the 4 MiB limit. Fix the spec wording or note the growth; 172 has set the CLI budgets (7 MiB).

### Follow-ups from task 161 (service outcome and history; not scheduled)
- NOTE (for 166, which is planned from this API): `crates/service/src/history.rs:23-48` `record` never builds `RecordError::Open`, because `HistoryStore::open` cannot fail, so the GUI's `Open` arm at `crates/app/src/workspace.rs:2752` is unreachable.
- NOTE: no test drives the `on_event` path that turns `nothing_sent` and `headers_seen` into the entry's `delivery` (AC7); the AC6 test calls `append_history` directly.
- NOTE: AC8's test exercises `with_data_dir`, not `Workspace::new`; removing the `cfg(test)` arm in `new` fails no test.
- NOTE: the AC1 golden's doc comment (`crates/service/src/outcome.rs:278`) says "What the app built", but `delivery: "responded"` is new in 0.4.0.
- NOTE: the AC1 GOLDEN_JSON is the fb21541 pre-move capture with `outcome` changed to Completed and `delivery` added, not a fresh pre-move Completed capture.
- NOTE: `crates/app/src/workspace.rs:2697-2730` the history-only re-read skips `plan_clash`; if the repaired id equals another open collection's id, this collection's entries go into that collection's history store.
- NOTE: the AC14 test's jar check reads `jar(Some(ID))`, but a Failed id's session cookies live in the canonical-root jar; only the `coll_id`-still-Failed assertion rules out `apply_id`.
- [NOTE] "Give this copy its own id" on a collection whose id is cached `Failed` and whose history was recovered leaves history on the recovered store (Amendment 4 step 5 acts only on a `Known` id); cookies and secrets do not follow it either (Amendment 2).
- [NOTE] a `Failed` re-read that fails after a close-and-reopen of the same root (generation changed) drops its entry with no notice, by the generation design.
- [P3] workspace.rs `append_history`: `recovered.expect("checked above")` relies on a match guard with no adjacent proof comment (docs/conventions.md "Errors"); `if let Some(store) = recovered` would avoid it.
- [P3] test `late_append_after_newid_does_not_pin_old_store`: its `Ready(s)` count cannot tell A from B; compare `s` to B's store.

### Follow-ups from task 160 (wisp-service plan and send; not scheduled)
- [P3] `scripts/check-boundaries.sh:52`: the `resolve(` check (and the task's no-resolve check) misses a path-qualified `wisp_core::resolve::resolve(` call or an aliased import.
- NOTE: `crates/service/src/plan.rs:340` `prepare` always returns empty `dynamics`; the caller must copy them from its `Recording` (`crates/app/src/workspace.rs:2281`). This is a trap for 166's CLI caller, and no test checks that `Workspace::send` stores them.
- NOTE: the task's `suite-and-lint` check says `cargo fmt --check`; `cargo fmt --all --check` is needed when not run from the workspace root.

### Follow-ups from task 164 (engine delivery contract; not scheduled)
- P3: `crates/core/src/resolve/engine.rs:824` has a stale comment ("reqwest returns a 307/308 instead of following it"). `plan` is redundant now that `own_redirects == follow_redirects`, and the `if own_redirects` guard on 301..=303 (~:983) and the comment at :980 describe a split that no longer exists.
- NOTE: `crates/core/src/client.rs:41` `ClientCache` still keys on `follow_redirects`, so two identical clients can exist (two connection pools).
- NOTE: `crates/core/src/timing.rs:26` doc comment is stale. With engine-followed redirects, `Timings` describe the first hop that opened a connection, not the final hop.
- Suggestion: the Digest-refused test would also pass if the challenge were rejected before any retry; assert that a second connection attempt happened.
- Open, partly verified: hyper pooled-request retry only when nothing was written (hyper's own `take_message` conditions not traced); `is_connect()` for proxy CONNECT and `TimedResolver` DNS failures (not traced, no tests).

### Follow-ups from task 159 (DynamicSource; not scheduled)
- NOTE: `lookup_var` (`crates/core/src/resolve/engine.rs:165`) still uses `Fresh`, so GUI hover and highlight (`crates/app/src/highlight.rs:60`, `:122`) generate a new UUID for a variable whose value contains `{{$uuid}}`. The spec names only the URL bar for Placeholder.

### v0.3.0 (released 2026-11-17)
Section moved verbatim to `docs/plan/archive/2026-11-17-v0-3-0-section.md` (PLAN.md was over 100 KB). It holds the decisions, and the still-open follow-ups from tasks 149, 155, 157 and 158 (all P3 or notes, none fixed). Release runbook: `docs/release.md`. User to-do: delete draft release `dryrun-44046922250`; Windows and Ubuntu hand tests of 0.3.0 come later.

### User setup (optional release channels)
**User setup (optional channels):** none of these block 0.2.0.
- SignPath Foundation application, then the repository secret and variables (`scripts/release/sign-windows/README.md`).
- Confirm the winget PackageIdentifier and open the winget-pkgs PR after 0.2.0 is published (`packaging/winget/README.md`).
- Partner Center account, the `MSIX_*` repository variables and Store submission (`packaging/msix/STORE-SUBMISSION.md`).

### Follow-ups from task 163 (storage hardening A; not 0.4.0 unless scheduled)
- [P3] collection.rs:296 `assign_new_collection_id` does not take `with_root_lock`, so it can race `save_collection_defaults` and write the old id back.
- [P3] data_dir.rs:136 `strip_trailing_separators` also strips `\` on unix.
- [P3] data_dir.rs:121 a dir narrower than 0700 is widened to exactly 0700, and setgid/sticky bits are dropped.
- NOTE data_dir.rs:94: a trailing `/.` still gets past the symlink check (not reachable today).

### Storage-safety follow-ups (from task 85, not v0.1.1)
- The Windows test `sidecar_rejects_8dot3_short_name_of_sibling_toml` fails rather than skips on a volume with 8.3 names off; it should return early when `B~1.TOM` does not exist.
- No test fails if the symlink-to-`.toml` check in `sidecar_path` is disabled; Unix is covered by the dev/ino check.
- The reserved-name rule also rejects display names like "CON.txt" whose file slug would be harmless (decided).
- A folder containing `con.toml` can still be moved, while moving `con.toml` itself is refused.

### Hardening backlog (post-0.2.0)
`docs/plan/tasks/99-hardening-0-2-0.md` stays as the backlog: its done ACs are ticked, and the many "-> task 99" citations in this file keep resolving. Open for 0.2.0: AC39 (size gate). Every other unticked AC is deferred past 0.2.0 by triage; split it into new tasks when scheduled. The 🔒 items in "Follow-ups from 0.3.0 wave 1" join this backlog.

### Follow-ups from 0.3.0 wave 2 (tasks 139, 142, 150, 151; PRs #95-#98; not 0.3.0 unless scheduled)
None of these is marked 🔒 in the review notes (🔒 would mark a security-relevant item for the task 99 backlog). Items from task 144 are under "Follow-ups from task 144".
- 151: `explain_auth` walks auth alone, so it can still show a SigV4 region whose variable is hidden by a header; task 140 uses only its kind and origin.
- 151 (P3): a header name that comes from a hidden variable gets the key `MARK h:<name>` and no longer merges with a same-named header, so the report lists a header that is not sent and can duplicate `Content-Type` or `Authorization`.
- 142 (P3): the canary fixture never references `{{api_token}}` or the secret `{{k}}`; the golden JSON checks only four fields; non-File secrets modes are all labelled "keychain" (an Unknown mode from a newer Wisp is mislabelled).
- 142 (P3): `explain_keychain_secret_shadows_lower_scope` does not assert the label (text `secret tok (not set)`, JSON `resolved:false present:false`), and `contains("secret")` always passes. The text says "not set" plus a `keychain: not loaded by the CLI` line, not "unavailable" as AC2 words it.
- 142 (Note): `explain` checks `confined` for symlinks and then `load_request` reopens the file, a TOCTOU race that needs a concurrent writer.
- 139 (P3): `history_scope` shows `Pending` for `IdState::Failed` (the id cannot be read or created); it should be `Off(reason)`.
- (P3) Replace the `1234` canary in `redacts_secrets_in_url_params_vars_and_bodies` with a distinctive one; see `known-flakes.md`.

### Follow-ups from task 148 (PR #108; not 0.3.0 unless scheduled)
- [P3] `root.rs:481-487`: a folder whose `collection.toml` exists but cannot be opened (malformed, a directory, a symlink) shows "Already a collection: opened it" and then "Cannot open: ...".
- [Note] `create.rs:51` `check_env_name` is looser than storage `valid_name`: a leading or trailing '.', '..', '.secrets' and reserved device names get storage's generic "invalid environment name".
- [Note] `sidebar.rs:320-352` edits reach `sync_env` and `choose_env`, beyond the Owns note's "environment select's row only"; the change is confined to that select.

### Follow-ups from task 144 (history hardening; PRs #94, #103)
🔒 marks a security-relevant item; each is also part of the post-0.2.0 hardening backlog above (task 99). Items 103-1 to 103-6 are from the fresh review of PR #103 (round 8); the rest are from the `/verify` reviewer at 33e6c75. All are Low or Nit.
- 🔒 The lock file's symlink check runs lstat and then open, which leaves a window; open with O_NOFOLLOW via `custom_flags` (`history.rs:918-927`).
- 🔒 Refresh and Location credential pairs after `#` (an OAuth implicit-flow fragment) are stored as is; this predates 144 in `redact_url`.
- 🔒 `authority_start` in the import path has the same special-scheme gap (`https:user:pw@h`, `https:\user:pw@h`; `\` is never a slash), so import may keep userinfo (`import/mod.rs:267`).
- 🔒 `redact_ignoring_newlines` does not trim leading or trailing C0 and space as WHATWG does; a request URL like ` https:/alice:secret@h/` would keep its credentials if untrimmed URLs reach storage; check whether they can (`history.rs:147`).
- `open_scrubs_old_entries_once` sleeps 20 ms and relies on mtimes; compare bytes or inode and drop the sleep (`tests/history.rs`).
- Design question: the global `APPEND_LOCK` queues appends across every collection, so N queued appends can wait N × 5 s; consider a per-collection mutex.
- A `mailto:alice@example.com` Location is redacted because `authority_start` returns 0 and the whole value is treated as authority; this over-redaction is pinned by `mailto_location_is_handled_by_the_existing_rule`, and the round-2 decision was left to the orchestrator, so decide it. A no-scheme `a@b` Location is likewise mangled to `@b` (no leak).
- No test for a request URL `https:/alice:secret@h/` (only headers are pinned), and the redaction of a non-special no-slash `custom:a:b@h` is untested.
- The fifth `CONTROL_CHAR_HEADERS` entry (`https://ali\r\nce:sec\r\nret@h/`) is vacuous: JSON escapes CR and LF, so `secret` never appears on disk anyway.
- The `leading_whitespace_headers` tests cover Location and Content-Location only; none pins leading C0 for Refresh or for request URLs.
- `file_scheme_without_userinfo_stays_unchanged` lacks `file:/p` and `file:p` (round-8 Low).
- 103-1: the `authority_start` doc comment does not mention the `file:` two-or-more-slash rule (`history.rs:103-104`).
- 🔒 103-2: stores marked `.scrubbed-1` by unreleased post-#94 main builds never re-scrub `file:\\` values; bump the marker to `.scrubbed-2` before 0.3.0, or record the dev data as accepted.
- 103-3: `file:////srv@x/share` is over-redacted (authority is taken after all the slashes; WHATWG takes it after exactly two); start after two, or pin it with a test.
- 103-4: the new `file:` tests use a fixed request URL, not the looped one.
- 103-5 (Nit): the new `file:` branch duplicates the special-scheme return; merge the conditions.
- 103-6 (Nit): no uppercase `FILE:` or Refresh case in the new `file:` tests.

### Follow-ups from task 147 (assertion and history wiring; not 0.3.0 unless scheduled)
- [Note] AC4's test checks the history records only. `NewEntry.request` still carries assertions with expected values and `response_headers` carries header values; the store strips both (`redact_request` history.rs:449, `redact_response_headers` :1043), as the spec intends, but no test inspects the whole `NewEntry`.
- [Note] A failed or cancelled send recording an empty assertion list is checked by reading only (`show_sending` resets `ResponseState`); no test covers it.

### Follow-ups from tasks 143 and 152 (PRs #101 and #102; not 0.3.0 unless scheduled)
From PR #102's review (https://github.com/AindriuB/wisp/pull/102#pullrequestreview-5927425261):
- [P3] 102-1: the save-time backstop in `Workspace::save` is reached by no integration test; only the open-time inlining is.
- [P3] 102-2: inlining runs after `sanitized_origin`, so `HistoryOrigin.stored_body` still names the sidecar.
- [P3] 102-3: the editor-stash test uses a hand-built `Stash`, not the `BodyEditor`.
- [Nit] 102-4: wording of the `new_tomls` doc comment.

From PR #101 (task 143, save and close):
- [P3] 101-3 and 101-4: Lows the owner deferred; text not recorded on PR #101 or in the 143 task file. 101-1 and 101-2 were fixed in 91f09b5 and a53bbd6.
- [P3] `sidecar_seen` entries are never removed when a tab closes.
- [P3] Keep mine records the sidecar as it is at click time, not the version the refusal saw.
- [P3] The quit prompt fixes its tab list before it opens.
- [P3] Test `a_non_utf8_sidecar_that_changes_again_after_keep_mine_is_refused` does not exercise the refused-then-changed path.
- [P3] TOCTOU between the sidecar check and the write.
- [P3] The sidecar is written before the TOML.

### Follow-ups from 0.3.0 wave 1 (tasks 130-138, PRs #80-#90; not 0.3.0 unless scheduled)
🔒 marks a security-relevant item; each is also part of the post-0.2.0 hardening backlog above (task 99). Task 134's Lows 81-1 to 81-5 are listed below as 81-1 to 81-5.

Accepted residuals (decided, not defects to fix now):
- 135: a templated scheme from a variable, `{{base}}//u:p@h`, keeps its userinfo in `inspect` output (the template hides what the userinfo is).
- 130: a hidden-set variable whose value is empty and not cut lists as `Shown([])`; this reveals only that the value is empty (`report.rs:1472`).

From PR #80 (task 135, `wisp-cli`):
- 🔒 80-15: `safe_url` reads separators inside `{{...}}` (`{{a:b}}://u:p@h` keeps userinfo). Also: `?` in a password prints part of it, and a backslash in a non-special scheme.
- 🔒 80-16: stderr error messages skip `display()`.
- 🔒 80-17: `display()` leaves backslash, U+2028/2029 and Cf (zero-width) characters unescaped.
- 🔒 80-7: the boundary denylist misses `vault::`, `ops::` and `save_`.
- 🔒 80-8: symlinked folders that escape the root are followed.
- 80-18: only name escaping is tested through the binary.
- 80-9: a NotUtf8 parser row; 80-10: a BrokenPipe test; 80-11: drop `dto!(NodeInfo)`; 80-12: `inspect` loads whole requests; 80-14: a read-only test tempdir guard; 80-13 (Nit): test names.

From PR #82 (task 132, environment info):
- 82-1: file mode skips `read_names`, so a malformed `secret_names` is Ok here and Invalid in the loader.
- 🔒 82-2: file mode loads every secret value to collect names; it needs a names-only reader.
- 🔒 82-3: Unknown kind and names keep control characters; strip or escape them in `inspect`.
- 82-4: no symlink-refusal tests (plain and secrets); 82-5: the loader hint is tested only in file mode; 82-6 (Nit): `cx()` leaks the temp dir.

From PR #81 (task 134):
- 81-1: collection.toml is written in place, not atomically; a crash can leave a broken file that create_collection then refuses to overwrite (`ops.rs:511`).
- 81-2: directories created by the call are left behind when the file write fails (`ops.rs:542`).
- 🔒 81-3: a check-then-use race on environments/ could put the new file outside the collection root (`ops.rs:544`).
- 81-4: the returned paths are absolute, unlike the module contract and the other create_* functions (`ops.rs:545`).
- 81-5: no tests for a missing parent folder, the empty tree, environments being a file, or a dangling symlink (`create.rs:25`).

From PR #83 (task 130, explain core):
- 🔒 83-3: variables in JSON, XML and text bodies are listed with values under credential keys (`password:{{pw}}`).
- 83-4: `InvalidPattern` is dropped when a path-param value is not plain.
- 83-5: the query separator is computed on placeholders (`?page` against `&page`).
- 🔒 83-6: fragments of a credential variable's value can reach `problems` through nested-ref errors.
- 83-7: auth origin is never None (collection `Auth::None`).
- 83-8: the parity test compares only the param count; API key query placement is untested.
- 83-9 (Nit): `lookup_all` duplicates the `lookup` precedence.
- 83-10: after the listing budget runs out, every later variable is labelled `TooLong`, though the reason is the budget and not length; add a Budget reason or a report warning.
- `explain_counted` sums only the last listing walker, so a revert that resets without accumulating would undercount and escape the bomb tests; make the counter structural.
- Inline query names are checked raw (`%61`, after `#`), as history does; `finish_url` drops a scheme that carries userinfo; an ordering comment is wanted on the inside-check `Option`.

From PR #84 (task 131, assertions):
- 84-2: codec `same()` compares by Debug output; compare structurally (with NaN) before 84-3.
- 🔒 84-3: derived Debug prints expected values and raw text inside `Request` Debug; write a redacting Debug (after 84-2).
- 84-4: a comment above an Unrecognised `[[assert]]` empties kind, target and op, so fingerprints collide. The reviewer's P3 is the same: a leading comment above `[[assert]]` in `raw_string` gives empty fingerprint fields, and trailing comments or quoted keys also collide.
- 84-5: an inline-array `assert = [{...}]` is silently read as none.
- 84-6 (Nit): JSON deeper than 128 is reported as not JSON; add boundary tests (depth, exactly 1 MiB).
- The codec label for malformed versus not supported (`codec.rs:678-681`).
- Header any-of `matches`: an earlier value over 64 KiB skips the check before a later matching value, so the verdict depends on order; decide whether to skip only that value.
- The haystack cap test is ASCII only; add a multibyte (é) case to pin bytes, not chars. `dfa_size_limit(1 MiB)` is untested (no observable; acceptable).

From PR #86 (task 133, `storage validate`):
- 🔒 86-3: the validate doc says messages never carry file values, but Invalid messages echo the method, body type and auth type, unbounded; fix the doc and the residuals, or use fixed text.
- 86-5: the walk duplicates `scan_dir` and differs (escaping links, canonicalize failures); `read_doc` and `read_format` are `pub(crate)` and unused; share one classifier.
- 86-6 (Nit): the fixture leaks outside its tempdir via `mem::forget`; the symlink-escape fixture leaks a tempdir the same way.
- 86-7 (Nit): the sidecar cap borrows `history::BODY_CAP`; use a named validate constant.
- P3: `MAX_ASSERTIONS` is copied, not imported from `wisp_core::assert`; the warning text hides that entries after the 50th are skipped; the code compares the literal "malformed" instead of `ParseError::Malformed.reason()`.
- P3: a missing-sidecar finding names only the sidecar; add the referencing request (key `body.file`, "sidecar of a.toml").
- P3: the oversize tests use 8 GiB sparse files; use `BODY_CAP + 1` and `MAX_REQUEST_BYTES + 1` so the boundary is tested (a cap mutated to 4 GiB is not caught).
- P3 (flake risk): `ancestor_symlink_loop` and `folder_cap` run under the 20 s `validate_in_time` guard though they cannot hang (`folder_cap` takes about 2.6 s locally); call `validate()` directly.
- 🔒 P3: `folder.toml`, `collection.toml` and environment and secrets files are still read without a regular-file or size check, so a FIFO `folder.toml` blocks validate (and `open_collection`) forever.
- No test reaches validate's truncated (`MAX_FOLDERS`) path, and `set_readonly` leaves the fixture root writable.
- Notes: a missing `collection.toml` still walks the whole tree (`wisp-cli validate ~`), so 2D may stop after "not a collection"; `Report` holds a private root in derived Debug and PartialEq, so 2D must print findings and never `Report` Debug.

From tasks 136 and 137:
- 136: the measure.yml "Check CLI size limit" step passes silently on a non-integer limit (`[` exits 2 inside the `if`).
- 137: `provenance_canary_values_never_stored` does not assert that `index.jsonl` and `<id>.req.json` were among the scanned files, so it is vacuous if the layout moves.
- 137: the task's Checks line `--lib history` matches 0 lib tests (a vacuous check).

### Follow-ups from the 0.2.0 waves (tasks 86-128; archived)
Sections "Follow-ups from task theme-palette" through "Follow-ups from task 93" moved verbatim to `docs/plan/archive/2026-11-17-v0-2-0-followups.md` (PLAN.md was near 100 KB). They are unscheduled P3s and notes from 0.2.0 work; coverage check: the task ids in the cut text all appear in HISTORY.md or `docs/plan/tasks/`, and the residual numbers are code line refs and byte counts.

### Open budget and measurement questions (from M0)
**Residuals (71):** if the screenshot times out or `import` fails silently, the log line is a bare `screenshot: ` with no reason (`measure.sh:236`); `dpkg -S` diversion lines would become a bogus package entry in `packages.txt` (none today); the `unowned:` branch of the packages step has never executed in CI.
**Residuals (65):** the Windows over-budget path (`within: false`, exit 3) has never executed; the logic reads correct and the first real over-budget run is its test.
**Residuals (64):** Linux idle RssAnon is about 4.8 MB higher with the link flags (58,168 vs 53,332 KiB for panic/opt-level alone) while after-request is lower; likely noise, recheck when it next shows in a run. `.cargo/config.toml` target rustflags are replaced by any `RUSTFLAGS` env and need lld (GNU ld rejects `--icf=all`). The responsiveness hand check was done on macOS arm64 only (same speed); Windows and Linux builds were not hand-checked. `panic = "abort"` means any panic ends the process.
**09 review P3s:** `WISP_TRACE window opened` prints from inside the `open_window` build closure, before it returns; `measure.ps1`'s final catch block (`:184-192`) prints only the message, not the stderr tail nor whether stderr was empty.
**Residuals (68):** `measure.ps1` has never run locally (no pwsh); its CIM-failure path (`Get-PrivateWs` returning null) and its bad-budgets and one-element-array checks have not executed at runtime, only been read. The Windows CI job covers the normal path.
**Residuals (73):** macOS portable mode is unsupported by decision, but `settings.rs:70-75` still honours a `portable` marker inside `Wisp.app/Contents/MacOS` (which edits the signed bundle), so the code is out of step with the README. The README's claim that Finder, not `unzip`, sets quarantine is unverified until the user's macOS hand test. Its platform-testing statements describe the state at release and must be confirmed before tagging. It needs a line for the app icon when task 76 lands and must drop any console or runtime caveats when task 77 lands; none are present today.
**Residuals (77):** in a Windows release launched from Explorer all stderr is lost (settings warnings, window-open failures, panic messages under `panic = "abort"`); a crash/error log file or attaching to the parent console would be a future feature. A local `cargo build --release` on Windows without `--target` applies `crt-static` to build scripts and proc-macros too (works on MSVC, noted only).
**Residuals (75):** `scripts/release/check-glibc.sh` discards objdump's own error text on failure, and its header's exit-2 description is narrower than its use. Linux RssAnon rose about 14% on ubuntu-24.04 (idle 61,132 vs 53,376 KiB; after request 51,256 vs 45,096) with no explanation; likely different mesa and libraries, still within budget.
**Residuals (76):** `generate.sh --check` does not flag stale extra files; shellcheck SC2209 false positive at `scripts/icon/generate.sh:18` (`mode=write`) makes shellcheck exit 1; `package.sh:58` `assetutil | grep -q` could SIGPIPE under pipefail on a very large catalog (task 78 will harden it); the macOS runner depends on the unpinned default Xcode having Icon Composer support; Linux window icon and `.desktop` file done by task 87; the user's visual check of default, dark and tinted is part of the release hand test. The README gains an icon line only if it discusses appearance (it does not).

**Residuals (78):** the saved keychain search list is not restored if it was empty (harmless on a fresh runner); the `notarytool` timeout JSON shape is unverified; Windows code signing (SmartScreen) is not done, candidate follow-up Azure Trusted Signing; a macOS portable-mode marker inside the bundle would break the notarised signature (see Residuals (73)).
**Residuals (74):** the non-macOS platform modifier is labelled "Win" where Linux calls it Super (no current binding uses it); only enter, escape and tab get display names, other named keys are uppercased.
**Residuals (63):** `measure.sh:131-133` vulkaninfo fallback may name a different device than GPUI picked, and the JSON does not say which source produced `renderer_device`. `measure.ps1` was not parse-checked locally (no pwsh); only CI covers it.

### Carry-forward: crates/core
**Debug redaction follow-ups (84):** multipart text values (`PreparedPartValue::Text`) still print in Debug; URL path and userinfo secrets are not redacted in `PreparedRequest` Debug (only the query is); `EngineError` messages include the URL; `ResponseCookie` derives Debug and holds cookie values; a proxy password with an unencoded `/`, `?` or `#` still prints in `ClientSettings` Debug (reqwest rejects such proxies anyway). None are in 0.1.1.
**resolve nits (11):** a verbose-mode regex `(?x)...#c` can be misreported as `InvalidPattern`; problem dedupe keys on the Debug string; a test helper has a leftover `eprintln`; a scheme-less URL's `://` search can land in the query (the engine rejects such URLs anyway).
**Template helpers (22):** no resolve-level test pins `apply_path_params` to `path_range_plain` (an unclosed `{{` in the authority). `query_start_is_linear` asserts wall-clock < 100 ms and could flake on loaded CI. `expand` allocates all var refs before the budget check (linear, not unbounded). `lookup_var("")` is Unresolved while `resolve` reports Syntax for `{{ }}`. `query_start` can return a `?` inside a fragment.
**Types test (10, optional P3):** the File/Multipart unsupported test checks only the error kind, not the message.
**Body store (12):** the uid-ownership check on the spill directory has no test (it needs root). Three body tests spill into the real `<temp_dir>/wisp-<uid>` rather than a test base directory. The non-unix spill-directory code is not compiled or tested on this Mac.
**Client cache (16):** `ClientCache` builds a client while holding a std `Mutex`. It is tested only through `ClientCache::get`, not `Engine::submit`.
**Engine tests and limits (16):** the multipart test leaves a temp file behind if an assertion fails. The app's `BODY_LIMIT` is now 64 KiB (first 64 KiB read from `bodies()`).

### Carry-forward: crates/app
**Hand check (69):** the macOS arm64 check of the `.id("…-editor")` removal passed on 2026-11-03 (long Path, Auth and Settings sub-tabs scroll, checked with task 66's build); only the Windows and Linux hand checks remain. Possible flake, unconfirmed: wisp-core `a_20_mib_file_body_streams_with_content_length` failed once in a heavily parallel full-workspace run and passed alone; watch for it.
**Residuals (79):** the Touch Bar Dock-Quit crash root cause is upstream (`accesskit_macos` subclass restore versus KVO) and not reported upstream yet. Cmd-Q and window close now ask before discarding drafts (task 83); Dock Quit and system shutdown still do not (documented in `docs/architecture.md#app-lifecycle`). The `WISP_CLOSE_WINDOW_AFTER_MS` hook calls `quit_app` directly, so `quit-check.sh` does not exercise the close button (hand-tested). No About menu item (GPUI 0.3.10 `OsAction` lacks it). Hide, Hide Others and Show All were added by task 83 on macOS. shellcheck SC2010 (`ls | grep`) in `quit-check.sh`.
**Escape (21):** Escape is bound with context None to `CancelRequest`. Hand-checked in task 41: Escape inside the focused URL input cancels a send (status "Cancelled").
**KvTable (29):** hand-checked in task 35 (typing in Params updates rows and the URL; Tab goes name, value, next name). Accepted residual: above the 50-row virtualisation threshold Tab cannot reach rows outside the viewport, because unpainted rows have no focus handle; scroll-to-focus is deferred. The table only virtualises if its container gives it a definite height (task 31). Optional: `locate` in `crates/app/src/ui/kv.rs` is O(n) per keystroke.
**Folder order (39, optional):** a list naming a request in both forms (`["a","a.toml"]`) has only its first match edited by rename, move and delete. `a.toml` is ambiguous when both `a.toml` and `a.toml.toml` exist. Hand-checked in task 41: rename and delete in the running app update `folder.toml` and the tree follows its order.
**Storage ops tests (23, optional):** `case_alias_sidecar_counts_as_shared` only exercises identity on delete and is vacuous on non-case-folding file systems.
**Palette and highlight (25, optional P3):** `palette.rs:136` lowercases only the first char of a label, so chars with a multi-char lowercase (such as "İ") never match. Write the hover " (uses a secret)" suffix format into the spec for task 31.
**Formatting (26/36, optional P3):** `sniff_kind` strips the BOM after the UTF-8 check, so a sniff window cut inside a BOM reads as Text (not a real-world issue).
**Settings (27):** ToggleTheme and ToggleLayout (task 28) surface `settings::save` errors to the user, because save now returns Err on an unparsable or unreadable file instead of overwriting it.
**Draft and URL sync (24):** accepted residual: `Request.vars` has no `DirtySections` flag (vars change only through external edits in v1), so a draft can be dirty with no section highlighted. `merge_parsed_params` aligns old and parsed params with a 3-level order-preserving LCS (name+value, name, same slot); it is O(n*m) per keystroke, fine at real param counts (about 8 MB at 1000 params). Names containing `=` or `&`, and empty names, do not round-trip (documented on `display_url`). Optional cosmetic: group the test helpers `pairs` and `kv` at the top of the `url_sync` tests.
**Workspace shell (28):**
- *Gaps:* (tab close button landed in task 55.) Routing, history and watcher wiring have no end-to-end tests because gpui test support is not enabled; the open-collection test covers only the `open_needs_confirm` helper.
- *Optional P3:* `bind_keys_indices_match_names` does not prove `key(i)` binds the right action.
**Sidebar (30):**
- *Optional:* a node created inside a collapsed folder is selected but hidden, because the parent is not expanded. An environment literally named "No environment" clashes with the `NO_ENV` entry. The delete-prune `collapsed.retain` has no unit test (correct by inspection).
**URL bar and sub-tabs (31):**
- *Decision on resync:* the "only on DraftReplaced" rule holds for Headers and Settings. Params, the URL input (when unfocused) and Path also resync on `DraftEdited`, behind equality guards so an edit never echoes back.
- *Optional:* a non-numeric or zero timeout is silently ignored in Settings (`settings.rs:73-75`). The URL bar also refreshes on `CollectionChanged` (harmless). A URL that does not round-trip through `apply_url_text` (an enabled param with an empty name) can still produce one extra edit.
**Auth and body editors (32):**
- *Lesson for later UI work:* gpui-component 0.8.1 `InputState::set_value` emits no Change event, so any programmatic set must update the model or draft by hand (the cause of the round-1 P1/P2).
- *Optional:* `path.exists()` stats the filesystem on every render of the File body; a hand-typed relative File path is checked against the process cwd rather than the collection dir, so "File not found" can be wrong; a Choose… pick into a row deleted while the dialog is open is silently dropped.
**Palette and environment switcher (34):**
- *Lesson:* in gpui-pre 0.3.10 key bindings dispatch before key-down listeners, and a context-less binding (such as escape to CancelRequest) beats component-context ones. Overlays must handle such actions with `on_action` on their own div, and an overlay that takes focus must restore it on dismiss.
- *Optional:* with more than 50 environments the active one past the limit is not pre-selected. ⌘K or ⌘E while the palette is open reloads it in place and clears the query rather than toggling it closed.
**Response viewer (33):**
- *Optional P3 (42):* the `jump_still_valid` asserts sit inside the key test and could be split; when the 4 MB boundary falls mid-line the cap line follows a partial final line. Budgets measured: binary 18,998,304 B (+33,088), idle RSS 100,064 KiB, after request 104,512 KiB.
- *Decision:* headers are per-cell selectable divs with ⌘C copy, not the gpui-component `Table`. There is no partial-text selection and no keyboard cell navigation.
- *Optional:* header selection is cleared on every `ResponseChanged`, which sub-tab and Pretty/Raw switches also emit (consider a separate view-change notify). `failed_create_keeps_existing_target` silently skips its assertions when run as root. `File::create` truncates before writing, so a failed Replace leaves no original (not an atomic replace).
**Icon assets (37):**
- *Facts:* 19 icons embedded from gpui-kit-assets 0.8.1 (Apache-2.0, NOTICE kept) in `crates/app/assets/icons/`. If dialogs or notifications are used, add CircleCheck, CircleX and Info. The walk test fails naming any `IconName` used in app sources that is not embedded.
- *Optional:* `main.rs`'s run line was split into `let app = ...with_assets(...); app.run(...)` to avoid a rustfmt reflow.

### Carry-forward: crates/storage
**Codec (13):** `write_vars` never reorders existing keys. Flipping an explicit `enabled = true` to false may drop a trailing comment on that line. New entries in multi-line inline arrays get `toml_edit` default spacing. The Windows readonly-target save path in `atomic_write` is untested. `write_auth` keeps its signature, so callers skip the call when the value equals that file's default and the file had no `[auth]`; use `atomic_write_private` for files that hold secrets.
**Watcher (14):** nested directory renames within the same 100 ms window may emit one spurious `Changed` for a directory-derived path (accepted); the M2 reload must re-check the disk and tolerate `Changed` for non-file paths. On Linux notify follows symlinks into existing linked directories by default; consider `follow_symlinks: false`. The event-time `p.is_dir()` follows symlinks while `skip_directory` does not (harmless).
**History (15):** Concurrent writers could race rotation of `history.jsonl`; v1 has one writer.
**Request files (17):** Method parsers unified in task 58 and `.toml` sidecar names are rejected. Task 85 closed sidecars aliasing sibling request files, trailing dot/space names, reserved device names and Unix hard links; hard links are not detected on Windows (std has no stable identity API). Windows `::$DATA` aliases are not covered. The drive-prefix check rejects any name with `:` as its second byte. Sidecar symlinks have a check-then-use race. `bare_relative_request_path_allows_inside_symlink` uses the process-global `set_current_dir`; safe today because all other tests use absolute paths, but a future relative-path test could flake.
**Collection tree (18, walk fixed in 58):** the cycle rule is now ancestor-only with a 10,000-folder whole-walk budget; directory symlinks pointing outside the root are followed; a broken `folder.toml` fails the whole open; `scope_for` with a non-canonical absolute root returns no folders (callers pass relative paths).
**Environments (19):** names with spaces are now rejected, so an existing environment with a space in its name will not load; on case-insensitive filesystems `Dev` and `dev` map to the same file. A symlinked `environments/dev.toml` is refused on load and save (task 120) but is still listed; one `file_type` error aborts the whole listing. The plain file is written before the secrets file, so a save that half-fails can lose a var that is moving between them.
**Integration test (20, accepted):** the JSON body fixture has no comment because JSON cannot hold one.

## Next

### Follow-ups from task 83 (not v0.1.1)
- No automated test sends the real `Quit` action with a window active. It needs gpui `test-support` in `crates/app/Cargo.toml`, or `quit-check.sh` sending the action instead of AppleScript `terminate:`. The review P1 (the handler did nothing while a window was active) got past the tests for this reason.
- The open-collection prompt (`ui/root.rs:193`) and the delete-from-disk prompt (`ui/sidebar.rs:678`) do not set `prompt_up`, so a quit during one of them with a dirty tab queues a second "Discard unsaved changes?" sheet on macOS. No data is lost.

### Follow-ups from task 88 (not v0.1.1)
- The Cookies column header now scrolls up with the rows instead of staying pinned (accepted by the user, 2026-11-09). Fix: a separate header x-scroller that follows the rows' horizontal offset.
- When the Cookies table is tall, sideways scrolling works only with the pointer over the rows.

## Someday
- KeePassXC secret store (later version; the 111 abstraction allows it).
