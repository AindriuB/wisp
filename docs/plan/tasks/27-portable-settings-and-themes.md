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
