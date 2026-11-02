# 40 — Resync the URL input when a change did not come from it

**Repo:** .
**Depends on:** none
**Owns:**
- crates/app/src/ui/url_bar.rs

## Goal
With the URL input focused, toggling a Params checkbox or deleting a Params row updates the draft and the highlight strip but not the URL text, because `on_tab` skips the text resync whenever the input is focused; the next keystroke in the URL then resurrects the param. The URL input should skip resync only for edits it emitted itself, and resync for every other draft change.

## Context
- crates/app/src/ui/url_bar.rs:160-190 — `on_input` applies the input text to the draft through `url_sync`
- crates/app/src/ui/url_bar.rs:192-212 — `on_tab` passes `!focused` to `resync` on `DraftEdited`
- crates/app/src/ui/url_bar.rs:229-246 — `resync` compares `display_url(draft)` with the input value before `set_value`
- HISTORY 2026-11-02 "Auth and body sub-tab editors" — gpui-component 0.8.1 `InputState::set_value` emits no Change event, so a programmatic set cannot loop through `on_input`, but the draft must not be re-edited from it either
- HISTORY 2026-11-02 "URL bar and request sub-tabs" — Params, the URL input and Path resync on `DraftEdited` behind equality guards so an edit never echoes back
- crates/app/src/url_sync.rs — `display_url`; a typed URL need not equal `display_url` of its own parse (for example a trailing `&` or `?`), which is why focused resync was skipped
- Build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-40`

## Acceptance
- [ ] `UrlBar` records what its own last edit produced (for example the `display_url` of the draft right after `on_input`'s `tab.edit`), and `on_tab` sets the input text on `DraftEdited` when the draft's `display_url` differs from that record, focused or not.
- [ ] The decision is a GPUI-free helper (for example `fn should_set_text(focused: bool, own_last: Option<&str>, current: &str, want: &str) -> bool`) with unit tests: focused own edit with a trailing `&` is not overwritten; focused with a Params toggle (want differs from own_last) is overwritten; unfocused and equal does nothing; unfocused and different overwrites.
- [ ] `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-40 cargo test -p wisp-app` passes; `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean.
- [ ] Hand check (orchestrator, temp copy of `my-api`, a request with two query params): click into the URL input, untick a param in Params, the URL text drops it; type a character at the end of the URL, the unticked param stays unticked and does not reappear; typing `&x=1` character by character into the focused URL produces the row with no cursor jump or doubled characters; deleting a Params row while the URL is focused removes it from the URL text.
- [ ] `git diff --stat main` lists only `crates/app/src/ui/url_bar.rs`.

## Out of scope
- `url_sync.rs` merge logic, the Params `KvTable`, Path or Headers sub-tabs.
- Rendering coloured text inside the input (rejected in task 31).
- Preserving the caret position after an external resync beyond what `set_value` does; note the behaviour in the PR.
