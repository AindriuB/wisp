# 157 — Document the CLI, the guard and the 0.3.0 release contents (3C, part 2)

**Repo:** .
**Depends on:** 153, 154, 155, 156
**Executor:** `scribe` (docs only; no implementer, no code)
**Owns:**
- docs/architecture.md
- docs/cli.md (new)
- README.md ("Install" subsections, "Unsaved changes", and a new CLI section and guard paragraph only)

## Goal
Bring the docs in line with what 0.3.0 ships: a read-only `wisp-cli` with a
documented contract, the per-user environment guard and its limits, the three
archives' new contents, and the architecture drift the spec lists. Written
after 155 and 156 merge so the docs describe merged behaviour, not plans.

## Context
- docs/plan/specs/2026-11-14-wisp-0-3-0.md "Documentation drift to fix with this release" (:351-359), Feature 4 (:236-300), "2b. Environment guard" (:119-143), Amendment 2 (task 153).
- docs/architecture.md: :25 version, :32-44 components, :56-57 "a later cli/ crate", :111 stale history location, :177-178 and :187-203 boundaries (now enforced by `scripts/check-boundaries.sh`, HISTORY "## 2026-11-15 — wisp-cli scaffold and inspect (task 135)"), :204-290 budgets (CLI 4,194,304 B per target in `scripts/measure/cli-budgets.json`; Windows GUI gate 16,000,000 B and soft line 15,900,000 B per Amendment 2; `budgets.json` 16 MiB unchanged), :291-311 release contents.
- CLI contract: `crates/cli/src/**` (commands, exit codes, `--json` schema version) and HISTORY "## 2026-11-16 — wisp-cli validate and explain (task 142)"; the CLI reads only, opens no secret store and never prints a secret value.
- Guard as merged by task 155: `crates/app/src/guard.rs`, `ui/root.rs`; storage is `confirm_send` in the per-user `settings.toml`; the committed hint is `suggest_confirm = true` in `environments/<env>.toml`; OAuth Authorize is not guarded; a renamed environment loses its guard.
- Packaging as merged by task 156: macOS `Wisp.app/Contents/MacOS/wisp-cli` (give the one-line symlink), Windows zip `wisp-cli.exe` beside `wisp.exe` (unsigned until SignPath is updated), Linux `wisp-cli` beside `wisp`, winget alias `wisp-cli` if 156 kept it, not in the MSIX.
- Save and close (task 143): the tab, collection and quit prompts are `Save and close` / `Save all and close` / `Save all and quit`, Discard, Cancel.
- Worktree: `./.claude/scripts/wt-new.sh . 157 docs-cli-guard-release`. Run every command in the foreground; never start a background command; never use `pkill` or `killall`.

## Acceptance
- [ ] AC1 `docs/cli.md` documents each command (`inspect`, `validate`, `explain`), its arguments, exit codes, the `--json` schema version and fields, and the redaction and read-only contract, each matching `crates/cli/src` at the merged head.
- [ ] AC2 architecture.md fixes every drift item at spec :353-358: version 0.3.0 pending tag (or the Cargo.toml version at the merged head), the history location, `wisp-cli` as a fourth component, the enforced boundary check, the release contents.
- [ ] AC3 architecture.md#budgets states the CLI limit per target with the wave-2 measured sizes, and the Windows GUI gate 16,000,000 B and soft line 15,900,000 B citing Amendment 2; no live doc in `Owns` still states 15,500,000 or 15,400,000 as current.
- [ ] AC4 architecture.md records where the guard and assertion data live (per-user `settings.toml` `confirm_send`; `suggest_confirm` in environment files; `[[assert]]` in request files, results only in history as records without values) and keeps the v1 "no scripting" rule's wording.
- [ ] AC5 README gains a CLI section (where the binary is on each OS, the macOS symlink line, a pointer to docs/cli.md) and a guard paragraph (per user, keyed by name, lost on rename, not applied to OAuth Authorize); the Install subsections name `wisp-cli` in each archive and the Windows unsigned-CLI note; "Unsaved changes" names the Save-first prompts.
- [ ] AC6 No file outside `Owns` changes.

## Checks
- `scope` command — AC6
  run: `git diff --name-only main...HEAD | grep -vxE 'docs/architecture\.md|docs/cli\.md|README\.md' | wc -l | grep -x 0`
- `old-gate-gone` command — AC3
  run: `! rg -n '15,?500,?000|15,?400,?000' docs/architecture.md docs/cli.md README.md`
- `contract` read — AC1, AC2, AC4, AC5
  how: the reviewer checks docs/cli.md's commands, exit codes and JSON fields against `crates/cli/src` and its tests, and each drift line against the spec list.

## Out of scope
- PLAN.md, the spec and the roadmap (task 153 and later records).
- The README's 0.2.0 introduction and release-notes text for 0.3.0: written at release time with the user.
- Any code or workflow change; a doc that disagrees with the code is reported, not fixed in code.
