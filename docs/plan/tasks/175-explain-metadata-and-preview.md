# 175 — Show request metadata in Explain; deterministic URL-bar preview (3B, D3, E3)

**Repo:** .
**Round:** 0
**Depends on:** 159, 162, 165
**Owns:**
- crates/app/src/ui/explain.rs
- crates/app/src/ui/url_bar.rs

## Goal
Explain shows a request's `description` and `expected_status` read-only, and the
URL bar's live problem check stops generating a fresh UUID on every keystroke by
resolving with the placeholder dynamic source.

## Context
- docs/plan/specs/2026-11-17-wisp-0-4-0.md "Feature 3" behaviour (:292), "Feature 4" display (:323, :332), Size gate cut order (:445: D3's Explain display is the first cut), Waves 3B (:479).
- crates/app/src/ui/url_bar.rs:408 `resolve(&draft, &scope)` in the live check; `strip_text` (:54) keeps its signature (called by `ui/explain.rs` tests).
- `resolve_with` and `Placeholder` (159); `ResolutionReport.description` (`Shown::Text`, already capped at 1,000) and `expected_status` (162).
- crates/app/src/ui/explain.rs (the report pane; existing section layout to follow).
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 175 explain-metadata-and-preview`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-175`. Foreground commands only; never `pkill` or `killall` (the user may be running Wisp).

## Acceptance
- [ ] AC1 The Explain pane shows "Description" with the report's text (no editing control) when non-empty, and "Expected status" as a comma-separated list when non-empty; neither section appears when empty. A pure text-builder test covers both empty, one, and both present, and a 1,000-character description ending in "…".
- [ ] AC2 The URL bar's live problem check calls `resolve_with` with `Placeholder`; a test shows two checks of a draft using `{{$uuid}}` produce identical problem output and that `Placeholder` is the source used (no `resolve(` call remains in `url_bar.rs`).
- [ ] AC3 Theme tokens only; `strip_text`'s signature unchanged; no file outside Owns changes; fmt, clippy and the workspace suite pass.

## Checks
- `explain-meta` command — AC1, AC2
  run: `cargo test -p wisp-app --bin wisp --locked metadata 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([3-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
  (put `metadata` in each new test name.)
- `no-fresh` command — AC2
  run: `rg -n '(^|[^:.\w_])resolve\(' crates/app/src/ui/url_bar.rs; test $? -eq 1`
- `scope` command — AC3
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/app/src/ui/explain.rs$' -e '^crates/app/src/ui/url_bar.rs$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC3
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`

## Out of scope
- Editing either field in the GUI (register verdict: display only); the send path's dynamics (160).
