# theme-palette — Give Wisp its own light and dark theme

**Repo:** `.`
**Depends on:** none
**Owns:**
- crates/app/src/theme.rs (including its inline unit tests)
- crates/app/src/theme.json (new)
- crates/app/Cargo.toml (only the one `serde_json.workspace = true` line)
- Cargo.lock (only if cargo changes it; no new crates)
- crates/app/src/ui/root.rs (only the banner lines ~298-315, ~552-566 and the overlay containers ~701-760; root.rs is also touched by 0.3.0 work, keep the diff confined)
- crates/app/src/ui/sidebar.rs
- crates/app/src/ui/response/headers.rs
- crates/app/src/ui/palette.rs

The id is non-numeric on purpose: the 0.3.0 spec is claiming numeric ids in parallel. Design approved by the user 2026-11-14.

## Goal
Replace gpui-component's default shadcn neutral themes (dark is #fafafa on #0a0a0a; light status colours fail WCAG as text; warning banners are 1.9:1) with Wisp's own "Wisp Light" and "Wisp Dark". No pure-black backgrounds in dark mode. Every colour stays a theme token: crates/app has no hard-coded colours today and none may be added outside `theme.json`.

## Design
- `crates/app/src/theme.json`, gpui-component 0.8.1 theme-set format (same shape as `gpui-component-0.8.1/src/theme/default-theme.json`): themes "Wisp Light" (`mode: light`) and "Wisp Dark" (`mode: dark`), `is_default: false`, each defining every key the default theme defines in `colors` (94 keys) and `highlight`. Compiled in with `include_str!`.
- `theme.rs`: a fn taking `&str` parses the JSON (serde_json; add `serde_json.workspace = true` to crates/app/Cargo.toml — already a workspace dep and already in the binary) into gpui_component's `ThemeSet`/`ThemeConfig`. `apply` installs the two configs as `Theme`'s `light_theme`/`dark_theme`, then switches mode exactly as today (System/Light/Dark). A parse failure logs and falls back to today's behaviour; it never panics. All switching already goes through `theme::apply` (main.rs:181, ui/root.rs:161, ui/root.rs:506), so main.rs and settings.rs do not change.

### Base tokens (dark / light)
| token | dark | light |
|---|---|---|
| page | `#1b1d24` | `#fbfbfd` |
| panel | `#16181e` | `#f2f3f7` |
| raised | `#23262f` | `#ffffff` |
| hover | `#2b2e39` | `#e8eaf0` |
| selected | `#2c3352` | `#e3e6fa` |
| border | `#323644` | `#dcdfe7` |
| input border | `#6c738b` | `#8a90a1` |
| text | `#e4e6ee` | `#1b1d26` |
| muted text | `#9ba1b2` | `#5c6172` |
| accent | `#aab6ff` | `#3f4bbd` |
| accent hover | `#bcc6ff` | `#3540a6` |
| accent active | `#9aa7f5` | `#2e3891` |
| ink (on-accent / on-status) | `#14161c` | `#ffffff` |
| text selection | `#3d4775` | `#c7cdf7` |
| success | `#74d49e` | `#1c7543` |
| warning | `#e8b85c` | `#8a5800` |
| danger | `#f2877e` | `#bb3329` |
| info | `#6fc8e2` | `#0b6a85` |
| overlay (scrim) | `#0d0e1299` | `#1b1d2640` |

### Key mapping (gpui-component key -> token)
- page: background, list.background, table.background, tab.active.background, accordion.background
- panel: sidebar.background, title_bar.background, status_bar.background, tab_bar.background, tab_bar.segmented.background, list.head.background, list.even.background, group_box.background, skeleton.background, description_list_label.background
- raised: popover.background
- hover: accent.background, muted.background, secondary.background, secondary.hover.background, list.hover.background, sidebar.accent.background
- selected: secondary.active.background, list.active.background
- border: border, sidebar.border, title_bar.border, status_bar.border, window.border, table.row.border, chart.grid
- input border: input.border, switch.background
- text: foreground, popover.foreground, accent.foreground, secondary.foreground, sidebar.foreground, sidebar.accent.foreground, group_box.foreground, description_list_label.foreground, tab.active.foreground, caret
- muted text: muted.foreground, tab.foreground, table.head.foreground
- accent: primary.background, ring, list.active.border, drag_border, progress_bar.background, slider.bar.background, link.foreground, sidebar.primary.background
- accent hover: primary.hover.background, link.hover.foreground; accent active: primary.active.background, link.active.foreground
- ink: primary.foreground, slider.thumb.background, sidebar.primary.foreground
- text selection: selection.background; drop_target.background -> accent at ~25% alpha
- success/warning/danger/info.background -> that status colour; their `.foreground` -> ink
- base.red/green/yellow/cyan -> danger/success/warning/info; base.blue/magenta, the `.light` variants, chart.1-5 (an accent ramp), bullish/bearish: harmonious values from the same family
- tab.background, scrollbar.background: transparent; scrollbar.thumb.background / scrollbar.thumb.hover.background -> muted text at ~55% / ~80% alpha
- overlay -> overlay
- highlight: editor.background -> page, editor.foreground -> text, editor.active_line.background -> panel, editor.line_number -> muted text, editor.active_line_number -> text; status-ish highlight backgrounds (error/warning/success/info/created/deleted/modified/hint) tinted from the matching status colour over page; keep the default syntax colours but adjust any that miss 4.5:1 against editor.background.

### Styling fixes (code, tokens only)
1. Warning banners, ui/root.rs:306 and ui/root.rs:560: replace the solid `warning` fill + `warning_foreground` with a tint: bg = warning blended over background at ~17% (target ≈ `#3a3222` dark / `#fbefd6` light), text = `foreground`, 1px bottom border in `border`. The blend lives in one helper in theme.rs (e.g. `banner_bg(theme)`) used by both sites and by the tests.
2. Overlays, ui/root.rs:701-760 (palette, import, history, cookies, secrets): each full-size overlay container gets `bg(theme.overlay)` as a scrim. Event handling unchanged.
3. ui/sidebar.rs:580: the open request (`is_active`) keeps the `list_active` fill; the keyboard cursor (`selected`) gets `list_hover` fill when not active plus a 2px left border in `ring`. Every row gets a 2px transparent left border so content does not shift.
4. ui/response/headers.rs:94: the selected header cell uses `list_active`, not `selection`.
5. ui/palette.rs:228: match highlights use `primary` instead of `accent_foreground`.

## Context
- crates/app/src/theme.rs (whole file, 15 lines) — today's `apply`
- `~/.cargo/registry/src/index.crates.io-*/gpui-component-0.8.1/src/theme/default-theme.json` — key set and format
- gpui-component-0.8.1/src/theme/mod.rs:280-400 — `Theme::update` / `change` / `apply_config`
- gpui-component-0.8.1/src/theme/registry.rs:1-75, schema.rs:22-80 — `ThemeSet`, `ThemeConfig`
- crates/app/src/ui/root.rs:298-315, 552-566, 701-760; crates/app/src/ui/sidebar.rs:551-581
- docs/conventions.md#commits — `theme-palette: <imperative summary>`

## Acceptance
- [ ] AC1 Unit tests in theme.rs (no window) parse theme.json into exactly two themes, modes light and dark.
- [ ] AC2 A test asserts each theme defines every key in a const list of the 94 colour keys of gpui-component 0.8.1's default theme plus the `editor.*` highlight keys.
- [ ] AC3 A test asserts no opaque background-type colour in Wisp Dark (every key ending in `background`, plus editor.background; `overlay` excluded) has relative luminance below 0.008 (pure black and `#0a0a0a` fail).
- [ ] AC4 Tests recompute WCAG 2.1 contrast from the JSON hex values and assert, in both themes: text on page/panel/raised/hover/selected ≥ 4.5; muted text on page/panel/raised/selected ≥ 4.5; accent, success, warning, danger, info on page/panel/raised/selected ≥ 4.5; ink on primary and on each status fill ≥ 4.5; text on selection ≥ 4.5; warning and text on `banner_bg(...)` ≥ 4.5; ring and input.border on page and raised ≥ 3.0; every syntax colour on editor.background ≥ 4.5.
- [ ] AC5 The parse is a fn taking `&str`; a test feeds it broken JSON and shows it returns an error / fallback without panicking, and `apply` logs and keeps today's behaviour on that path.
- [ ] AC6 Styling fixes 1-5 above are in the diff; both banner sites call the single theme.rs helper; overlay event handling is unchanged.
- [ ] AC7 No hard-coded colour added outside the theme files: `rg -n 'rgb\(|rgba\(|hsla\(|#[0-9a-fA-F]{6}' crates/app/src --glob '!theme.json' --glob '!theme.rs'` finds nothing the base commit did not.
- [ ] AC8 `cargo fmt --check`, `cargo clippy -p wisp-app --all-targets -- -D warnings` and `cargo test -p wisp-app` pass.
- [ ] AC9 No new crates in Cargo.lock; Cargo.toml changes only by the serde_json line.
- [ ] AC10 Commits follow docs/conventions.md#commits; root.rs diff confined to the banner and overlay lines.

## Checks
All cargo commands run with `CARGO_TARGET_DIR=target/wt-theme-palette`.
- `theme-unit` command — AC1, AC2, AC3, AC4, AC5
  run: `CARGO_TARGET_DIR=target/wt-theme-palette cargo test -p wisp-app theme::`
  (cargo test writes no JUnit XML here; tester confirms from the output that the theme tests ran, at least 5 passed, 0 ignored.)
- `app-tests` command — AC8
  run: `CARGO_TARGET_DIR=target/wt-theme-palette cargo test -p wisp-app`
- `fmt-clippy` command — AC8
  run: `cargo fmt --check && CARGO_TARGET_DIR=target/wt-theme-palette cargo clippy -p wisp-app --all-targets -- -D warnings`
- `no-hardcoded-colours` command — AC7
  run: `rg -n 'rgb\(|rgba\(|hsla\(|#[0-9a-fA-F]{6}' crates/app/src --glob '!theme.json' --glob '!theme.rs'` compared with the same at 7b756b9 (expected empty both times)
- `no-new-crates` command — AC9
  run: `git diff main...HEAD -- Cargo.lock | rg '^\+name' ; test $? -eq 1` and `git diff main...HEAD -- crates/app/Cargo.toml`
- `styling-read` read — AC6, AC10
  how: reviewer reads the root.rs, sidebar.rs, headers.rs, palette.rs diffs against fixes 1-5, and the commit subjects.

## Hand test
Launch with a temp `WISP_DATA_DIR` and `WISP_KEYCHAIN_SERVICE=wisp-themereview`, never the user's real settings or keychain. Check System/Light/Dark switching, a warning banner, each overlay's scrim, and sidebar cursor vs open request. Stop only the pid you launched; never `pkill`/`killall`.

## Out of scope
- main.rs and settings.rs (switching already goes through `theme::apply`).
- A theme picker, user-supplied themes, or loading themes from disk.
- Any other root.rs change, including 0.3.0 work in the same file.
- Restyling other components or changing layout/spacing beyond the 2px row border.
- New crates or new workspace dependencies.

## Status
PR #79 is open and under review; not merged. An earlier version of this file said "Merged at 17d1e22" and was retired too early (finding 79-7). Retire it only when the PR merges.

## Review round 1
Fixed: 79-1 (open sidebar row painted with the clamped `list_active`), 79-2 (header cell, same cause), 79-3 (contrast test ignored the library's 20%/30% alpha clamp), 79-7 (premature record).

Lows left as follow-ups: 79-4 (`eprintln!` instead of `tracing::warn`), 79-5 (`apply` reparses theme.json on every call), 79-6 (stacked overlays double-dim), the banner test does not call `banner_bg`, and the bad-JSON test asserts nothing.
