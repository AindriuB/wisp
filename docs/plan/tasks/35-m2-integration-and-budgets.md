# 35 — Integrate M2, remove the scaffold allowances and re-measure the budgets

**Repo:** .
**Depends on:** 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34
**Owns:**
- crates/app/** (wiring fixes only; runs alone in its wave)

## Goal
Close M2: every view is reachable, the `#[allow(dead_code)]` scaffold markers from task 21 are gone, leftover walking-skeleton helpers are removed, the whole v1 flow is hand-checked end to end, and the macOS arm64 budgets are re-measured with the full UI.

## Context
- crates/app/src/main.rs — `// mounted by 28, allow removed by 35` markers
- crates/app/src/format.rs — `BODY_LIMIT`, `body_text`, `completed_line` kept for `view.rs`
- scripts/measure/measure.sh — the hook and budget run
- docs/architecture.md#budgets — binary ≤ 20,971,520 B, idle ≤ 120 MB, after one request ≤ 128 MB (macOS arm64, hardware renderer)
- Task files 21–34 hand-check lists
- Build with `CARGO_TARGET_DIR=/Users/Andrew/HttpClient/target/wt-35`.

## Acceptance
- [ ] `rg -n 'allow\(dead_code\)|mounted by 28' crates/app` returns nothing; unused helpers are deleted rather than allowed.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean; the test count is at least the 161 at `49da271` plus every test added by tasks 22–34 (both counts in the PR).
- [ ] `cargo build --release -p wisp-app` then `RENDERER=hardware scripts/measure/measure.sh` on macOS arm64 reports binary ≤ 20,971,520 B, idle RSS ≤ 122,880 KiB and after-request RSS ≤ 131,072 KiB; the three figures and the commit are in the PR for `scribe` to record. If any is over, the task stops and reports rather than trimming features.
- [ ] `cargo tree -p wisp-app -e normal | rg -i 'openssl|native-tls'` is empty and `Cargo.lock` gained no package since `49da271` other than what tasks 22–34 recorded.
- [ ] Hand check (orchestrator, window frontmost) on a temp copy of the `my-api` fixture, launched as `wisp <copy>`: ⌘E picks `dev`; ⌘K opens Get user; the URL highlights `{{baseUrl}}`, hover shows its value; ⌘↵ against a local server shows status, Pretty JSON, Headers and Cookies; editing marks `●` on the tab and the sub-tab; ⌘S clears it and `git diff` shows only the edited lines with comments intact; an external edit while clean reloads silently; an external edit while dirty shows "Changed on disk", Reload takes the disk copy and Keep mine keeps the draft; deleting the file externally shows "Deleted on disk"; light and dark themes both render legibly; with no `portable` marker `settings.toml` appears only under the OS config dir and only after a toggle.
- [ ] Any defect found by the hand check that needs more than a wiring fix in a file owned by tasks 22–34 is filed as a successor task, not fixed here.

## Out of scope
- New features, dependency changes, or refactors of the modules tasks 22–34 delivered.
- Per-platform budgets for Windows, Linux or macOS x64 (user decision).
- Docs, `PLAN.md`, `HISTORY.md` (scribe).
