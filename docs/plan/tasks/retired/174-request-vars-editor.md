# 174 — Per-request variables editor (3A, A4)

**Repo:** .
**Round:** 0
**Depends on:** 161, 165
**Owns:**
- crates/app/src/draft.rs
- crates/app/src/tab.rs
- crates/app/src/ui/request/mod.rs
- crates/app/src/ui/request/vars.rs (new)
- crates/app/src/ui/kv.rs (additive only: a `KvTable::set_row_mask(...)` setter)
- crates/storage/src/codec.rs (`write_vars` and its tests only; Amendment 1)
- crates/app/src/workspace.rs (the save validation hook in `Workspace::save` and its tests only; Amendment 2)

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
- [ ] AC3 `ui/request/vars.rs` renders a `KvTable` with `toggles: false`; editing emits a draft update that replaces `Request.vars`. A pure mapping `Var` ↔ `KeyValue` (with `enabled` true on the way in and ignored on the way out) has a round-trip test preserving order. A row whose name duplicates an earlier row's shows an inline error on that row, and the request cannot be saved while any duplicate exists (storage keys `[vars]` by name). The refusal is a validation hook in `Workspace::save` (`workspace.rs:3041`), which every save path (Ctrl+S, autosave, save-all, save-and-close) goes through: it returns `Err` before any write when the draft's vars hold a duplicate name. A test with two rows named `k` sees the second flagged, and for each of Ctrl+S, autosave, save-all and save-and-close the save is refused, the file on disk is byte-identical to before, and both rows are still in the draft (save-and-close leaves the tab open).
- [ ] AC4 `KvTable` gains an additive `set_row_mask(...)` setter (unset by default, so existing tables are unchanged; `KvOptions` keeps its fields so the literals in `params.rs`, `headers.rs`, `body.rs` still compile); the Vars table masks a row's value exactly when `is_credential_name(name)` is true, re-evaluated on rename. A pure test of the mask decision covers `api_token`, `password`, `base_url`, `page`.
- [ ] AC5 Theme tokens only; no key binding; `params.rs`, `headers.rs`, `body.rs` and storage unchanged, and `workspace.rs` changed only by the AC3 save validation hook and its tests; except that `codec.rs` `write_vars` rewrites the `[vars]` table in the draft's order so a renamed var keeps its position; a storage test with vars `[a, b, c]`, `b` renamed to `x`, saved and re-read, gets `[a, x, c]`; fmt, clippy and the workspace suite pass.

## Checks
- `vars-unit` command — AC1, AC3, AC4
  run: `cargo test -p wisp-app --bin wisp --locked vars 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([3-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
  (put `vars` in each new test name.)
- `vars-save-refusal` command — AC3 (each save path refuses; file untouched)
  run: `cargo test -p wisp-app --bin wisp --locked vars_save 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([4-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
  (one test per save path, each named `vars_save_...`.)
- `scope` command — AC5
  run: `git diff --exit-code main...HEAD -- crates/app/src/ui/request/params.rs crates/app/src/ui/request/headers.rs crates/app/src/ui/request/body.rs crates/storage ':(exclude)crates/storage/src/codec.rs' crates/core crates/service crates/cli`
- `vars-storage` command — AC5 (rename order)
  run: `cargo test -p wisp-storage --locked vars 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9][0-9]* passed; 0 failed; 0 ignored'`
  (put `vars` in the new test name.)
- `suite-and-lint` command — AC5
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `ui` read — AC2, AC4
  how: the reviewer checks the sub-tab order, the dirty mapping, that the per-row mask is a `KvTable::set_row_mask` setter left unset by other tables, that the `codec.rs` diff touches only `write_vars` and its tests, and that the `workspace.rs` diff is only the duplicate-name check in `Workspace::save` (before any write) plus its tests.

## Out of scope
- Editing `description` or `expected_status` (display only, 175); Explain's variable origins (already show request vars).
- Any change to environment editing.

## Wave stopped — step 2 — three orchestrator findings at 80dcf3f: duplicate var names lost on save (storage keys [vars] by name; AC3 asks duplicates preserved); renaming a non-last var reorders it on disk (write_vars appends; AC5 keeps storage unchanged); AC4 per-row mask is a KvTable setter, not a KvOptions field

## Amendment 1 (user, 2026-11-17)
Reason: the three findings in "Wave stopped" above. Decisions, now in Owns, AC3, AC4, AC5 and Checks:
1. Duplicate names: storage keys `[vars]` by name, so duplicates cannot be preserved. The Vars editor flags a duplicate row inline and refuses to save while one exists (AC3).
2. Rename order: `write_vars` removed the old key and appended the new one, so renaming moved the var to the end. It now rewrites `[vars]` in the draft's order; `codec.rs` (`write_vars` only) joins Owns as the one storage exception (AC5).
3. Per-row mask: a `KvTable::set_row_mask(...)` setter, not a `KvOptions` field, because a new field would break the struct literals in `params.rs`, `headers.rs`, `body.rs` (AC4).
Fix round, P3, optional: a test that `VarsEditor` actually wires `set_row_mask`.

## Wave resumed — user accepted the orchestrator recommendations (Amendment 1)

## Wave stopped — step 2 — AC3 save refusal not buildable inside Owns at 80dcf3f
- [P1/SPEC, route: orchestrator] crates/app/src/ui/request/vars.rs:70 — duplicate var names reach the draft with no inline error and no save refusal; saving `k=1`,`k=2` writes only `k = "2"` (codec.rs:152-154). Every save path (Ctrl+S, autosave, save-all, save-and-close) goes through `Workspace::save` (crates/app/src/workspace.rs:3041), which is outside Owns; AC5 freezes it and storage except `write_vars`, which returns `()`. Amendment 1 decision 1 needs Owns or AC5 to change: add a validation hook in `workspace.rs`, or make the storage save fallible on duplicates.
Held in-spec: [P2] codec.rs:134-155 `write_vars` still reorders a renamed non-last var (`[a,b,c]`→rename b→x→`[a,c,x]`); storage order test missing.
Follow-up: [P3] no test that `VarsEditor` wires `set_row_mask`.

## Amendment 2 (user, 2026-11-17)
Reason: the P1 in the last "Wave stopped" above. Decision, now in Owns, AC3, AC5 and Checks: save refusal is a validation hook in `Workspace::save` (`crates/app/src/workspace.rs:3041`). Every save path (Ctrl+S, autosave, save-all, save-and-close) goes through it, so it refuses any request whose draft vars contain a duplicate name, before writing; the Vars editor shows the inline flag. `workspace.rs` joins Owns for that hook and its tests only; AC5's "workspace.rs unchanged" is relaxed to that hook. Storage stays infallible.
Still in scope from the stop: the held [P2] `write_vars` order fix and its storage order test (AC5). Follow-up, not this round: the [P3] `set_row_mask` wiring test.

## Wave resumed — user: save refusal through a Workspace::save validation hook (Amendment 2)

## Review round 1 — findings
Reviewed HEAD de5fe2f7cba89d6a381b3be47fec9d0a3b093ea4 (full branch, after Amendment 2).
- [P2] crates/app/src/ui/request/vars.rs:39 — `duplicate_var_rows` deliberately skips blank names, but storage keys `[vars]` by name, so two blank-name rows are a duplicate key too. The `Workspace::save` hook (crates/app/src/workspace.rs:3048) lets them through, and one row is silently lost on disk (AC3: "cannot be saved while any duplicate exists").
  route: implementer — treat repeated empty names as duplicates (flag inline and refuse the save), with a test.
  input: in the Vars tab, type `1` into the trailing row's value field only (KvTable appends `{name:"", value:"1"}`); do the same with `2`; Ctrl+S → no flag, no refusal; `write_vars` calls `set_str(t, "", ..)` twice (codec.rs:155), so the file holds only `"" = "2"` while the draft and `saved` hold two rows.

## Wave verified — 2999253a44aa941408c51092f395d539943de4ef

## Outcome
Merged locally as 0bdd4c5 (task head 2999253, PASS + APPROVE, workspace 1414 passed, AC1-AC5). The request pane has a Vars sub-tab; duplicate (including blank) names are flagged inline and `Workspace::save` refuses them before writing; `write_vars` rewrites `[vars]` in the draft's order. Follow-ups are in PLAN.
