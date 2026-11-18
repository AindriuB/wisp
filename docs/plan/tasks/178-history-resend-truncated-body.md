# 178 — Refuse to resend a truncated history request body until it is repaired

**Repo:** .
**Round:** 0
**Depends on:** 168, 174
**Owns:**
- crates/service/src/plan.rs (`HistoryOrigin`, the send refusal next to `body_redaction_refusal`, and their tests)
- crates/app/src/workspace.rs (`sanitized_origin`, its call in `open_history_entry`, and the `HistoryOrigin` literals in its tests only)
- crates/app/src/ui/history.rs (`origin_line` and the `HistoryOrigin` literals in its tests)

## Goal
History stores request bodies cut to `BODY_CAP` (1 MiB; Form and Multipart too)
and marks them `StoredEntry.request_truncated` (168). Opening the entry drops
that flag, so a cut body reopens as a draft and can be resent incomplete with
no warning (168's accepted residual). Carry the flag into the reopened tab and
refuse Send while the body is still the stored, cut one.

Decision: "explicit acknowledgement" is an edit to the body, the same rule the
redacted-body refusal already uses (`plan.rs:218-262`). No new prompt. This
reuses `refused_body_message` (`workspace.rs:3224`, `ui/root.rs:445`), so
`root.rs` does not change.

## Context
- crates/storage/src/history.rs:386-395 `StoredEntry.request_truncated` (read only; 168's file).
- crates/service/src/plan.rs:13-35 `HistoryOrigin` (`stored_body` is the body the tab opened with); :218-262 `body_redaction_refusal`; :319 where `prepare` applies it; tests :365 `origin`, :443 `body_origin`, :479-560 the refusal tests to mirror.
- crates/app/src/workspace.rs:2942-2992 `open_history_entry` (`stored` is the `StoredEntry`); :863-876 `sanitized_origin`; :2304 the refusal notice; test literals :4274, :4375.
- crates/app/src/ui/history.rs:122-130 `origin_line` (`body_truncated` there is the *response* body); literal :504.
- 174 owns `workspace.rs` for its `Workspace::save` hook; this task waits for it and touches only the lines named above.
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 178 history-resend-truncated-body`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-178`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 `HistoryOrigin` has a `request_truncated: bool` field. Opening a history entry whose `StoredEntry.request_truncated` is true gives a tab whose `from_history` has `request_truncated == true`; false otherwise. App test prefixed `truncated_`.
- [ ] AC2 With `request_truncated` true and the draft body equal to `origin.stored_body`, `prepare` returns `Err` with one `Problem::Syntax { location: Location::Body, .. }` whose message starts `Edit the request body before resending:` and says the stored body was cut at 1 MiB. Nothing is submitted. Service test, for a Text body and for a Form body.
- [ ] AC3 With `request_truncated` true and any change to the draft body (including switching body type), `prepare` does not refuse on truncation. Service test.
- [ ] AC4 With `request_truncated` false, every existing `plan.rs` and app resend test passes unchanged, and a redacted-and-truncated body is still refused (either message is acceptable). Service test for the combined case.
- [ ] AC5 `origin_line` for a truncated request adds the words `request body truncated at 1 MiB`, separate from the existing response-body text. App test prefixed `truncated_`.
- [ ] AC6 `refused_body_message` matches the truncation refusal, so Send shows that one notice and no `Cannot send:` notice. App test prefixed `truncated_`, or the reviewer reads that the message uses the existing prefix.
- [ ] AC7 No file outside Owns changes; fmt, clippy and the workspace suite pass.

## Checks
- `service-truncated` command — AC2, AC3, AC4
  run: `cargo test -p wisp-service --locked truncated_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([4-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
  (prefix the new `plan.rs` tests with `truncated_`.)
- `app-truncated` command — AC1, AC5, AC6
  run: `cargo test -p wisp-app --bin wisp --locked truncated_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([2-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `scope` command — AC7
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/service/src/plan.rs$' -e '^crates/app/src/workspace.rs$' -e '^crates/app/src/ui/history.rs$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC4, AC7
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `workspace-lines` read — AC7
  how: the reviewer confirms the `workspace.rs` diff is limited to `sanitized_origin`, its call site and test `HistoryOrigin` literals.

## Out of scope
- A confirm prompt or "send anyway" button (`ui/root.rs`): the edit is the acknowledgement.
- Storage changes: the cap, the flag and `.req.json` are 168's and stay as they are.
- Response-body truncation (`HistoryEntry.body_truncated`) and its text.
- `Workspace::save` and anything else 174 owns in `workspace.rs`.
- The CLI's history display (`crates/cli/**`, 173).
