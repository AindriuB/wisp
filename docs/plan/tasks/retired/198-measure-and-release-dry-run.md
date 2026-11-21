# 198 — Measure 0.5.0 and dry-run the release

**Repo:** .
**Round:** 0
**Depends on:** 183, 184, 185, 186, 187, 188, 189, 190, 191, 192, 193, 194, 195, 196
**Owns:**
- scripts/measure/results/v0.5.0-wave-3-size.json (new)

## Goal
Prove the 0.5.0 head fits its budgets, and that the release workflow produces
all three archives with `wisp-cli` inside. Hand the user a macOS hand-test
checklist. The version bump, tag and publishing stay with the user (D-I).

## Context
- docs/plan/specs/2026-11-19-wisp-0-5-0.md "Size gate" (cut order), "Success", and D-I.
- Baselines: `scripts/measure/results/v0.4.0-wave-3-size.json` (Windows GUI 15,616,000 B) and task 184's `v0.5.0-trust-size.json`. The gate is 16,000,000 B and the soft line 15,900,000 B. The CLI limits are in `scripts/measure/cli-budgets.json`.
- docs/plan/tasks/retired/177-measure-and-release-dry-run.md: the same procedure for 0.4.0.
- A draft release is outward-facing: dispatch `release.yml` only after the orchestrator confirms the user approved it. `measure.yml` needs no approval.
- Worktree: `./.claude/scripts/wt-new.sh . 198 measure-and-release-dry-run`; no local build. Foreground commands only. Wait with one foreground `gh run watch <id> --exit-status`.

## Acceptance
- [ ] AC1 A `measure.yml` dispatch on main at the 0.5.0 head (all dependencies merged) completes green on every target. The run id and head commit are recorded.
- [ ] AC2 The result file records, per target, the GUI and CLI bytes, the deltas against 0.4.0 and against 184, the Windows GUI against 16,000,000 B and 15,900,000 B, and each CLI against its limit. Over the soft line: the PR says so first and lists the spec's cut order, without cutting. Over the gate: stop and report.
- [ ] AC3 After approval, a `release.yml` dispatch with `draft_release=true` on the same head completes green. Each archive lists `wisp-cli` (the listing is quoted in the PR), and `SHA256SUMS` lists exactly three `Wisp-*` files.
- [ ] AC4 The PR body carries a macOS hand-test checklist:
  - **Trust:**
    - a send to the homelab host whose Caddy root is in the System keychain succeeds with system trust on;
    - with system trust off it fails, and the hint line is shown;
    - with "Choose CA certificate file…" and system trust off, it succeeds;
    - a public HTTPS site still works with system trust on and off;
    - `wisp-cli run --ca-bundle` and `--no-system-trust` give the expected exits.
  - **Compare:**
    - "Compare with previous" on a JSON API;
    - Ignore a `Date` header and a body field, then confirm `collection.toml` gained them;
    - an entry recorded with history bodies off shows the "not stored" note.
  - **GraphQL:** a query with variables to a public GraphQL API; Format; save, reopen and send.
  - **Postman:** import a collection with status, header and JSON tests; the checks appear and pass; untranslated tests give one warning.
  - **Regression:** a 0.4.0-era history entry opens and compares with "recorded before 0.5.0".
  - **Signing:** the bundled `Wisp.app/Contents/MacOS/wisp-cli` passes `codesign --verify` and `spctl`.
- [ ] AC5 No file other than the result file changes. The dry-run draft is left for the user to delete.

## Checks
- `measure-run` command — AC1
  run: `gh run view "$(jq -r .run_id scripts/measure/results/v0.5.0-wave-3-size.json)" --json conclusion -q .conclusion | grep -x success`
- `release-run` command — AC3
  run: `gh run view <release-run-id> --json conclusion -q .conclusion | grep -x success`
- `only-result` command — AC5
  run: `test "$(git diff --name-only main...HEAD | grep -vx scripts/measure/results/v0.5.0-wave-3-size.json | wc -l | tr -d ' ')" = 0`
- `figures` read — AC2, AC3, AC4
  how: the bytes match the run logs, the thresholds are stated, and the listings and checklist are in the PR.

## Out of scope
- The version bump, tag, publishing and deleting the draft (all the user's); Windows and Ubuntu hand tests (D-I); any size cut.

## Wave stopped — step 1 — AC3 needs user approval to dispatch release.yml (draft_release=true) on main at 3a52ac10fa699e24e26ed09e0bd8ce5ab3f9a814; AC1, AC2 done (measure run 44926945513), branch pushed, no PR yet

## Wave resumed — user approved (2026-11-21, in chat: "Yes, run it") the draft release.yml dispatch; main moved to 58d75d8 (docs-only PR #165), so re-run measure.yml on that head first, then dispatch release.yml draft_release=true on the same head

## Wave verified — a05e3bb018370f77d43d5daffca9d89a70ade47b

## Outcome
Landed on `task/198-measure-and-release-dry-run`. Measure run 44929534630 and release dry run 44930780226 (`draft_release=true`) both succeeded on main 58d75d8. Windows GUI 15,823,872 B: 76,128 B under the 15,900,000 B soft line and 176,128 B under the 16,000,000 B gate; every CLI under 7,340,032 B (Linux tightest, 856,968 B headroom). Nothing was cut. The only file added is `scripts/measure/results/v0.5.0-wave-3-size.json`. The draft release `dryrun-44930780226` is left for the user to delete; the AC4 macOS hand-test checklist is in the PR body.
