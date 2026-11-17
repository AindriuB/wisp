# 177 — Measure 0.4.0 and dry-run the release (3M)

**Repo:** .
**Round:** 0
**Depends on:** 166, 167, 168, 169, 170, 171, 172, 173, 174, 175, 176
**Owns:**
- scripts/measure/results/v0.4.0-wave-3-size.json (new)

## Goal
Prove the 0.4.0 head fits its budgets and that the release workflow produces all
three archives with `wisp-cli` inside, and hand the user a macOS hand-test
checklist. Tagging, the version bump and publishing stay with the user (D-I).

## Context
- docs/plan/specs/2026-11-17-wisp-0-4-0.md "Size gate" (:443-445), Waves 3M (:482), D-I (:17), Success (:9).
- Baselines: 165 (`v0.4.0-wave-1-size.json`) and 172 (`v0.4.0-wave-2-cli-size.json`); 0.3.0 Windows GUI 15,533,568 B. Gate 16,000,000 B, soft line 15,900,000 B; CLI limit per `scripts/measure/cli-budgets.json`.
- docs/plan/tasks/retired/158-measure-and-release-dry-run.md — the 0.3.0 version of this task (same procedure; its Outcome notes the BSD `wc` padding and the macOS CLI signature bytes).
- A draft release is outward-facing: dispatch `release.yml` only after the orchestrator confirms the user approved it. `measure.yml` needs no approval.
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 177 measure-and-release-dry-run`; no local build. Foreground commands only; no polling loop; never `pkill` or `killall`. Wait with one foreground `gh run watch <id> --exit-status`.

## Acceptance
- [ ] AC1 A `measure.yml` dispatch on main at the 0.4.0 head (all listed tasks merged) completes green on every target; run id and head commit recorded.
- [ ] AC2 The result file records per target GUI and CLI bytes, the 0.3.0 and 165 baselines and deltas, the Windows GUI against 16,000,000 B and 15,900,000 B, and each CLI against its limit. Over the soft line: the PR says so first and lists the spec's cut order (D3 Explain display, then A4 per-row masking) without cutting. Over the gate: stop and report.
- [ ] AC3 After approval, a `release.yml` dispatch with `draft_release=true` on the same head completes green; each archive lists `wisp-cli` (listing quoted in the PR) and `SHA256SUMS` lists exactly three `Wisp-*` files.
- [ ] AC4 The PR body carries a macOS hand-test checklist: `wisp-cli run` against a real API producing exits 0, 1, 4, 5, 6 and 7; `--json` output; `--keychain` on macOS (note any keychain prompt); `--history` then the entry in the GUI history list ("outcome unknown" chip for an exit-6 run); `snapshot` with `--request` and `--recent`; the Vars sub-tab (add, rename, mask, dirty dot, save); Explain shows description and expected status; the URL bar no longer changes on each keystroke for `{{$uuid}}`; a GUI send with a redirect and with Digest auth still works; the bundled `Wisp.app/Contents/MacOS/wisp-cli` passes `codesign --verify` and `spctl` with the app.
- [ ] AC5 No file other than the result file changes; the dry-run draft is left for the user to delete.

## Checks
- `measure-run` command — AC1
  run: `gh run view <measure-run-id> --json conclusion -q .conclusion | grep -x success`
- `release-run` command — AC3
  run: `gh run view <release-run-id> --json conclusion -q .conclusion | grep -x success`
- `only-result` command — AC5
  run: `test "$(git diff --name-only main...HEAD | grep -vx scripts/measure/results/v0.4.0-wave-3-size.json | wc -l | tr -d ' ')" = 0`
- `figures` read — AC2, AC3, AC4
  how: the bytes match the run logs; thresholds stated; listings and the checklist are in the PR.

## Out of scope
- The version bump to 0.4.0, the tag, publishing, deleting the draft (user); Windows and Ubuntu hand tests (D-I); any size cut.
