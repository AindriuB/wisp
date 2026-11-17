# 174 — Per-request variables editor (3A, A4)

**Repo:** .
**Round:** 0
**Depends on:** 161, 165
**Owns:**
- crates/app/src/draft.rs
- crates/app/src/tab.rs
- crates/app/src/ui/request/mod.rs
- crates/app/src/ui/request/vars.rs (new)
- crates/app/src/ui/kv.rs (additive only: a per-row value-mask option)

## Goal
A "Vars" request sub-tab after "Path" lets the user add, remove, rename and edit
`Request.vars`, masking credential-like names, and the tab shows a dirty dot when
vars differ from the saved file. This closes the 0.3.0 gap where a draft could be
dirty with nothing highlighted.

## Context
- docs/plan/specs/2026-11-17-wisp-0-4-0.md "Feature 6" (:391-410), Size gate cut order (:445: A4's per-row masking is the second cut), Waves 3A (:478).
- crates/app/src/draft.rs:6-17 `DirtySections`, :45-48 the comment to delete, :56 the literal, :126-230 all-fields test literals; `workspace.rs:5264` reads `.assertions` (unaffected).
- crates/app/src/tab.rs:17-25 `RequestSubTab`.
- crates/app/src/ui/request/mod.rs:29-37 `SUB_TABS` (7 → 8), :39 the exhaustive `is_dirty` match, :115 the body match; `ui/request/path.rs` (why not reused: path vars are URL-derived).
- crates/app/src/ui/kv.rs:20-25 `KvOptions` (`mask_values` is per table today), :184-188 and :322-323 where it applies; users `params.rs:61`, `headers.rs:75`, `body.rs:199` must not change.
- `wisp_core::redact::is_credential_name`; `Var` (`core/src/request.rs:14`) vs `KeyValue` (`name`, `value`, `enabled`). Storage already round-trips `[vars]`.
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 174 request-vars-editor`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-174`. Foreground commands only; never `pkill` or `killall` (the user may be running Wisp).

## Acceptance
- [ ] AC1 `DirtySections` gains `vars: bool`; `dirty_sections` sets it when `vars` differ; the `draft.rs:45-48` comment is gone; a test changes only vars and sees `vars` true and every other flag false; the all-fields literals include `vars`.
- [ ] AC2 `RequestSubTab` gains `Vars`; `SUB_TABS` has 8 entries with "Vars" directly after "Path"; `is_dirty` maps `Vars` to `d.vars`.
- [ ] AC3 `ui/request/vars.rs` renders a `KvTable` with `toggles: false`; editing emits a draft update that replaces `Request.vars`. A pure mapping `Var` ↔ `KeyValue` (with `enabled` true on the way in and ignored on the way out) has a round-trip test preserving order and duplicates.
- [ ] AC4 `KvOptions` gains an additive per-row mask decision (default off, so existing tables are unchanged); the Vars table masks a row's value exactly when `is_credential_name(name)` is true, re-evaluated on rename. A pure test of the mask decision covers `api_token`, `password`, `base_url`, `page`.
- [ ] AC5 Theme tokens only; no key binding; `params.rs`, `headers.rs`, `body.rs`, `workspace.rs` and storage unchanged; fmt, clippy and the workspace suite pass.

## Checks
- `vars-unit` command — AC1, AC3, AC4
  run: `cargo test -p wisp-app --bin wisp --locked vars 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([3-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
  (put `vars` in each new test name.)
- `scope` command — AC5
  run: `git diff --exit-code main...HEAD -- crates/app/src/workspace.rs crates/app/src/ui/request/params.rs crates/app/src/ui/request/headers.rs crates/app/src/ui/request/body.rs crates/storage crates/core crates/service crates/cli`
- `suite-and-lint` command — AC5
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `ui` read — AC2, AC4
  how: the reviewer checks the sub-tab order, the dirty mapping and that the per-row mask option defaults off.

## Out of scope
- Editing `description` or `expected_status` (display only, 175); Explain's variable origins (already show request vars).
- Any change to environment editing.
