# 74 — Show each platform's real binding in shortcut hints

**Repo:** `.`
**Depends on:** none
**Owns:**
- crates/app/src/actions.rs
- crates/app/src/ui/sidebar.rs (the `sidebar-palette` button label at ~:816 and the imports it needs, nothing else)
- .github/workflows/measure.yml (insertions only: a Linux screenshot step and its upload path)

## Goal
The palette button next to the environment picker in the sidebar has the
label `"⌘K"` hard-coded, so the Linux and Windows builds show ⌘K even though
the binding there is `ctrl-k`. Every shortcut hint in the app must come from
the binding that is actually registered, so the hint and the binding cannot
drift: ⌘K on macOS, Ctrl+K on Windows and Linux.

## Context
- crates/app/src/actions.rs:25-73 — `KEYS` is the per-platform `cfg` table that `bind_keys` registers; the palette is `KEYS[2]` (`cmd-k` / `ctrl-k`).
- crates/app/src/ui/sidebar.rs:812-820 — the only hard-coded hint (`.label("⌘K")`).
- crates/app/src/ui/palette.rs:202-219 — palette rows already use `gpui_component::kbd::Kbd::global_binding_for_action`, which is derived from the registered binding and already correct on every OS. Leave it alone.
- A sweep of `crates/app/src` on 5dc4dd3 found no other user-visible hint: there is no app menu (no `set_menus`), the tooltips (`root.rs:309`, `kv.rs:352`, `request/body.rs:602`, `url_bar.rs:344`, `response/*`) name no shortcuts, and the `⌘` in `palette.rs:1` and `response/body.rs:66` is in doc comments. If you find one the sweep missed, stop and report it. Do not edit it.
- gpui-component 0.8.1 `src/kbd.rs:105` `Kbd::format(&Keystroke)` gives `⌘K` / `Ctrl+K`, but it picks the OS with `#[cfg]`, so a test can only exercise the host's OS. gpui-pre 0.3.10 `Keystroke::display(OperatingSystem)` (`src/platform/keystroke.rs:246`, `:717-776`) takes the OS as an argument but gives `ctrl-K` / `❖K` on Linux, which is not the Ctrl+K style. Write a pure formatter that takes the OS as a parameter, and build its input from `gpui::Keystroke::parse`.
- `.github/workflows/measure.yml:80-108` — on 5dc4dd3 the Linux job runs the app under `xvfb-run` but **takes no screenshot**. The upload holds only `.json` and `.log` files.
- docs/conventions.md — commit format is `74: <imperative summary>`.

## Acceptance
- [ ] `actions.rs` has a pure `pub fn` that takes a keystroke string and a `gpui::OperatingSystem` (or a `mac: bool`), plus a hint function for an action that reads the same `KEYS` entry `bind_keys` registers. Its unit tests assert `ctrl-k` on Linux and Windows gives `"Ctrl+K"` and `cmd-k` on Mac gives `"⌘K"`, and they pass on a macOS host.
- [ ] `sidebar.rs` gets the palette button label from that hint function. `rg -n '"⌘|"Ctrl' crates/app/src/ui` returns nothing, and `palette.rs` is unchanged.
- [ ] With `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-74`, `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` all pass. No string in `KEYS` changes, and no `KeyBinding` changes.
- [ ] `measure.yml` gets a Linux-only step that runs the built binary under `xvfb-run`, captures the root window to `measure-${{ matrix.target }}.png` and adds it to the upload. A `workflow_dispatch` run of measure.yml on `task/74-shortcut-hints` (you may push the branch for this) uploads a Linux screenshot whose sidebar shows `Ctrl+K`. Put the run URL in the hand-back.

## Out of scope
- Changing any keybinding, or the `KEYS` and `bind_keys` structure beyond adding read access.
- `palette.rs`, the doc comments that mention ⌘, and every file under `docs/`. The scribe updates the ⌘-only wording in PLAN.md.
- The macOS and Windows measure jobs, `scripts/measure/**`, and changes to the RSS measurement itself.
- Opening a PR or merging. Pushing the branch only to run measure.yml is allowed.

## Amendment — orchestrator (2026-11-03)

The Linux screenshot step already exists on task 71's branch (task/71-measure-screenshot, not yet on main), which owns .github/workflows/measure.yml. To keep Owns disjoint:
- .github/workflows/measure.yml is removed from this task's Owns. Acceptance item 4 is withdrawn.
- For acceptance item 5, merge `origin/task/71-measure-screenshot` into this branch before the measure.yml run (its screenshot is uploaded as `measure-x86_64-unknown-linux-gnu.png`), then run measure.yml on this branch. Depends on: 71 (for the screenshot run only; the code work does not).
