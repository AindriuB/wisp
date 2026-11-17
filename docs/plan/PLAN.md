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

Spec: `docs/plan/specs/2026-11-17-wisp-0-4-0.md` (service layer, `wisp-cli run`, snapshot, metadata, hardening). Next step is `/plan` from the spec's Waves section.

### Follow-ups from task 164 (engine delivery contract; not scheduled)
- P3: `crates/core/src/resolve/engine.rs:824` has a stale comment ("reqwest returns a 307/308 instead of following it"). `plan` is redundant now that `own_redirects == follow_redirects`, and the `if own_redirects` guard on 301..=303 (~:983) and the comment at :980 describe a split that no longer exists.
- NOTE: `crates/core/src/client.rs:41` `ClientCache` still keys on `follow_redirects`, so two identical clients can exist (two connection pools).
- NOTE: `crates/core/src/timing.rs:26` doc comment is stale. With engine-followed redirects, `Timings` describe the first hop that opened a connection, not the final hop.
- Suggestion: the Digest-refused test would also pass if the challenge were rejected before any retry; assert that a second connection attempt happened.
- Open, partly verified: hyper pooled-request retry only when nothing was written (hyper's own `take_message` conditions not traced); `is_connect()` for proxy CONNECT and `TimedResolver` DNS failures (not traced, no tests).

### v0.3.0 (released 2026-11-17)
Explain Request, Save and close with the environment guard, assertions with history provenance, and a read-only `wisp-cli`. Spec: `docs/plan/specs/2026-11-14-wisp-0-3-0.md` as amended by its Amendments 1 and 2; roadmap: `docs/plan/roadmap.md`.
- **Waves 0 to 2 and tasks 147 and 148 are recorded (PRs #78-#110):** detail in `docs/plan/archive/2026-11-16-closed-sections.md`, `docs/plan/archive/2026-11-17-closed-sections.md` and HISTORY. Open follow-ups are under the "Follow-ups from ..." sections below.
- **149 measured wave 2 (PR #110):** Windows GUI 15,494,656 B, 94,656 B over the old 15.4 MB soft line and 5,344 B under the old 15,500,000 B gate. Detail: `scripts/measure/results/wave-2-size.json`.
- **Decision 1 (user, 2026-11-17):** cut task 148's sidebar empty-state buttons and keep the palette "New collection…" and "New environment…" commands (task 154, done).
- **Decision 2 (user, 2026-11-17):** the Windows GUI hard gate is 16,000,000 B and the soft line 15,900,000 B, replacing 15,500,000 B and 15,400,000 B (task 156 changes `release.yml`; `budgets.json` keeps 16 MiB); see spec Amendment 2.
- **Follow-up (149):** the CLI roughly tripled since wave 1 (macOS 552,528 → 1,415,600 B); cause not investigated.
- **153 recorded** (PLAN.md pruned from 99,898 B to 94,230 B; decisions written into the spec and roadmap).
- **154 recorded:** the sidebar buttons are cut (two reverts); the palette commands remain.
- **156 recorded:** `wisp-cli` ships in all three archives, the Windows GUI gate is 16,000,000 B, and Windows CLI signing waits on the owner's `SIGNPATH_SIGNS_CLI` variable. Follow-ups (both P3, not fixed): (a) `release.yml:68-81` checks the CLI size before Authenticode signing, so a signed `wisp-cli.exe` is a few KB larger than the size checked; (b) `release.yml:72` accepts a limit stored as a JSON string (`"4194304"`). (c) 114-2 (P3): `release.yml` does not check that the CLI signing state is "signed" or "unsigned", so an empty state file yields the "not code-signed" note. The CLI growth (about 3x since wave 1, about 1.4 MB on macOS) is still not investigated.
- **155 recorded:** sending to an environment with the guard on asks for confirmation, and a `suggest_confirm` environment offers to turn the guard on. Follow-ups (none fixed):
  - [P3] `crates/app/src/ui/root.rs:594-604`: when saving settings fails, the toggle's own notice replaces "Could not save settings", so the guard looks saved but is lost on restart. `ToggleHistoryBodies` at `root.rs:1155-1165` has the same pattern.
  - [Note] The confirm prompt detail shows the host as typed, before variables resolve (it shows `{{base}}`), which is weaker than spec :121.
  - [Note] `guard.rs:72-79` `host_of` cuts at `/ ? #` before stripping userinfo, so a typed password containing those characters is partly shown.
  - [Note] `ui/url_bar.rs:499-517`: the badge may be clipped on a narrow window with a long context strip.
  - [Note, for 158's hand test] keyboard Send on a guarded tab that is already Sending now prompts before re-sending.
  - [Known residual, 115-1, accepted by the user 2026-11-17] the guard fails open while the collection id is Reading or Failed; this matches spec :136. Tasks 157 (docs) and 158 (hand test) must state it.
  - [For 158's hand test] a tab switch while "Send to <env>?" is up, then Confirm, must abort (no test drives the root.rs callback).
  - [For 158's hand test] whether a Reprompt is refused by `has_active_prompt()` inside `done`.
  - [P3] the prompt detail is not re-checked against the draft at send time, so an edit or disk reload during the prompt can send a different URL.
- **157 recorded:** `docs/cli.md` is new, architecture.md drift is fixed and the README gains the CLI and guard text. Follow-ups (none fixed; detail in `docs/plan/tasks/retired/157-docs-cli-guard-release.md`):
  - [P3] docs/cli.md `explain` JSON lacks the nested shapes (params, headers, auth.fields, settings, body, variables, problems; warnings are strings): `crates/cli/src/dto.rs:118-180`, `cmd/explain.rs:156-184`.
  - [P3] docs/cli.md wrongly says a keychain environment is "not opened at all"; the env file loads and only the secret store is skipped (`cmd/explain.rs:68-88`).
  - [P3] docs/cli.md exit code 2: usage text goes to stderr only for parse errors; unknown env prints only "wisp-cli: unknown environment"; non-UTF-8 env name or path also exits 2 (`main.rs:40-41`, `:60`).
  - [P3] architecture.md "How they talk" overstates `scripts/check-boundaries.sh` (`:8-38`): it checks banned deps and patterns, not dependency direction.
  - [Note] The docs/cli.md Usage block is a synopsis, not literal `--help`. The Windows soft line 15,900,000 B is enforced nowhere (`release.yml:177`). The `SIGNPATH_SIGNS_CLI` name looks blanked in PLAN.md:30, HISTORY.md:26 and `scripts/release/sign-windows/README.md:22,:33`.
  - [Low] the `scope` Check in tasks 157 and 158 (`wc -l | grep -x 0`) fails on macOS because BSD wc pads the count.
- **158 recorded:** wave-3 measure (run 44045146818) and release dry run (run 44046922250) are green on d314c5f. Windows GUI 15,533,568 B, under the gate and soft line; the CLI is unchanged. Follow-ups (none fixed; detail in `docs/plan/tasks/retired/158-measure-and-release-dry-run.md`):
  - [Note] the macOS archive's `wisp-cli` is 1,425,616 B against measure's 1,415,600 B (+10,016 B), probably signature bytes; ties to the 156 follow-up (a).
  - [Low] the `only-result` Check (`wc -l | grep -x 0`) fails on macOS because BSD wc pads the count, as for 157's `scope` Check.
  - [P3, 116-1] README.md:81 macOS `ln -s ... /usr/local/bin/wisp-cli` fails on a fresh Apple silicon Mac (directory missing or root-owned): add sudo and `mkdir -p`, or use a user-owned directory.
  - [User] delete draft release `dryrun-44046922250` when done with it.
- **0.3.0 released 2026-11-17:** release PR #118, tag v0.3.0 at ffdccc1, release run 44080607742, winget run 44083496633 (manifest built; the winget-pkgs PR is the user's). macOS hand test passed; Windows and Ubuntu hand tests later (mac-first). Release runbook: `docs/release.md`.

### User setup (optional release channels)
**User setup (optional channels):** none of these block 0.2.0.
- SignPath Foundation application, then the repository secret and variables (`scripts/release/sign-windows/README.md`).
- Confirm the winget PackageIdentifier and open the winget-pkgs PR after 0.2.0 is published (`packaging/winget/README.md`).
- Partner Center account, the `MSIX_*` repository variables and Store submission (`packaging/msix/STORE-SUBMISSION.md`).

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

### Follow-ups from task theme-palette (PR #79 merged; not 0.2.0)
- 79-4: `eprintln!` instead of `tracing::warn`.
- 79-5: `apply` reparses theme.json on every call.
- 79-6: stacked overlays double-dim.
- The banner contrast test rebuilds the tint instead of calling `banner_bg` (`theme.rs:341`).
- `install_keeps_defaults_on_bad_json` asserts nothing when there is no `Theme` global (`theme.rs:363`).
- 79-8: in light mode the open sidebar row differs from the hover/cursor rows by hue only (ΔE 7.3). The hand test found it readable thanks to the cursor's accent edge; an accent edge on the open row would make it clearer.
- The tests check token values, not which token `sidebar.rs` and `headers.rs` use.
- Dark muted text on `list.active` inside popovers is 4.44:1 (only the check icon uses it today).
- Unverified: a copy launched with a collection path argument and no `WISP_AUTOSEND_URL` opened no window during the hand test; check whether main does the same.

### Follow-ups from task 128 (Save and disk conflicts, not 0.2.0)
- 🔒 data loss: Save checks only watcher-processed state. An external edit saved over before the watcher event lands is overwritten; it needs a re-read or stat at write time. Ties to the 0.3.0 spec's open question 1.
- 🔒 data loss: on a clean tab, an external edit that leaves the file unparseable is Ignored (`draft.rs:76-80`), so no banner shows and a later save overwrites it.
- `mark_saved` does not reset `DeletedOnDisk` (`tab.rs:209-213`).
- Test (b) compares only the URL, not the whole request.
- Task 128's check command used `--lib` on a bin crate.
- README says macOS has no portable mode, but `data_dir.rs` and `settings.rs` honour the portable marker on every OS; reconcile Scheduled: 0.3.0 B2 (README fix only; macOS uses `WISP_DATA_DIR`, the marker is for Windows and Linux).

### Follow-ups from tasks 122-127 (Windows distribution, not 0.2.0)
- winget (125): `winget.yml:47` and the README hardcode the `AindriuB/Wisp` path, so the AC4 byte check is skipped silently when the identifier changes.
- winget (125): a prerelease publish runs `winget.yml` and fails.
- winget (125): `GH_TOKEN` is job-level; move it to the Generate step.
- winget (125): a CRLF `SHA256SUMS` fails with an unclear message.
- winget (125): YAML-special characters in Publisher are not escaped.
- SignPath (124): `actions: read` is needed in `release.yml` permissions if the repo goes private.
- SignPath (124): set a ref/branch policy in SignPath, because `workflow_dispatch` on any ref can submit for signing.
- verify.ps1 (124): the expected signer is not checked; only the first WIN_CERTIFICATE is read and NumberOfRvaAndSizes is not checked.
- MSIX (126): `--` in the display name breaks the manifest's XML comment.
- MSIX (126): the STORE-SUBMISSION wording on zip vs Store data separation is wrong: existing AppData files are shared, and new ones go to a private per-package location.
- MSIX (126): `generate.sh:8` has a stale header and an existing line was edited.
- MSIX (126): `pack.ps1` matches "error" anywhere in the output.
- MSIX (126): `Copy-Item` with the `*.png` wildcard does not error on no match.
- `release.yml`: two pre-existing SC2012 actionlint notes.
- Windows CI (123) 🔒: the 8.3 test passes silently when 8.3 names are off; add a visible skip message and a deterministic CI setup that enables 8.3 names (task 99 AC77).
- Windows CI (123) 🔒: `same_file_entry` always returns false on Windows, so the hard-link check is a no-op there (task 99 AC78).
- README and release notes: the History and importer-hardening bullets were written from index rows only; check them against HISTORY.

### Follow-ups from tasks 117-121 (hardening wave 1, not 0.2.0)
🔒 marks a security-relevant item. Task 116's 57-1 to 57-3 are already listed in the task 116 section below.
From PR #59 (task 117):
- 59-3 (Low) engine.rs:1078 URL removal parses reqwest's Display text.
- 59-4 (Low) engine.rs:795 replay clones the whole body for any custom header.
- 59-5 (Nit) engine.rs:958 comment wording.
From PR #60 (task 119):
- 60-4 🔒 a username-only token with raw `/`, `?` or `#` stays in the URL; at least warn.
- 60-5 🔒 curl.rs:600 `--negotiate -u :` claims kept-as-secrets but returns Inherit.
- 60-6 a single-brace `{port}` plus a later `@` is treated as userinfo.
- 60-7 🔒 a backslash lenient scheme is not split.
- 60-8 postman.rs:297 `may_hold_credentials` is computed twice.
- 60-9 curl.rs:596 the auth scheme is matched by string.
- 🔒 `ftp:h/x@y` imports its credentials as Basic.
- 🔒 a password with `/`, then `@`, then `?` is split at the wrong `@`.
- curl `--basic` and `--anyauth` are not recognised, so `--digest --basic -u u:p` imports Digest though curl's last option wins (curl.rs:349-351).
- 🔒 the curl extra-URL warning in the Maybe case still prints a `name:<digits>...@` URL, whose digits may be a password (curl.rs:409-415).
From PR #61 (task 121):
- 61-3 the test skips AC1's resolution clause and the plain-vars-kept claim.
- 61-4 🔒 a cleared secret name can fall through to a same-named env or collection var.
- No test pins the moment of clearing at the failure; the existing test checks state after the reload completes.
From PR #62 (task 118):
- 62-1 (Low) 🔒 history.rs:635: secrets written by earlier builds stay in existing history files until eviction. Fix: a one-time scrub on open, or record it as a residual.
- 62-2 (Low) 🔒 history.rs:535: response headers are checked by name only, so a token in a Location URL is stored. Fix: pass Location, Content-Location and Refresh through redact_url.
- 62-3 (Low) tests/history.rs:755: the Set-Cookie test skips the redacted note and the second header, and the path test skips {{ref}} being kept. Fix: add the three asserts.
- 62-4 (Nit) history.rs:479: the JSON body is parsed twice and the two error branches are identical. Fix: one visitor that builds the Value.
- 62-5 (Nit) redact.rs:17: the fragments overlap (pass covers password, sig covers signature). Fix: drop or comment.
- 62-6 (Nit) workspace.rs:714: the sanitize_redacted doc omits path vars. Fix: extend the doc.
- 62-n1 the fragments are substring matches: importers now move headers like `X-Bypass-Cache` and `X-Assignment-Id` into the placeholder secrets env, and Text/XML history bodies containing words like "design", "signal" or "compass" are dropped whole. Consider word-boundary matching.
- 62-n2 response header names share the unprefixed `redacted` namespace with request headers: a same-named request header with an intentionally empty value is disabled on resend, and WWW-Authenticate and the like appear in every resend notice.
From PR #63 (task 120):
- 63-2 `ensure_ignore_line` confines from `environments/`, not the collection root; confine from the root.
- 63-3 a refused `.gitignore` fails the save after `<env>.toml` was already written.
- 63-4 tests assert `is_err` rather than `Invalid`; `ensure_marker_ignored` and a FIFO are untested.
- 63-5 (Low) environment.rs:99: the listing follows links, so a symlinked <env>.toml is still listed but always fails to load. Fix: list with symlink_metadata. (The behaviour change is already in HISTORY.)
- 63-6 (Nit) environment.rs:68: read_plain_text takes an unused _root. Fix: drop the parameter.
Internal-review suggestions:
- engine.rs:714 the comment on `own_redirects` still describes the old trigger.
- Tests that check `is_err` rather than the `Invalid` variant.
- 118: a `raw.contains("42")` check is weak, and the non-empty `path.` note is not pinned.

### Follow-ups from task 116 (not 0.2.0 wave 3)
From the review of PR #57:
- 57-1 (Low) AC4's per-task split lumps 110-114 into one row; pull 114's own delta from PR #54's base run if needed.
- 57-2 (Nit) the Linux "unexplained" figure compares a net that already includes the opt-level remedy against a gross prediction.
- 57-3 (Nit) the measure-run check dispatches a workflow, so rerunning the check starts a new run; split dispatch from the check that reads the cited run.

### Follow-ups from task 115 (not 0.2.0 wave 3)
From the user review of PR #55:
- 55-6 file-mode is derived from load status, so a Loading or Error file-mode environment still calls keychain delete.
- 55-7 (security, -> task 99, AC72) no data dir reads bindings from the shared /tmp/wisp-no-data-dir.
- 55-8 the NewId failure notice does not name the operation.
- 55-10 Move-to-file forget uses the cached id and is skipped silently when the id is not Known.
- 55-11 a re-probe keeps a stale Ok until the new probe returns.
- 55-12 too-large token banners are global and never cleared.
- 55-13 set_environment_at cannot fail, leaving dead error branches.
- 55-14 refresh_known_id blocks the UI thread on the RESOLVE lock.
- 55-15 (nit) the index prefix is derived by stripping a formatted account.
- 55-16 (nit) the value is cleared before the outcome is known.
- 55-17 (nit) a folder-picker error is treated as cancel.
- 55-18 a Send during post-move Loading clears the move mark, so an older in-flight save can store.
- 55-20 (nit) duplicate asserts.
- 55-21 (security, -> task 99, AC75) the test Workspace::new resolves the real user data dir before it is swapped.

From the internal review:
- A quick double env switch where the second fails can leave the status Loading.
- A forget/save race on the index is possible (no input found that triggers it).
- The mark_persist test does not cover the call sites in send/authorize.
- The NewId and Move-to-file wiring tests call helpers directly rather than the UI path.
- The watcher is skipped for all app unit tests under cfg(test).

From the AC9 hand check:
- Tell users a stale keychain prompt can remain after a timeout (it outlives the app process).

### Follow-ups from task 114 (not 0.2.0 wave 3)
- (a) keyring 4's platform errors are opaque, so `map_error` classifies by Debug text; real Windows errors are still unseen (the Windows CI job runs tests, but the keychain is not exercised there).
- (b) Linux `Locked`/`Prompt` raised by create_item, and PromptDisconnected/Unavailable, arrive as PlatformFailure and map to Other (fails closed).
- (c) The Windows TooLarge row (0x800706F7) is defensive only: the store returns TooLong above 2560 B before any OS call, and a real OS failure would be 1783.
- (d) `extract_code` tries the first "0x" anywhere before "code: ", so a macOS message containing "0x" could override the OSStatus.
- (e) keyring's store init is process-wide: with no bus at first call the store stays NoService until restart.
- (f) The test seam (Backend, open_with, map_error) is `#[doc(hidden)]` but public, exposing keyring::Error in wisp-storage's API.
- (g) After a backend panic the next one or two calls return Other before the worker respawns (dev/test only; release is panic=abort).
- (h) macOS signed-release keychain behaviour is untested; 115's hand check covers the dev build.

PR #54 round 1 (54-1 fixed in the PR: TooLong maps to TooLarge only for the secret, plus a 513-byte account guard):
- 54-2 macOS -61, -25244 and -25292 (access refusals) map to Unavailable{NoService} instead of Denied -> add them to classify's Denied arm.
- 54-3 a TimedOut set/delete may still land later, so callers (111 rollback, 115) must not assume nothing was written -> document TimedOut as outcome-unknown on set/delete and in the trait contract.
- 54-4 (-> task 99, AC68) Windows credentials default to Enterprise (roaming) persistence -> use Local, or record roaming as accepted.
- 54-5 (-> task 99, AC69) items are keyed by a generic service/account, so other apps can plant or duplicate them (Linux Ambiguous), and '.' is unescaped in Windows target names -> distinctive reverse-DNS service in 115, reject empty/dotted service in open(), document Ambiguous.
- 54-6 (tests) the timeout test uses equal probe/call bounds and only blocks the probe.
- 54-7 (tests) the mapping test misses Windows 1312 and Linux NoResult rows; the NO_SERVICE keywords are mostly untested.
- 54-8 (tests) the probe test cannot see the account read.
- 54-9 the PR #54 body misses block-padding, cbc, hkdf, secret-service and the 109 figures/stat note.
- 54-10 (nit) tests poll with thread::sleep -> use a channel signal.
- 54-11 (nit) the in-crate store-Debug test never inserts ACCOUNT/SECRET sentinels before formatting.
- 54-12 (nit, -> task 99, AC70) secret copies on the worker are not zeroized.

PR #54 round 2 (54-13 fixed in the PR: only bound main checkouts, meaning a real `.git` directory, anchor worktree verification; the untrusted-read branch of `common_dir` is removed; orchestrator decision):
- 54-14 (-> task 99, AC71) post-match I/O uses the Disk path form, so Win32 name rewriting returns: `worktrees/COM1` opens a DOS device, and trailing-dot or trailing-space names reach another folder -> do I/O on the verbatim form, or refuse reserved names and trailing dots/spaces; cfg(windows) tests.
- 54-15 `tests/vault.rs` ~2222 `file_mode_env` is dead code on Windows after the cfg(unix) gating, so clippy -D warnings fails there -> cfg(unix).
- 54-16 `vault_binding.rs` ~405 nested if-let in Windows-only code may trip collapsible_if -> let-chains; run clippy for the msvc target.
- 54-17 (nit) the lexical check runs before simplify_verbatim, so a verbatim gitdir text keeps `..` inside one component -> refuse verbatim prefixes in gitdir text.
- 54-18 (nit, tests) the lexically plausible UNC test never matches the trusted dir, so it passes without the guard.
- 54-N1 the 54-13 regression test passed before the fix (old code swallowed the read error); no test proves a bound worktree's gitdir/commondir is never opened.
- 54-D1 document on `vault::bind_additional_checkout`: a new folder is one-click verifiable only as a worktree of a bound main checkout (bound worktrees do not anchor).

### Follow-ups from task 113 (not 0.2.0 wave 3)
- collections.rs:66-72 re-derives names and walks collection_root(i) instead of using OpenCollection::name() and Workspace::collections; drift risk.
- collections.rs list_open stays true when the count drops to 1, so the list reappears expanded when a second collection opens.
- The duplicate-name test only checks that paths pass through.
- 53-1 collections.rs:147 row click activates by the index captured at render; if a collection closed in between (e.g. a clone refusal) another one is activated → capture the row's root and resolve with coll_ix on click, as close does
- 53-2 collections.rs:99 the open list has no dismiss on click-away or Escape; it stays open over the tree
- 53-3 (nit) collections.rs:126 active mark is a text ✓ with padded spaces; alignment depends on the font → icon or fixed-width slot

### Follow-ups from task 112 (not 0.2.0 wave 3)
- Settings write on activate is now synchronous file I/O on the UI thread; move persist_collections off-thread and coalesced, or skip when unchanged.
- Startup restore scans up to 8 trees synchronously.
- AC8 clone refusal closes the refused collection's tabs without a dirty check, so a tab edited before the id lands is discarded silently.
- The history test covers history_target but not that append_history passes sent.root.
- Clone refusal makes the last collection active rather than the previously active one.
- Collection paths are stored with to_string_lossy; a non-UTF-8 path on Linux is dropped on restart.
- set_cookie_persistence reads the active collection's id inside its callback, so switching while an id is being created could apply to the wrong collection.
- measure.ps1 changes are unrun and unlinted until the first Windows measure run.
- The refused-argv-path active fallback has no test.
- 50-4 `quit-check.sh:53` still runs Wisp on the real data dir (now restores and rewrites the collection list); use a per-launch temp WISP_DATA_DIR as in measure.sh.
- 50-5 Palette Switch/Close resolve-by-root is untested; add a pure helper test with shifted indices.
- 50-6 `settings.rs:149` corrupt `open_collections`/`active_collection` is untested, and one bad element drops the whole list; keep valid strings and test each key.
- 50-7 `settings.rs:574` restore dedup branch is untested; duplicate missing paths are reported twice.
- 50-8 (task 99, AC52) `workspace.rs:527` history re-reads the collection id from disk while cookies and tokens use the cached id; an on-disk id change routes history into another collection's store. Build HistoryJob from the known id.
- 50-9 Scratch tabs become unreachable once a collection opens but still count as dirty at quit.
- 50-10 Restore silently forgets a remembered collection that exists but fails to open.
- 50-11 A settings-save failure during restore is emitted before the window subscribes.
- 50-12 Jar changes after the close-time flush are never persisted.
- 50-13 Watcher batches from background collections rescan trees on the UI thread.
- 50-14 `release_tab` leaves id waits; a closed tab can still send or authorize under the wrong partition (relates to 47-4).
- 50-15 A clone whose id lands first is briefly live, and its refusal can save the shared jar.
- 50-16 (nit) `measure.ps1:117` fail path kills the app without WaitForExit, so temp dir removal can fail.
- 50-17 (nit) Palette collections mode does not preselect the active collection.
- 50-18 (nit) SwitchCollection has no root handler.
- 50-19 (nit) Repeated tab-to-collection lookups in send/authorize; use one helper.
- 50-R1 Residual, accepted by the user: close-collection and clone-clash decisions are tested via plan_clash/plan_close/confirmed_open, but the dispatch in close_collection and the clash handler is not (mutations there pass every test); no GPUI test harness exists.
- 50-R2 Add a GPUI test harness (gpui TestAppContext) so Workspace flows get end-to-end tests.
- 50-P3a `sidebar.rs:529-535` after a confirmed delete in a non-active collection, collapsed/selected are trimmed by relative path in the shown collection (cosmetic).
- 50-P3b `import.rs:610` new test sits before the module's use lines (idiom nit).

### Follow-ups from task 86 (not for 0.1.1)
- The streamed resend checks only each file's length, not its contents, so a file edited to the same size between sends is resent with the new bytes.

### Follow-ups from task 107 (not 0.2.0 wave 2)
- [ACCEPTED residual, user 2026-11-10] AC10: copies/clones of a collection (same id) share history, OAuth2 tokens, the in-memory jar and cookies/<id>.json.
- [Residual -> task 99] AC11: CookieFile::load symlink check-then-open race and a symlinked cookies/ parent (data dir is per-user 0700).
- [P3] Quit only saves the open collection's snapshot; jobs still queued (Disable/Enable/Save from a just-switched collection) are dropped at quit; window is one file write.
- [P3] Load-merge reads snapshot then load()s (two jar locks); a Set-Cookie between them is lost; needs an atomic merge in core CookieJar.
- [P3] A single failed save pauses saving for that collection until toggled off/on; consider auto-retry on the next change.
- [P3] Cookies set by a request still in flight for the old collection after a switch go to the old jar and are not saved this run unless reopened.
- [P3] Tests: `cancel_drops_a_send_waiting_for_the_id` does not exercise cancel(); `quit_save_only_when_saving_and_dirty` guards nothing in take_quit_save.
- [P3] WISP_AUTOSEND_URL with a collection path at launch: send returns Ok(None) while the id is read, so no WISP_EVENT line (measure.sh does not use a collection).
- [Low] 47-3: cookie panel rebuilds rows only on jar generation change, so expiry labels go stale and expired cookies stay listed (crates/app/src/ui/cookie_jar.rs ~215); rebuild on time too.
- [Low] 47-4: close_tab doesn't drop the tab's id_waits entry, so a pending Send can still go out (workspace.rs ~1235).
- [Low -> task 99] 47-5: serde parse errors from a corrupt cookie file can echo file content into the log and notice (workspace.rs ~2225); map to a fixed message with line/column.
- [Low] 47-6: loading bumps jar generation, so each persisted collection rewrites its cookie file 2 s after opening (workspace.rs ~2311); reset debounce to post-load generation.
- [Low] 47-7: Enable failure leaves no persistent banner in the panel (AC5) (workspace.rs ~2234).
- [P3] 47-P3: a cookie save/delete job already running (not queued) at quit isn't ordered against the quit future; relies on gpui's 200 ms shutdown wait.
- [NOTE] A running Save and the quit save both take WRITE_LOCK; an older snapshot could in theory overwrite a newer one.

### Follow-ups from task 105 (not 0.2.0 wave 2)
- [P3] tests/timings.rs AC6 cancel test can't detect a leftover registry entry (second request uses a fresh port/token); add a cfg(test) registry length check.
- [P3] h2 test asserts `accepted >= 1` (concurrent sends open extra connections before the first is established); a warm-up send would allow `== 1`.
- [P3] 46-1 Cargo.toml:38 new deps tower-layer, tower-service, http, h2, bytes omit `default-features = false` (conventions: Dependencies).
- [P3] 46-2 tests/timings.rs:81 the concurrent test blocks on a Barrier with no timeout (hangs instead of failing); wrap in a 10 s timeout naming the accepted count.
- [P3] 46-3 tests/timings.rs:110 keep-alive tests assume the pooled connection is idle before the next connect wins (small race).
- [Nit] 46-5 timing.rs:24 the Timings doc doesn't say what phases mean through a proxy.
- [NOTE] Registry MAX_ENTRIES=64 FIFO: >64 unclaimed concurrent connects could evict a real entry (reported None); unreachable today (no runner/bulk send). Intermediate redirect hops and lost-race connects are never claimed and rely on the cap.
- [NOTE] With reqwest-followed redirects the timings describe the final connection; with the engine's own loop, the first hop that opened a connection.

### Follow-ups from task 111 (not 0.2.0 wave 3)
- (a) A malformed `secrets` value makes `save_environment` write only plain vars and return Ok, silently dropping edited secret values; return an error.
- (b) No test fails the first pre-set `get` (`fail_nth(0)`) or uses a malformed `<env>.secrets.toml` in `migrate_to_store`.
- (c) Document `MemoryStore::accounts()` as test-only so 115 does not rely on enumeration.
- (e) Clones fail closed until bound/rebound; a copy is offered a new id.
- 52-9 TooLarge during migration names no var; contract doesn't forbid truncation; read-back mismatch reported as Other -> wrap errors with the var name, require TooLarge, never truncate.
- 52-10 restore after a failed env.toml write ignores its own errors and can leave plaintext while mode is store -> log and return the leftover path; test the no-old-file branch.
- 52-11 secret names differing only by case collide on case-insensitive stores (Windows) -> reject case-insensitive duplicates.
- 52-12 tests missing: set_secret write failure; chmod test lacks a drop guard; import_secrets_file leftover-warning case (shares the tested helper).
- 52-13 (nit) environment.rs io widened to pub(crate) but vault.rs builds StorageError::Io by hand.
- 52-P3a `rebind` on an id with no row creates one; decide whether rebind requires an existing row or is named as a second explicit binder (115 AC16 says only bind binds).
- 52-P3b load probes the store before the binding check; unbound + store absent falls back to the file, unbound + locked reports Locked rather than Unbound.
- 52-P3c a brand-new keychain environment with no names also shows the bind prompt (harmless extra click; smooth over in 115).
- 52-P3d env_lock leaks one Mutex per environment path (bounded).
- 52-18 (-> task 99) import_secrets_file overwrites existing store values unasked with a stale leftover or committed file -> return differing names as a typed conflict and require confirmation; refuse tracked files.
- 52-19 (-> task 99) the binding check trusts the caller's id rather than reading it from root's collection.toml; a 115 wiring slip would pass -> read or derive the id from root in check.
- 52-20 migrate_to_file's deletes share the read deadline: slow reads orphan every entry, late successful deletes reported TimedOut -> fresh budget for cleanup, check the deadline only before each delete.
- 52-21 (-> task 99) to_string_lossy registry key lets distinct non-UTF-8 roots share a binding -> fail closed on non-UTF-8 roots.
- 52-22 cross-process env_lock, symlinked-root canonicalisation and check have no tests; vault_binding has no unit tests; no test for a non-regular `.wisp-binding` (e.g. a directory).
- 52-24 (nit) tests' cx() leaks one tempdir per test -> return the TempDir with the ctx.
- 52-P3e (-> task 99) TOCTOU: marker and secrets-file checks run before the open, and the reads/writes follow symlinks (fs.rs resolve) -> open with O_NOFOLLOW or verify the opened handle.
- 52-P3g (for 115's prompt text) FailReason::Unbound can't tell "never bound" from "previously bound here, marker missing", so the bind prompt can't warn that confirming gives this path's existing keychain secrets to the folder now there -> consider a distinct reason.

PR #52 round 5 (post-merge; 52-52 and 52-53 fixed on the task 114 branch: UNC/device gitdir refused before I/O, exact lexical match against `<canonical trusted common>/worktrees/<name>` before any I/O, Windows verbatim paths compared in Disk form; cfg(unix) on the symlink tests):
- 52-54 (-> task 99, AC65) worktree verification trusts the bound root's own .git as anchor; if that root arrived by zip/sync its .git is attacker content -> require the common dir outside every collection root, or document the trust assumption.
- 52-55 (-> task 99, AC66) environment.rs:280 evaluates sp.exists() twice; a file appearing between them is written unchecked.
- 52-56 (-> task 99, folded into AC64) the back-pointer is canonicalised through symlinks; a retargeted component of a real entry's path verifies another folder.
- 52-57 (-> task 99, AC67) verify and bind resolve new_root separately under separate locks, and so does share_with_folder -> canonicalise once, check and bind under one lock.
- 52-58..52-60 (nits, tests) symlinked_dot_git_is_refused asserts only is_err; the dangling-link test doesn't assert the target stayed absent; missing tests for an in-folder symlink gitdir, worktree-to-worktree, relative paths and a mismatched id.
- 52-61 (nit) non-top-level collections, separate-git-dir and submodules never verify as additional checkouts -> document.
- 52-62 (nit) HISTORY reads as if vars-only saves through a symlinked environments/ now work; they still fail, with a clearer message.
- 52-W Windows code paths in vault_binding (simplify_verbatim, cfg(windows) tests, make_worktree git_style) are uncompiled and unrun; the Windows CI job (tasks 122/123) now compiles and runs them.
- 52-30 (-> task 99, fold into AC53) migrate_to_file still merges store secrets into a git-tracked regular secrets file -> refuse or warn on tracked files.
- 52-31 (-> task 99) copy check uses exists() (metadata errors read as gone, fails open); a read error blocks a real move; relies on the committed id rather than the old root's marker -> try_exists, fail closed except NotFound, use the marker nonce.
- 52-32 lowercasing silently re-keys history, cookies and tokens of collections whose id already had uppercase (unreleased builds only) -> write the lowercase id back and move old paths, or note it.
- 52-33 a registry from earlier dev builds (v1/v2) is rejected as corrupt and bind can't repair it -> treat old rows as Unbound or let bind replace after confirmation.
- 52-34 (tests) the symlinked environments-dir test fails on the binding check, not check_secrets_path -> assert the specific error.
- 52-35 unlocked save_environment is still pub (import uses it) -> pub(crate) or rename; 115 must use save_environment_locked.
- 52-36 environments/.gitignore is written under the registry lock and under env_lock; interleaving can drop the secrets ignore line -> one per-dir lock or write both lines.
- 52-37 (nit) broken doc link to rebind in vault_binding.rs; 52-38 (nit) save_environment_locked doc says parsed once but file mode re-reads; 52-40 (nit) first bind adds an untracked environments/.gitignore to store-only collections -> mention in the 115 prompt.
- 52-P3h (-> task 99) once any bound root is gone, rebind's LooksLikeCopy refusal stops firing for live roots, so a copy can be bound through the "moved" prompt; decide between the current rule and refusing when any live root carries the id (which blocks moving one checkout while another is live).
- 52-N1 marker cross-registry read-modify-write race: fails closed (extra bind prompt only).
- 52-N2 root list per id has no size cap (grows only via user-confirmed additional checkouts) -> cheap cap later.
- 52-N3 a collection whose collection.toml is a symlink (dotfiles setups) now errors and history turns off; intended fail-closed, user-visible.
- 52-44 (-> task 99) one stable registry id is written into every marker; a tracked marker shows modified and leaks a linkable per-user id -> per-root key (e.g. hash of registry id and root); warn on a tracked marker.
- 52-45 (-> task 99) the attacker-controllable marker is read whole with no size or entry cap -> cap size from lstat, fail closed; drop malformed entries on write.
- 52-46 LooksLikeCopy / IdBoundElsewhere / rebind docs still describe one root and point worktrees at assign_new_collection_id -> point at bind_additional_checkout / share_with_folder; fix the 52-37 link.
- 52-47 ensure_collection_id now needs canonicalize; where it fails on Windows volumes, history turns off -> note for AC38 or fall back when the component walk passed.
- 52-48 (nit, -> task 99 AC56) collection.toml check-then-write race -> extend AC56.
- 52-49 (nit) non_regular_collection_toml_is_refused passes without the helper (EISDIR) -> assert StorageError::Invalid.
- 52-50 (nit) marker lines for vanished registries are never removed -> fold into the 52-N2 cap.
- 52-51 (nit) confined_regular_file returns Ok(true) when the file equals the root -> refuse when there are no components.
- 52-N4 (-> task 99) a stale linked-worktree entry (folder deleted by hand, not `git worktree remove`) verifies any folder later created at that path -> also require the entry not to be prunable, or check the folder's owner.
- 52-W1 (task 115 wiring rule) share_with_folder can't prove its caller: 115 must pass the currently open, bound collection's root as from_root and take new_root from a folder picker; never fill from_root from bound_roots inside an unbound folder's prompt.

### Follow-ups from task 104 (not 0.2.0 wave 2)
- [P3] cookies.rs: a Set-Cookie with UTF-8 values is silently dropped (`to_str`), and one non-ASCII value drops the whole Cookie header; use `from_utf8` / `HeaderValue::from_bytes` like reqwest's Jar.
- [P3] The generation bumps on unchanged load/clear and on a rejected Set-Cookie; task 107 may save more often than needed.
- [P3] `load` truncates remaining lifetime to whole seconds; a record expiring less than 1 s after now is stored expired.
- [P3] Each jar clones the parsed public-suffix list (a hash-map clone per jar/load).
- [NOTE] cookie_store logs non-Secure cookie values via `log::debug!`; no log backend exists today, so it would leak if a log/tracing-log bridge is ever added at debug level.
- [NOTE] Loopback hosts count as a secure context (Secure cookies are sent over http://localhost), matching browsers.
- [P2 -> task 108 / release] The public-suffix list snapshot (crates/core/data/public_suffix_list.dat, 2026-11-09) ages; refresh it before each release.
- [P3 -> task 99] 45-9 cookies.rs `load()` accepts ';', CR/LF and userinfo hosts in records; reject invalid cookie-octets and host mismatch.
- [P3 -> task 99] 45-10 plain-http responses can set or overwrite Secure cookies; `__Host-`/`__Secure-` prefixes are not enforced.
- [P3 -> task 99] 45-11 unlisted single-label suffixes (test, local) are accepted as Domain from responses (load rejects them); apply `is_registrable` to responses too.
- [P3] 45-6 `publicsuffix` is declared outside workspace deps; IDN suffix rejection relies on punycode enabled only via reqwest; add an IDN suffix test.
- [P3] 45-8 the PSL parse `expect()` has no invariant comment; a bad refresh aborts on first send; add a dedicated parse test.
- [P3] 45-12 Cookie header order is arbitrary, not longest path first.
- [P3] 45-13 generate.sh notice URL serves the latest list, not this snapshot; version is hard-coded twice; read VERSION/COMMIT from the file and add a permalink.
- [P3] 45-16 every Set-Cookie response scans the whole jar under the mutex (`needs_eviction`); keep running counts; the "no allocation" doc comment is wrong; the 3100-domain test takes about 5-7 s in debug.
- [P3] 45-14 (Nit) `PARSE_COUNT` ships in release (`cfg(test)` it); sync tests are marked async. Item 42-8 (jar `debug_redacts` test) was done in task 104 and is retired.

### Follow-ups from task 106 (not 0.2.0 wave 2)
- [P3] auth.rs:156 `parse_port` silently turns an invalid redirect port (81234, 8l23) into 0 ("any free port"); reject or show a notice.
- [P3] workspace.rs:553 Authorize on a history tab with a redacted body shows "Cannot authorize: Syntax(Body)" without Send's explanation; reuse `refused_body_message`.
- [P3] workspace.rs:490 vs :1074 the collection partition is set in two places (`authorize_command` and `send`); use one helper.
- [P3] The Authorize button and status strip sit at the bottom of the scrolling auth form, below the response split on a normal window, so "Authorized"/"Failed" can be off-screen; keep the strip visible.
- [P3 -> task 99] 44-1 auth.rs:329 a literal client_secret can be saved into committed request TOML with no hint; offer placeholder `{{client_secret}}` or a muted hint when no `{{` is present.
- [P3 -> task 99] 44-4 tab.rs:82 AuthorizeState derives Debug over the URL holding OAuth state; hand-write a redacting Debug, add a `debug_redacts` test, fix the doc comment.
- [P3] 44-2 auth.rs:603 changing auth type while Waiting hides Cancel; the listener stays bound until timeout.
- [P3] 44-3 auth.rs:772 tests: the secret-slot test doesn't check masking; round-trip tests duplicate each other and skip the client_auth label mapping.
- [P3] 44-5 workspace.rs:568 "Cancelled" is detected by matching the engine message string; use a shared const or ErrorKind::Cancelled.
- [P3] 44-6 workspace.rs:873 "Authorized" survives an environment switch though tokens are keyed by environment; also survives edits to client id, scope or URLs (earlier NOTE). Clear or recheck on change.
- [P3] 44-7 workspace.rs:1189 Cancel leaves the tab Waiting, so a queued AuthUrl can still open the browser.
- [P3] 44-8 workspace.rs:2357 AC8 test builds the expected Send without tree or env, so collection client settings aren't compared.
- [P3] 44-9 (Nit) auth.rs:332 the shown authorization URL is editable but never read back; make it read-only.
- [P3] 44-10 (Nit) workspace.rs:1176 the re-authorize cancel branch is unreachable from the UI and untested.
- [NOTE] Authorize resolves the whole request (same path as Send), so an unresolved variable in the URL or body also blocks it; the button appears only for the request's own auth-code auth, not inherited auth.

### Follow-ups from task 110 (not 0.2.0 wave 3)
- [P3] The fail-closed test's RT_SENTINEL check only bites on the panic store; the `fail_load` path cannot fail that assertion.
- [ACCEPTED residual, user] The overlap test uses a 500 ms window rather than a state-driven wait.
- [ACCEPTED residual, user, 49-R1] A refresh that times out waiting for the per-key store lock (a previous save hung >30 s) drops the IdP-rotated refresh token, so memory keeps the revoked one and the user must re-authorize. Follow-up: cache a rotated refresh token even when the store lock times out (relaxes the rule that the generation check runs under the store lock).
- [P3, 49-2] "Fails closed on panic" holds only in test builds; release uses panic = abort. Fix the wording or require stores to catch_unwind (document on the trait).
- [P3, 49-3] The save-then-forget order test is sequential and passes without the lock (check whether round 1/2 tests supersede it); race a blocked save against a re-authorize, or rename the test.
- [P3, 49-4] JoinError mapping for save and forget is untested in integration tests.
- [P3, 49-5] manager.rs:366 Authorize now fails when the store refuses the save; with a keychain that always denies, Authorize never succeeds and the message offers no way out. Point the error at turning persistence off (task 115's keychain/persist setting), or cache in memory and mark stale; record the behaviour change.
- [P3, 49-6] (tests) manager.rs:841 no test for a forget that times out, or for recovery after a stuck call returns. Add a stuck-forget mode plus a release-then-authorize test.
- [P3, 49-7] (nit) manager.rs:62 `bounded`'s select is unbiased, so a result ready together with the timeout can be reported as a timeout. Use `biased;` with the future polled first.
- [P3, 49-8] (nit) manager.rs:333 the comment says a stuck refresh save cannot lose the rotated token; the lock-wait timeout still can (49-R1). Qualify the comment.

### Follow-ups from task 103 (not 0.2.0 wave 2)
- [ACCEPTED residual, user 2026-11-10] After a re-authorize, a send that read the old refresh token just before it can still POST that RT to the IdP once; the result is discarded (generation check) and nothing old is used or persisted. Closing it means holding the store lock across the token POST.
- [P3] The stale marker is in-memory only: a failed forget/save leaves the old RT in the store after restart (wave 3).
- [P3] engine.rs:972-975 a 401 for a send that started with AT_X, arriving after a re-authorize, invalidates the new AT_Y; invalidate only when the cached token equals the one sent.
- [P3] No single-flight on refresh; concurrent refreshes may get a rotating RT family revoked.
- [P3] Token fetch uses the full request timeout before `began`, so a send can take about 2x timeout.
- [P3] tests/oauth2.rs:1348 orders the paused-save test with a 700 ms sleep, not a state wait.
- [NOTE] TokenKey excludes client_secret: after editing the secret, a cached client-credentials token is reused until expiry.
- [NOTE] Public `Clock` trait and `Engine::start_with_clock` were added for expiry tests.
- [P3 -> task 99] 43-4 manager.rs:291 token endpoint inherits the request's verify_tls=false, so secrets go to the IdP unverified; always verify TLS for token/auth endpoints, or show it.
- [P3 -> task 99] 43-5 loopback.rs:51 listener spawns unbounded connection tasks (now also kept listening after ignored callbacks); cap in-flight connections (~16).
- [P3 -> task 99] 43-6 token.rs:258 error-body redaction covers only three token keys; add client_secret, code, code_verifier, camelCase; scrub sent secrets.
- [P3] 43-7 token.rs:225 negative expires_in means never expires; 0-30 s tokens never reused; treat negative as zero; margin min(30 s, lifetime/2).
- [P3] 43-8 manager.rs:305 memory path reads epoch and refresh token under separate locks; can burn a fresh RT.
- [P3] 43-9 manager.rs:333 a rejected refresh token (invalid_grant) is kept and re-posted every send; forget it under the generation check.
- [P3] 43-10 engine.rs:186 Cancel and AuthorizeCancel share one map and can cancel each other's operations.
- [P3] 43-12 tests: Basic client auth never tested with reserved characters (a:b c&d+e).
- [P3] 43-13 tests/oauth2.rs:740 oversized-head test uses a 1 s wall-clock timeout; widen to ~3 s.
- [P3] 43-14 tests/oauth2.rs:845 port-closed assert can race parallel tests reusing the port.
- [P3] 43-15 (Nit) pkce.rs:10 random bytes from UUIDv4; use getrandom directly; rename the state test.
- [P3] 43-16 (Nit) token.rs:155 clone_without_url doesn't clone; rename.
- [P3] 43-17 (Nit) storage tests/history.rs:656 test name says var refs but uses literals.
- [P3] 43-18 (Nit) manager.rs:134 per-key generation and store_locks maps never pruned.
- [P3] loopback.rs:71 a callback with a non-matching state (e.g. a stale browser tab) gets the "You can close this tab" success page while Wisp keeps waiting and later times out; show a neutral "this sign-in link has expired, try Authorize again" page instead.
- [P3] 43-19 manager.rs:470: an IdP error callback sent without `state` is now ignored, so the user waits the full 120 s authorize timeout with no message; show the ignored-callback count (or "a sign-in error was received without a matching state") in the timeout error.

### Follow-ups from task 102 (not 0.2.0 wave 2)
- [P3] cookies.rs:202-203 `save` doc doesn't mention the domain/path caps or soonest-expiring trimming.
- [P3] cookies.rs:85-88 MAX_DOMAIN_BYTES/MAX_PATH_BYTES are declared after `acceptable`; group them with the limits at :20-24.
- [P3] cookies.rs:171 Parse error uses serde_json's message, which can echo a mistyped scalar from a hand-edited file (array-form entry); use a fixed message.
- [P3] cookies.rs:224 with more than 3,000 input cookies, the first 3,000 in input order are kept before expiry trimming; keep the latest-expiring.
- [NOTE for task 107] `save` checks `is_enabled` then writes with no lock, so a concurrent `disable` can re-enable persistence; `load` checks the symlink then opens (race) and does not refuse a symlinked `cookies/` parent.
- [P3 → task 99] 41-3 cookies.rs:207: symlink check then write through a path that follows symlinks; use a non-following write path.
- [P3 → task 107] 41-4 cookies.rs:183: load accepts any domain scope and control characters from the file; validate here or in the jar.
- [ACCEPTED, user 2026-11-10] 41-5 / 42-12: copies or clones of a collection (same id) share history, OAuth2 tokens, the in-memory jar and the cookie file.
- [P3] 41-6 cookies.rs:266: no fsync before rename; a crash can leave a truncated file; sync_all before rename and define recovery on Parse.
- [P3] 41-7 tests: no test that enable keeps cookies or refuses a symlink; error test only checks is_err.
- [P3] The in-process WRITE_LOCK mutex is not covered by a deterministic test (the file lock is).

### Follow-ups from task 100 (not 0.2.0 wave 2)
- [P3] No app test drives `Event::Completed` with non-empty timings through on_event/append_history, or the ignored auth events; task 105 has made timings real.
- [P3] No test that commit/save leave an OAuth2 draft untouched in the auth editor; add before task 106 reworks it.
- [NOTE] Literal OAuth2 `client_secret` is written to committed request TOML, like Digest password and SigV4 secret_key; values are normally `{{vars}}` (spec :58).
- [NOTE] Until task 106, the auth type list doesn't offer OAuth2, so switching an OAuth2 request away can't be undone in the UI.
- [NOTE] Task 100 added no deps; 103/104 bring serde_json/getrandom/tokio net and cookie crates and must recheck the lockfile rule and size.
- [P3 → task 103] 42-3 core auth/oauth2.rs:26: RefreshTokenStore is sync with no Result; keychain failures would be lost and prompts would block the engine. Return Result; document spawn_blocking use.
- [P3 → task 103] 42-7 storage history.rs:266: history empties non-secret OAuth2 fields (token/auth URLs, client id, scope), so Resend loses them; redact only client_secret.
- [P3] 42-4 app ui/request/auth.rs:123: build_auth maps OAuth2 kinds to Inherit, guarded only by an early return elsewhere.
- [P3] 42-5 storage codec.rs:439: redirect_port read error and stale-key removal are untested.
- [P3] 42-6 core engine.rs:459: serde round-trip test omits AuthorizeStart and AuthorizeCancel.
- [P3] 42-9 (Nit) model.rs:167: AuthorizeStart timeout None has no stated meaning; document as the 120 s default.
- [P3] 42-10 (Nit) app workspace.rs:1135: inner auth-event arm is unreachable.
- [NOTE] Round-1 fix for PR #42: TokenKey now carries collection id and environment; each collection has its own client and CookieJar (ClientSettings.collection); EngineConfig.cookies was removed. Task 106 must pass `environment` and `client.collection` on AuthorizeStart.

### Follow-ups from task 97 (not 0.2.0 wave 1)
- [P3] workspace.rs:1006-1018: Save on a from-history tab does not clear `from_history`, so later sends of the saved request still show the "Resent..." notice.
- [P3] core body.rs:206: a borrowed history body deleted by Clear or eviction while its tab is open reads as "unknown body handle: BodyId(n)"; show "history body was removed".
- [NOTE] Resend tests exercise the scope helper, not `Workspace::send` (no gpui test harness).
- [P3] 38-5 workspace.rs:647: storing a resend loses the original redacted list, so a later resend gives no warning.
- [P3] 38-6 history.rs:163: after a collection switch the panel shows old rows until reload finishes; set Loading at reload start.
- [P3] 38-7 root.rs:181: toggling the panel closed drops it without restoring focus.
- [P3] 38-8 workspace.rs:812: with history_bodies off the body is still read on the UI thread.
- [P3] 38-9 ui/history.rs:228: each render deep-clones up to 500 entries for click closures.
- [P3] 38-10 settings.rs:381: round-trip test doesn't assert the written file text.
- [P3] 38-11 ui/history.rs:434: storage test lives in the app UI module with a pid temp dir.
- [P3] any redacted `auth.*` field resets auth to Inherit, including non-secret fields (ApiKey header name, SigV4 region/service) because storage over-redacts them; fix the redaction in storage.
- [P3] tab.rs:87-89: the Resend notice keeps listing disabled headers after the user re-enables and fills them.
- [P3] core body.rs:210-214: on Windows a pending-delete history body opens with PermissionDenied, not NotFound, so it shows a generic Io error instead of Removed.
- [P3] storage history.rs:741-760: no test covers clear's best-effort deletes and later orphan sweep.
- [P3] workspace.rs empty_json_leaves: keys compared in raw escaped form vs decoded names stored by the redactor (escaped keys never match); the `{{` branch skips to the first `}}` regardless of content.
- [P3] workspace.rs test `long_key_with_many_empty_leaves_is_fast` asserts < 1 s wall-clock; may flake on slow CI.
- [IDEA] Export storage's `mask_bare_refs` and use serde_json in the app for the redacted-body check instead of the hand-rolled scanner.
- [P3] r2 38-1 storage history.rs:752: clear ignores every delete error and reports success; bodies may stay on disk; lock-file comment wrong. Tolerate only in-use/pending-delete errors, report leftovers, fix comment. (→ task 99 when split)
- [P3] r2 38-2 tab.rs:95: `stored_body` is unused by the refusal and its doc describes a rule no longer implemented; remove or use it and fix the doc.
- [P3] r2 38-3 url_bar.rs:279: URL bar previews the collection scope while Send uses the original folder scope, so it shows false problems; share scope_path_for with refresh.
- [P3] r2 38-4 workspace.rs:162: the scope guard compares only the URL template; a request var override can send folder credentials to a new host; also treat changed vars as edited, or compare the resolved host. (→ task 99 when split)
- [P3] r2 38-5 workspace.rs:452: a non-JSON Text/Xml body is refused for good; Body::None handled inconsistently; refuse a failed scan only for Body::Json.
- [P3] r2 38-6 workspace.rs:1414: body refusal is a Syntax problem detected by message prefix; use a dedicated SendError variant.
- [P3] r2 38-7 (Nit) workspace.rs:238: with both userinfo parts redacted, url.password is reported as unhandled.

### Follow-ups from task 96 (not 0.2.0 wave 1)
- [P3] 37-5 import.rs:435: warnings are shown verbatim; a malformed curl header can echo its token (fix at source, task 99 AC19).
- [P3] 37-6 import.rs:448: the secrets note mentions Postman collection variables for curl imports; pick the text by format.
- [P3] 37-7 import.rs:156: the folder count omits the created target; a failed write leaves it behind and the error does not say the import is partial.
- [P3] 37-8 import.rs:431: each render re-walks the result and rebuilds warnings; Import deep-clones the result on the UI thread.
- [P3] 37-9 root.rs:464: the overlay does not block clicks behind it; switching collection behind it retargets the import.
- [P3] 37-11 import.rs:407: collection switched or closed mid-write hides the Done report or misattributes it; keep the write root and render Done from it, or block switching while writing.
- [P3] 37-12 import.rs:208: missing-folder branch can import into a renamed folder (api-2) with no mention; refuse or report when create_folder returns a different name.
- [P3] 37-13 (Nit) import.rs:206: reserved target gives two different error messages; inputs stay editable while writing.
- [P3] import.rs:141-148: read_capped checks metadata then opens by path (TOCTOU to a FIFO); open first, then call file.metadata().
- [P3] import.rs:194-205: the probe ignores remove_dir's result; a stray empty probe folder can remain.
- [P3] import.rs:704-710: `write_failure_returns_an_error` only reaches the not-a-folder refusal, not a failure inside import::write.
- [IDEA] storage ops should expose a public path validator (e.g. ops::check_dir) so the dialog stops probing with create_folder/remove_dir.
- [P3] Escape only closes the dialog while focus is inside it; clicking the workspace behind the overlay sends Escape to CancelRequest.
- [NOTE] import::write writes collection defaults and environments before requests, so a mid-import failure leaves a partial import (already tracked under task 94).

### Follow-ups from task 94 (not 0.2.0 wave 1)
- [P3] 34-43 curl.rs:355: unknown options fail open; their argument can become the committed URL. Prefer the `://` positional, or fail with a Parse error naming the option.
- [P3] 34-46 curl.rs:404: the extra-URL warning cuts at `?` or `#` before stripping userinfo, leaking part of a password; echo only the host.
- [P3] 34-50 curl.rs:1228: the new header-name tests skip terminated and whole-value checks; the "X Api Key" case passes without the fix.
- [P3] 34-47 (Nit) curl.rs:1047: the every-option test omits preproxy, json and post-7.84 entries; iterate the const.
- [P3] Collection- and folder-level Postman auth and variables are warnings only. Mapping them needs new `ImportResult` fields; most Postman collections put auth there.
- [P3] Postman folders count toward no cap (about 870k empty folders fit in 10 MiB).
- [P3] An environment name that is a reserved device name (for example server description "con") aborts the import partway through.
- [P3] openapi.rs:451-458 (34-22, Nit): the example budget is charged after `to_string_pretty`, so a deep example briefly allocates about 60 times its size (728 MB peak for 9.8 MiB).
- [P3] 34-34 postman.rs:65: generated secret names can shadow collection variables, which they outrank at resolve time; reserve collection var keys in SecretSink first.
- [P3] 34-35 postman.rs:195: unsupported auth types fall back to Inherit and get overwritten by URL userinfo, contradicting the warning; apply userinfo only when no auth object exists.
- [P3] 34-36 tests/import.rs:199: credential end-to-end test skips URL userinfo and the secrets .gitignore.
- [P3] openapi.rs: an OpenAPI server `description` becomes the environment file name, so credentials typed into a description reach a file name.
- [P3] `unique` in write.rs is quadratic in the number of colliding names; use a HashSet plus a per-base counter, as postman.rs does.
- [P3] `key_string` (yaml.rs) silently collapses null, sequence and mapping keys.
- [P3] 34-9 curl.rs:467: `-d` without Content-Type becomes text/plain; curl sends urlencoded.
- [P3] 34-11 curl.rs:401: warnings echo raw headers, extra URLs and fields (security).
- [P3] 34-12 curl.rs:385: dropped constructs give no warning.
- [P3] 34-13 write.rs:54: validate `folder_rel` before any write.
- [P3] 34-14 yaml.rs:185: YAML merge keys stay literal; nameless parameters are dropped silently.
- [P3] 34-16 openapi.rs:526: the depth test never reaches `check_json_depth`.
- [P3] 34-17 Cargo.toml:52: saphyr-parser lacks `default-features = false`.
- [P3] 34-18 yaml.rs:84: `TooDeep` matches saphyr's error text; pin `=0.3.3`.
- [P3] 34-19 yaml.rs:221: wall-clock asserts in tests risk flakes.
- [P3] 34-20 write.rs:207: `unique_stem` is quadratic when names share a slug.
- [P3] 34-21 write.rs:122: warn on `{{var}}` used but not defined by the import (security).
- [P3] 34-27 openapi.rs capped-names test accepts both Ok and TooLarge and asserts under 900 ms; pin one outcome, drop or loosen the timing assert.
- [P3] 34-28 openapi.rs:27 visit work shares the 64 MiB example budget; large legitimate specs may fail with a vague error. Use a separate visit counter scaled to input.
- [P3] 34-29 (Nit) openapi.rs:44 `capped_text` duplicates `truncate_bytes`; the two MAX_NAME_BYTES constants differ (100 vs 80).
- [P3] postman.rs: collection-variable keys go into secret names unsanitised; a key with `}}` or trailing whitespace makes an unresolvable ref.
- [P3] `split_userinfo`: a password containing raw `/`, `?` or `#` stays in the URL (curl does not treat it as userinfo either).
- [STYLE] mod.rs:441 test `split_userinfo_ignores_at_after_authority` sits above `use super::*;`.
- [NOTE] All non-empty Postman collection variables (including e.g. baseUrl) now go to the imported secrets environment and resolve only when it is selected; the Import dialog's secrets note covers this (task 96).
- [NOTE] User decision 2026-11-10: imported file parts become disabled empty parts (never a path); URL userinfo becomes Basic auth via secrets, explicit auth wins.
- [NOTE] Re-review: an unsafe file part is saved as `""` and loads as the request's directory, so Send fails with a read error rather than "no file selected". On Windows, nested 80-byte folder names can exceed MAX_PATH for other tools.
- [NOTE] `write` is not atomic; an error partway through leaves a partial import.
- [NOTE] saphyr-parser 0.3.3 buffers whole flow collections (scanner.rs:866-880); bounded by the 2 MiB YAML cap to about 450 MB. Accepted.
- [P3] 34-40 curl.rs:969: the expand-options test asserts only that credentials are absent, not that they were routed to secrets/auth.
- [P3] 34-41 (Nit) openapi.rs:251: the userinfo warning re-parses the host with `split_once` and can name the wrong host; return the authority from `split_userinfo`.
- [P3] curl.rs:21: arg-taking options newer than curl 7.84 are still missing (`ipfs-gateway`, `keepalive-cnt`, possibly `sigalgs`, `knownhosts`, `parallel-max-host`); the list was built from local curl 7.84 `--help all`. Regenerate from a current curl.
- [P3] curl.rs:381: `-h <category>` does not consume its argument (`curl -h all http://h/x` imports `all` as the URL); harmless.
- [NOTE] `is_credential_header` uses a broad substring heuristic (auth, token, secret, key, session, cookie, password, passwd, credential, signature), so headers like X-Monkey move to secrets; accepted, fail-safe.
### Follow-ups from task 95 (not 0.2.0 wave 1)
- [P3] `engine.rs:814-815`: the engine follows 9 redirects; reqwest follows 10.
- [P3] `engine.rs:834-838`: `transfer-encoding` is not stripped when the body is dropped on a rewrite.
- [P3] The 301/302 method-rewrite behaviour differs from reqwest's for SigV4/Digest requests (allowed by RFC 9110).
- [P3] `sigv4.rs:142-146`: a user-supplied `x-amz-*` header is sent alongside the added one, so values are duplicated.
- [NOTE] `replay` copies the body for every SigV4 request; it could be limited to `own_redirects || digest`.
- [NOTE] No test covers A→B→A; the code is correct.
- [NOTE] The S3 canonical URI and the `+` handling in queries are unproven against AWS.
- [NOTE] No test covers a 303 or a POST→GET rewrite on the own-redirect path (reviewer's suggestion).
- [P3] 33-6 `digest.rs:95`: the challenge locator matches `digest` inside a quoted value and gives up; params with bad whitespace are dropped.
- [P3] 33-7 `engine.rs:613`: Digest refuses File bodies, though the engine can reopen and resend them; the message is wrong.
- [P3] 33-8 `engine.rs:772`: an unsupported Digest algorithm or qop turns the 401 into an engine error that hides the response. Return the 401 with a warning, and truncate the echoed algorithm.
- [P3] 33-11 `sigv4.rs:148`: `sign` returns `Result<_, String>` and `DigestError` hand-implements `Error`; derive thiserror errors per the conventions.
- [P3] 33-12 `tests/auth.rs:146`: hand-made temp files leak on panic; use `tempfile::NamedTempFile`.
- [NOTE] 33-15 `tests/auth.rs:306`: use `assert_eq!`.
- [P3] `sigv4.rs:114` `canonical_query` decodes lossily, so `%FF` is signed as `%EF%BF%BD`. Fix: percent-encode the decoded bytes directly.
- [P3] No test signs a multipart body for a non-S3 service, so the boundary bytes are covered by no test.
- [NOTE] `hash_file` reopens the file by path, leaving a gap between hash and send for a same-length replacement. Hashing is not covered by the request timeout and continues after a cancel.

### Follow-ups from task 93 (not 0.2.0 wave 1)
- [P3] `history.rs:246`: data-dir creation failure shows "History entry dropped" instead of "History is off: <reason>" and never disables history.
- [P3] `tests/history.rs` `evicts_oldest_over_the_byte_cap` does not assert bytes <= cap or that the survivors are the newest run.
- [IDEA, user 2026-11-10] Opt-in setting to keep `Set-Cookie` values unredacted in history (developer tool); off by default.
- [P3] 35-5 `workspace.rs:612`: up to 1 MiB response body copied on the UI thread even when history is off; check history state first, read spilled bodies in the background.
- [P3] 35-6 `history.rs:293`: request body in `.req.json` is uncapped; one large body evicts all other history. Cap at BODY_CAP with a truncated flag.
- [P3] 35-7 `history.rs:357`: every append re-reads the index and stats every body file; store byte count in `HistoryEntry`.
- [P3] 35-8 `tests/history.rs:58`: redaction tests cover only Bearer and two headers; add a table test over every Auth variant, Cookie, mixed case, `Bearer {{t}}`.
- [P3] 35-9 (Nit) `history.rs:123`: `keep_var_ref` round-trips through `Auth::Bearer` to reach a private core helper; make core `keep_var_ref` pub; dedup names with `eq_ignore_ascii_case`.
- [P3] `history.rs` `redact_json_body`: once anything is redacted the JSON body is re-serialised compactly (keys re-sorted, formatting lost, integers beyond i64/u64 become lossy f64).
- [P3] `history.rs` `redact_json_body`: restoring masked `{{refs}}` uses one `text.replace` per ref, O(refs x body); do a single pass.
- [P3] 35-21 `history.rs:370`: `mask_bare_refs` is quadratic with allocation on bodies full of `{{` while both locks are held; scan forward over grammar chars only, then require `}}`.
- [P3] 35-22 `history.rs:665`: duplicate ids in the index can delete files a surviving line still uses; skip deletion for ids still present.
- [P3] 35-23 core `redact.rs:5`: public fixed-length array type; history repeats the body-matching logic inline. Expose a slice or `contains_credential_word`.
- [P3] 35-24 `tests/history.rs:590`: no positive tests that padded, dotted, `$` and Unicode refs survive, or uppercase Text secrets.
- [P3] 35-25 (Nit) `redact.rs:29`: test name `credential_names` is not a sentence.
- [P3] 35-27 (Nit) `import/mod.rs:370`: `is_credential_header` is a one-line passthrough; call the core fn directly.
- [NOTE] `history.rs`: `lock()` blocks without timeout while `APPEND_LOCK` is held, so a stalled second Wisp process stalls appends for every collection in this one.
- [NOTE] Name heuristic over-redacts (e.g. `monkey`, `auth_url`, `token_url`); accepted.
- [P3] 35-16 `history.rs:159`: unquoted `{{ref}}` under a secret key is emptied instead of kept.
- [P3] 35-17 `history.rs:511`: lock file opened with create and follows symlinks; use O_NOFOLLOW or a `symlink_metadata` check.
- [P3] 35-19 (Nit) `tests/history.rs:355`: "1234" leak check can match the random hex entry id.
- [P3] `history.rs:617-622`: index lines with no valid id are dropped silently on rewrite; add a comment or test.
- [P3] `tests/history.rs` `sweep_keeps_entries_with_unparseable_index_lines`: set a low entry cap so eviction runs and the verbatim-rewrite path is covered.
- [NOTE] Text/XML bodies containing any credential fragment are not stored (fail closed); a bare `{{ref}}` with characters outside letters, digits, `_`, `-`, `.` makes a JSON body drop. Accepted, safe side.


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
