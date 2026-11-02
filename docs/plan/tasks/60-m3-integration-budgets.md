# 60 — Integrate M3, re-measure the budgets and hand-check

**Repo:** `.`
**Depends on:** 52, 53, 54, 55, 56, 57, 58, 59
**Owns:**
- scripts/measure/results/aarch64-apple-darwin.json

Runs alone in its wave. Worktree in `.worktrees/wisp/`; build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-60`.

## Goal
Close M3 (v1 hardening). On main with tasks 52-59 merged, confirm the workspace is clean, the macOS arm64 budgets still hold, and the `WISP_AUTOSEND_URL` hook still measures. Then hand-check the hardening changes together in one running app.

## Context
- scripts/measure/measure.sh — usage `measure.sh <binary> <output.json>`, `RENDERER=hardware`.
- docs/architecture.md#budgets — binary ≤ 20,971,520 B, idle RSS ≤ 120 MiB (122,880 KiB), after one request ≤ 128 MiB (131,072 KiB), macOS arm64 hardware renderer.
- PLAN.md — after task 50: binary 19,114,864 B, idle 97.2 MiB, after request 102.3 MiB.
- Hand-check lists in task files 52, 53 and 55.
- PLAN.md "Icon assets (37)" tip: wrap the binary in a scratch `.app` bundle if the window is not exposed to computer use.

## Acceptance
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean on the integrated main.
- [ ] `cargo build --release -p wisp-app` then `RENDERER=hardware scripts/measure/measure.sh target/wt-60/release/wisp scripts/measure/results/aarch64-apple-darwin.json` reports status "measured", with binary ≤ 20,971,520 B, idle ≤ 122,880 KiB and after-request ≤ 131,072 KiB. The three figures, their deltas from the task-50 figures, and the commit are in the report for the scribe. If any figure is over, stop and report; do not trim features.
- [ ] `cargo tree -p wisp-app -e normal | rg -i 'openssl|native-tls|aws-lc'` prints nothing.
- [ ] Hand check (orchestrator, window frontmost) on a temp copy of the `my-api` fixture: the 52, 53 and 55 checks all pass in the integrated build. Also, a 20 MiB File-body POST to a local server completes and the server's sha256 matches.
- [ ] Any defect found is filed as a successor task, not fixed here. The only file this task changes is the results JSON.

## Out of scope
- Budgets for Windows, Linux or macOS x64 (the user's call; not M3).
- Any code change.
- Updating PLAN.md, HISTORY or docs (scribe's job).
