# 91 — Add wave-1 dependencies and storage module stubs

**Repo:** .
**Depends on:** none
**Owns:**
- Cargo.toml (`[workspace.dependencies]` insertions only)
- crates/core/Cargo.toml
- crates/storage/Cargo.toml
- Cargo.lock
- THIRD-PARTY-NOTICES
- crates/storage/src/lib.rs (two `pub mod` lines only)
- crates/storage/src/data_dir.rs (new, stub)
- crates/storage/src/import/mod.rs (new, stub)

## Goal
Land every Cargo manifest, lock and notices change for Wisp 0.2.0 wave 1 in one
small task, so tasks 92, 94 and 95 never touch the same manifest or
`Cargo.lock`. Also declare the two new storage modules as empty stubs so tasks 92
and 94 own disjoint files and neither edits `lib.rs`.

## Context
- Spec (lands with PR #30): `docs/plan/specs/2026-11-09-wisp-0-2-0.md#dependencies-and-size-estimates`
- `docs/conventions.md#dependencies` — `default-features = false`, notices regen, permissive licences only
- `Cargo.toml:31-60` — `[workspace.dependencies]` shape; members use `workspace = true`
- `Cargo.lock` already holds `sha2 0.10.9`, `hmac 0.12.1`, `md-5 0.10.6` (all on `digest 0.10`) and `uuid 1.29.0`
- Worktree: `/Users/Andrew/workspace/wisp/.worktrees/wisp/91-deps-scaffold`; build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-91`

## Acceptance
- [ ] AC1 `[workspace.dependencies]` gains `sha2 = "0.10"`, `hmac = "0.12"` and `md-5 = "0.10"`, each with `default-features = false` and only the features needed (`sha2` must provide `Sha256` and `Sha512_256`). They resolve to the `digest 0.10` versions already in `Cargo.lock`; no second `digest` version is added.
- [ ] AC2 `crates/core/Cargo.toml` depends on `sha2`, `hmac` and `md-5` with `workspace = true`.
- [ ] AC3 `[workspace.dependencies]` gains `saphyr` with `default-features = false`; if `saphyr` cannot be built on the pinned toolchain or its licence is not in `about.toml`'s accepted list, use `yaml-rust2` instead and say which in the PR body. `serde_yaml` (and `serde_yml`) appear nowhere in `cargo tree --workspace`.
- [ ] AC4 `crates/storage/Cargo.toml` depends on the YAML crate and on `uuid` with `workspace = true`.
- [ ] AC5 `crates/storage/src/lib.rs` gains exactly `pub mod data_dir;` and `pub mod import;`. `data_dir.rs` and `import/mod.rs` contain only a `//!` module doc line each.
- [ ] AC6 `THIRD-PARTY-NOTICES` is regenerated with `scripts/notices/generate.sh` and `--check` passes.
- [ ] AC7 The PR body states, for each new or newly-used crate, its version, licence, and its new transitive crates (from `Cargo.lock`), as the conventions require.
- [ ] AC8 Nothing else changes: no source file other than the three named above.

## Checks
- `deps-tree` command — AC1, AC2, AC3, AC4
  run: `cargo tree -p wisp-core -e normal -i md-5 && cargo tree -p wisp-core -e normal -i hmac && cargo tree -p wisp-storage -e normal -i uuid && ! cargo tree --workspace -e normal | grep -E 'serde_y(a)?ml' && test "$(grep -c '^name = "digest"$' Cargo.lock)" = "$(git show main:Cargo.lock | grep -c '^name = "digest"$')"`
- `notices` command — AC6
  run: `scripts/notices/generate.sh --check`
- `build-and-lint` command — AC5
  run: `cargo build --workspace && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
- `scope` command — AC8
  run: `test -z "$(git diff --name-only main...HEAD -- crates | grep -vE '^crates/(core|storage)/Cargo.toml$|^crates/storage/src/(lib.rs|data_dir.rs|import/mod.rs)$')"`
- `pr-body` read — AC3, AC7
  how: the PR body names the YAML crate chosen and lists version, licence and new transitive crates for each dependency.

## Out of scope
- Any code using these crates (tasks 92, 94, 95).
- Adding crates to `crates/app/Cargo.toml`.
- Bumping any existing dependency, including the GPUI pins.

## Notes
- Size: unused crates are not linked, so this task changes no binary size. The YAML crate's cost is measured by task 98.
