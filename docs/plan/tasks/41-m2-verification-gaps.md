# 41 — Hand-check the M2 verification gaps

**Repo:** .
**Depends on:** none
**Owns:**
- none (hand check only; no file in the repository changes)

## Goal
Task 35's window hand check left five behaviours unverified. Check each on a running build and report pass or fail with evidence, so `scribe` can close or reopen the PLAN items. Any failure is filed as a successor task, not fixed here.

## Context
- PLAN.md "Response viewer (33)" — Raw prefetch and the 4 MB cap line (needs about 190k distinct lines), over-limit Pretty tooltip (needs a JSON body over 5 MB), Save output `shasum`
- PLAN.md "Auth and body editors (32)" — multipart File part send
- PLAN.md "Escape (21)" — Escape is bound with context None to `CancelRequest`; unverified inside the URL input during a send
- crates/app/src/ui/response/body.rs — `RAW_DISPLAY_CAP`, prefetch gate, pretty limit
- HISTORY 2026-11-02 "Embedded icon assets" — wrap the binary in a scratch `.app` bundle with a bundle id if the window is not exposed to computer use
- Build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-41 cargo build -p wisp-app`; serve fixtures from a local server in the scratchpad (for example `python3 -m http.server` over generated files, and a small echo server for multipart and slow responses)
- Use a temp copy of `crates/storage/tests/fixtures/my-api`; settings writes go to the OS config dir, so note if `settings.toml` was touched

## Acceptance
- [ ] Raw prefetch: a text response of at least 250,000 distinct numbered lines scrolls in Raw past the first page with new lines appearing, and the cap line appears at the 4 MB display cap; report the last line number shown.
- [ ] Over-limit Pretty: a JSON body over 5 MB opens in Raw, the Pretty toggle is disabled, and its tooltip text is quoted verbatim in the report.
- [ ] Multipart File part: a multipart body with one Text and one File part sent to an echo server arrives with both parts; the File part's bytes match the file (`shasum` of both quoted).
- [ ] Escape during send: with the URL input focused and a request to a server that delays 10 s, Escape cancels the send (status shows cancelled) or does not; report which. If it does not, file a successor task naming `crates/app/src/actions.rs` and the URL input.
- [ ] Save: Save of a binary or large response writes a file whose `shasum -a 256` equals that of the served file (both quoted).
- [ ] Folder order (task 39): in a temp copy of the fixture, rename `get-user.toml` and delete `create-user.toml` in the running app; `users/folder.toml` `order` then lists the new name and not the deleted one, and the sidebar follows that order.
- [ ] `git status` in the repository is clean at the end; generated fixtures live only in the scratchpad.

## Out of scope
- Any code change, including for a failed check (file a successor task instead).
- Re-measuring the budgets, or the RSS observed with a 20 MB response.
- Defects covered by tasks 38-40.
