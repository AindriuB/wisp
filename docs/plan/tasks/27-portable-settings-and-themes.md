# 27 — Implement portable settings.toml and dark/light theme switching

**Repo:** .
**Depends on:** 21
**Owns:**
- crates/app/src/settings.rs
- crates/app/src/theme.rs

## Goal
Settings live in `settings.toml` beside the executable when a `portable` marker file is there, otherwise in the OS config directory, and are edited in place with `toml_edit` so user comments survive. The theme follows the system appearance by default, or is forced dark or light.

## Context
- crates/app/src/settings.rs, crates/app/src/theme.rs — frozen signatures from task 21
- docs/architecture.md#settings-location; spec "Portable settings … Nothing else persisted"
- crates/storage/src/fs.rs:113-120 — `wisp_storage::fs::atomic_write` (reuse it; do not write a second atomic writer)
- crates/storage/src/codec.rs — `toml_edit` in-place edit pattern
- ~/.cargo/registry/src/*/gpui-component-0.8.1/src/theme/mod.rs:350-400 — `Theme::sync_system_appearance`, `Theme::change`
- Build with `CARGO_TARGET_DIR=/Users/Andrew/HttpClient/target/wt-27`.

## Acceptance
- [ ] `location(exe_dir, true, _)` is `exe_dir/settings.toml`; `location(exe_dir, false, Some(c))` is `c/wisp/settings.toml`; `location(_, false, None)` is `None`. Pure and tested.
- [ ] `settings_path()` checks for `<exe dir>/portable` and otherwise derives the config dir without a new dependency: macOS `$HOME/Library/Application Support`, Windows `%APPDATA%`, Linux/other `$XDG_CONFIG_HOME` if absolute, else `$HOME/.config`.
- [ ] The file format is `theme = "system"|"light"|"dark"`, `split = "vertical"|"horizontal"`, `pretty_limit = <bytes>`, `body_memory_limit = <bytes>`. `load` on a missing file returns defaults and no warning; on unparsable TOML or a bad value it returns defaults for the bad keys plus a warning string naming the key, and keeps valid keys (tests for each).
- [ ] `save` creates parent directories, writes through `atomic_write`, changes only keys whose value differs, and preserves comments and unknown keys (test: a file with comments and an extra key round-trips byte-identical except the changed line).
- [ ] Nothing writes the settings file at startup or on load; only `save` writes (asserted by a test that `load` on a missing path leaves it missing).
- [ ] `theme::apply(System, ..)` follows the OS appearance via gpui-component and `apply(Light|Dark, ..)` forces that mode. Hand check (orchestrator, after task 28 mounts the root, window frontmost): toggling the macOS appearance with `theme = "system"` switches the window; `theme = "dark"` stays dark.
- [ ] `settings.rs` imports no `gpui` items; `cargo test -p wisp-app` passes, and `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean.

## Out of scope
- Persisting window size, open tabs, last collection or anything else (spec: nothing else persisted).
- A settings UI beyond the ToggleTheme / ToggleLayout actions (handled by the root view, task 28).
- Custom theme files or colours.

## Review round 1 — findings

Reviewed HEAD: 537a119e678a74674efaa1bdce5809ab54490888

- [P1] crates/app/src/settings.rs:155-158 (`save`) — when the existing file can't be parsed, or can't be read for any reason other than NotFound, `save` starts from an empty document and replaces the whole file (comments and unknown keys lost).
  route: implementer — return `Err` on a non-NotFound read error and on a TOML parse error instead of `unwrap_or_default()` / `DocumentMut::new()`. Add tests: a file with a syntax error and a non-UTF-8 file are left byte-identical and `save` returns Err.
  input: settings.toml = `# my notes\ntheme = "dark\nextra = "keep"\n`; load → defaults + warning; ToggleTheme → save rewrites the file with only the four known keys.
- [STYLE] crates/app/src/settings.rs:201 — test name `location_cases` isn't a sentence.
  route: implementer
  rule: docs/conventions.md, Tests: "Test names read as sentences".
- [P3] crates/app/src/settings.rs:233 (`bad_value_keeps_valid_keys`) — no case for a bad `split`, a non-integer `body_memory_limit`, or an out-of-range value.
  route: implementer — add those cases.
  input: `_ => bad.push("split")` → `_ => {}` and every test still passes.

Orchestrator decision (2026-10-30): following the OS appearance live when `theme = "system"` is implemented in task 28's root view (observe the window appearance and call `theme::apply` again); `theme::apply`'s frozen signature is unchanged. Task 27's hand check for live switching moves to task 28.
