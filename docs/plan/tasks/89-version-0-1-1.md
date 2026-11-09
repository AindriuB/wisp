# 89 — Bump the workspace version to 0.1.1

**Repo:** .
**Depends on:** 83, 84, 85, 86, 87, 88
**Owns:**
- Cargo.toml (the `[workspace.package] version` line only)
- Cargo.lock (the `wisp-app`, `wisp-core`, `wisp-storage` version lines only)
- THIRD-PARTY-NOTICES (regenerated only)
- docs/plan/tasks/89-version-0-1-1.md (the `## Notes` section only)

Branch `task/89-version-0-1-1`, worktree `.worktrees/wisp/89-version-0-1-1`,
`CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-89`. Start only after
83-88 are all merged, then `git fetch origin && git rebase origin/main`. Commit
on the branch; do not push.

## Goal
Version 0.1.1 carries the fixes from tasks 83-88. This task is only the version
bump, so the release commit is easy to read.

## Context
- Cargo.toml:5-9 — `[workspace.package]`, `version = "0.1.0"`.
- Cargo.lock — `name = "wisp-app"`, `"wisp-core"`, `"wisp-storage"`, each followed by `version = "0.1.0"` (about :8198, :8213, :8234 on 98738b3; they may have moved).
- scripts/release/package.sh:20-22 — the archive name comes from the wisp-app version in `cargo metadata`.
- docs/conventions.md#dependencies — regenerate notices after any `Cargo.lock` change (cargo-about 0.9.3).

## Acceptance
- [ ] AC1 Cargo.toml `[workspace.package] version = "0.1.1"`. In Cargo.lock exactly the three wisp-* version lines change, 0.1.0 → 0.1.1, and nothing else.
- [ ] AC2 `cargo metadata --format-version 1 --no-deps --locked` reports 0.1.1 for wisp-app, wisp-core and wisp-storage.
- [ ] AC3 `scripts/notices/generate.sh --check` exits 0. If it did not after the bump, THIRD-PARTY-NOTICES is regenerated in the same commit, and Notes says why it changed.
- [ ] AC4 `cargo build --locked -p wisp-app` succeeds.
- [ ] AC5 The diff touches only the files in Owns.

## Checks
- `lock-diff` command — AC1
  run: `test "$(git diff main...HEAD -- Cargo.lock | grep -c '^[-+]version')" -eq 6 && test "$(git diff main...HEAD -- Cargo.lock | grep -c '^[-+][^-+]')" -eq 6`
- `metadata` command — AC2
  run: `cargo metadata --format-version 1 --no-deps --locked | jq -e '[.packages[] | select(.name|startswith("wisp-")) | .version] | unique == ["0.1.1"]'`
- `notices` command — AC3
  run: `scripts/notices/generate.sh --check`
- `build` command — AC4
  run: `cargo build --locked -p wisp-app`
- `scope` command — AC5
  run: `git diff --name-only main...HEAD | grep -vxE 'Cargo.toml|Cargo.lock|THIRD-PARTY-NOTICES|docs/plan/tasks/89-version-0-1-1.md' | wc -l | grep -qx ' *0'`

## Out of scope
- README, docs/architecture.md, about.toml and other "0.1.0" text (the scribe updates the docs at record).
- Tagging, release.yml runs, publishing or the after-action report (the orchestrator does these).
- Any dependency update.

## Notes
