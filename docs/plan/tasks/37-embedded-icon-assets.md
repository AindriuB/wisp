# 37 — Register an embedded icon AssetSource so gpui-component icons render

**Repo:** .
**Depends on:** none
**Owns:**
- crates/app/src/main.rs (insertions only: `mod assets;` and `.with_assets(...)` on the application builder)
- crates/app/src/assets.rs (new)
- crates/app/assets/icons/** (new: the copied SVGs and a `NOTICE` file)

## Goal
gpui-component 0.8.1 draws every icon (the `Select` caret, checkbox check marks, input clear/close buttons, list empty-state icons) as an SVG loaded by path through gpui's `AssetSource`. The app registers no asset source, so every icon renders blank; this is the cause of PLAN M0 note 06 (no chevron on the method select). Register a small embedded `AssetSource` that serves exactly the icons the app uses, at the paths gpui-component requests, without embedding the whole 1,830-icon catalog.

## Context
- crates/app/src/main.rs:61-63 — `gpui_platform::application().run(|cx| …)`; insert `.with_assets(assets::Icons)` (or whatever the type is called) between `application()` and `.run`. Confirm `with_assets` exists on the returned `Application` in the locked gpui-pre version.
- ~/.cargo/registry/src/*/gpui-component-0.8.1/src/icon.rs:12-38 — `IconName` is generated from `gpui_kit_assets::__component_icon_names!`; `IconNamed::path()` returns `icons/<kebab-name>.svg` (e.g. `IconName::ChevronDown` → `icons/chevron-down.svg`). Use `IconNamed::path()` in the code and tests rather than hard-coded strings.
- ~/.cargo/registry/src/*/gpui-kit-assets-0.8.1/assets/icons/ — where the SVGs actually live (gpui-component itself ships none). `default-icons.txt` lists the 101 icons gpui-component's `IconName` covers. `LICENSE-LUCIDE` (ISC) covers the Lucide SVGs; the crate itself is Apache-2.0.
- ~/.cargo/registry/src/*/gpui-kit-assets-0.8.1/src/native_assets.rs:13-40 — the reference `AssetSource` impl (`load` returns `Ok(None)` for an empty path; `list` filters by prefix).
- Icons used inside the components the app mounts, found by `rg -o 'IconName::[A-Za-z]+' <gpui-component src>/<component>`: `select.rs` ChevronDown, Inbox; `checkbox.rs` Check; `list/` Inbox, Search; `input/` ChevronDown, ChevronLeft, ChevronRight, Close, Copy, Eye, EyeOff, Minus, Plus, Asterisk, CaseSensitive, Replace; `button/` Check, Loader, Plus; `tab/` BookOpen. Re-run this for every gpui-component module the app mounts (also `menu`, `popover`, `dialog`, `notification`, `tooltip`, `tree.rs`, `scroll` if used) on `main` and on the tips of `task/30-*` … `task/34-*`.
- App-side uses today: `rg -o 'IconName::[A-Za-z]+' crates/app /Users/Andrew/workspace/wisp/.worktrees/HttpClient/*/crates/app` → Close, TriangleAlert (in-flight branches).
- `rust-embed` and `gpui-kit-assets` are already in `Cargo.lock` (transitively via gpui-component), but `crates/app/Cargo.toml` is not owned here: use `include_bytes!` and a static `match`/table, no dependency changes.
- docs/conventions.md#dependencies, docs/conventions.md#tests; docs/architecture.md#budgets — binary ≤ 20,971,520 B (macOS arm64, hardware renderer).
- PLAN M0 note 06.
- Build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-37`. Keep precompiled Metal shaders; never enable `runtime_shaders`. Check `xcrun metal` works before building the app.

## Acceptance
- [ ] `crates/app/assets/icons/` contains one SVG per icon in the enumerated set, byte-identical to `gpui-kit-assets-0.8.1/assets/icons/<name>.svg` (`cmp` each against the registry copy), plus a `NOTICE` that names the source (gpui-kit-assets 0.8.1, Apache-2.0) and reproduces `LICENSE-LUCIDE` verbatim. No other files in that directory.
- [ ] `crates/app/src/assets.rs` defines a unit struct implementing `gpui::AssetSource` whose `load` returns `Ok(Some(bytes))` for every embedded path, `Ok(None)` for an empty path and for any path it does not embed (no panic, no error), and whose `list(prefix)` returns exactly the embedded paths that start with `prefix`. Bytes come from `include_bytes!`; no file I/O at runtime.
- [ ] `git diff main -- crates/app/src/main.rs` shows only the added `mod assets;` line and the `.with_assets(...)` call; `git diff main --stat -- Cargo.toml Cargo.lock crates/app/Cargo.toml` is empty.
- [ ] Unit test in `assets.rs` (name reads as a sentence): for every `gpui_component::IconName` in a `USED` list covering the enumerated set (internal component icons plus app-side uses on `main` and on the `task/30-*`…`task/34-*` tips), `load(name.path())` returns non-empty bytes beginning with `<svg`.
- [ ] Unit test in `assets.rs`: walks `crates/app/src/**/*.rs` from `env!("CARGO_MANIFEST_DIR")`, collects every `IconName::<Variant>` token, and fails naming any variant whose path does not resolve through the asset source, with a message saying to copy the SVG into `crates/app/assets/icons/` and add it to `USED`. Shown non-vacuous in the PR: temporarily removing one used icon makes it fail.
- [ ] Unit test: `load("icons/definitely-not-an-icon.svg")` and `load("")` both return `Ok(None)`.
- [ ] `cargo build --release -p wisp-app` then `RENDERER=hardware scripts/measure/measure.sh` on macOS arm64, run on `main` before and on the branch after, both with status "measured": the branch binary is ≤ 20,971,520 B and the PR states both sizes and the delta in bytes.
- [ ] Hand check (orchestrator, window frontmost), launched on a request tab: the method select shows a down chevron to the right of the method; a checked checkbox in the Params or Headers table shows a check mark; an unchecked one shows none. Screenshot or description in the PR.
- [ ] With `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-37`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` are clean.

## Out of scope
- Enabling a gpui-component or gpui-kit-assets feature, registering `gpui_kit_assets::Assets` or `AllAssets`, or adding any dependency (if `include_bytes!` proves unworkable, stop and report).
- App-specific icons that gpui-component does not already define, and any change to how views choose icons.
- Removing the `#[allow(dead_code)]` / `mounted by 28` markers in main.rs (task 35).
- Any file under `crates/app/src/ui/**` (tasks 30–34), even where an icon looks wrong there.
- Docs, `PLAN.md` note 06, `HISTORY.md` (scribe).
