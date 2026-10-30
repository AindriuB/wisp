# 29 — Build the reusable key-value table editor

**Repo:** .
**Depends on:** 21
**Owns:**
- crates/app/src/ui/kv.rs

## Goal
Params, Headers and the form body all edit `Vec<KeyValue>`. One `KvTable` component gives them a dense, keyboard-friendly table: enabled checkbox, name, value, delete, and an always-present empty trailing row that becomes a real row when typed into. It emits the full row list on every change and can be reset from outside without echoing an event.

## Context
- crates/app/src/ui/kv.rs — frozen API from task 21 (`KvTable`, `KvOptions`, `KvEvent`)
- crates/core/src/request.rs:19-25 — `KeyValue { name, value, enabled }`
- ~/.cargo/registry/src/*/gpui-component-0.8.1/src/input/input.rs:300-330 — `Input::mask_toggle`; src/checkbox.rs; src/virtual_list.rs
- crates/app/src/view.rs:50-60 — `InputState` creation pattern
- Build with `CARGO_TARGET_DIR=/Users/Andrew/HttpClient/target/wt-29`.

## Acceptance
- [ ] Each row shows a checkbox (only when `options.toggles`), a name input, a value input (masked with a reveal toggle when `options.mask_values`) and a delete button; one extra empty row is always last and is never included in `rows()` or `KvEvent::Changed`.
- [ ] Typing into the trailing row's name or value appends a real row (enabled) and a fresh trailing row; deleting a row removes it; toggling, typing and deleting each emit exactly one `KvEvent::Changed(rows)` per user edit.
- [ ] `set_rows` replaces the content, reuses existing `InputState` entities where the row count allows (no per-call reallocation of every input), and emits no `KvEvent`.
- [ ] `set_annotations` shows a warning marker with a tooltip on rows whose note is `Some`; notes beyond the row count are ignored.
- [ ] Tab moves name → value → next row's name; rows over about 50 render through a virtualised list or the task records in the PR why it is unnecessary.
- [ ] The row bookkeeping (trailing row, append on type, delete, index mapping) lives in a GPUI-free inner type in the same file with unit tests.
- [ ] Hand check (orchestrator, window frontmost, after tasks 28 and 31 land): typing in the Params table adds rows and the URL bar updates.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean.

## Out of scope
- Variable highlighting inside cells (only the annotation marker).
- Multipart part rows with file values (task 32 owns its own rows).
- Drag-to-reorder rows.
