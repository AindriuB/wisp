# 65 — Enforce per-platform budgets in the measure scripts and measure.yml

**Repo:** `.`
**Depends on:** none
**Owns:**
- scripts/measure/budgets.json (new)
- scripts/measure/measure.sh
- scripts/measure/measure.ps1
- .github/workflows/measure.yml
- docs/plan/tasks/65-platform-budgets.md (the `## Notes` section only)

## Goal
The user set per-platform budgets on 2026-11-03 from the post-task-64 figures,
keeping both Linux backends (x11 and wayland). Each measure run must now compare
its sample with its own target's budget, write the verdict into the result JSON,
and fail the CI job when a measured target is over budget. The known
`x86_64-apple-darwin` runner hang must not turn the run red.

## Context
- docs/architecture.md:161-213 — the Budgets section. The post-task-64 figures
  (run 41631211960) are at :188-195; the current arm64-only "Set budgets" table
  is at :197-213.
- scripts/measure/measure.sh:63-90 — `write_json`, the python3 block that emits the
  result. `binary_bytes`, `idle_rss_kib`, `after_request_rss_kib` and
  `rss_anon_kib {idle, after_request}` are the inputs. :92-108 — `fail()`, the
  `unmeasured` path (exit 1). :197 — the `measured` write.
- scripts/measure/measure.ps1:54-68 — `Write-Result`; `private_kib {idle, after_request}`
  is the Windows input. :91 and :160 — the `unmeasured` writes. :151 — the `measured` write.
- .github/workflows/measure.yml:17-38 — the matrix and the job-level `continue-on-error: true`.
- The current figures, all under the budgets below: arm64 10,420,080 B, RSS 73,152 / 77,728 KiB;
  x64 12,884,616 B, no memory figures; Windows 13,450,240 B, private bytes about 35-42 MiB;
  Linux 23,353,896 B, RssAnon 58,168 / 44,972 KiB.

### The budgets (user decision, 2026-11-03)
1 MiB = 1,024 KiB. In KiB, 120 MiB = 122,880 and 128 MiB = 131,072.

| Target | `binary_bytes` | idle metric ≤ 122,880 KiB | after-request metric ≤ 131,072 KiB |
|---|---|---|---|
| `aarch64-apple-darwin` | ≤ 20,971,520 | `idle_rss_kib` | `after_request_rss_kib` |
| `x86_64-apple-darwin` | ≤ 20,971,520 | `idle_rss_kib` (not enforceable while the runner hangs) | `after_request_rss_kib` (same) |
| `x86_64-pc-windows-msvc` | ≤ 16,777,216 | `private_kib.idle` | `private_kib.after_request` |
| `x86_64-unknown-linux-gnu` | ≤ 29,360,128 | `rss_anon_kib.idle` | `rss_anon_kib.after_request` |

The Windows rule uses private bytes because on WARP the working set leaves out GPU
memory. The Linux rule uses RssAnon because llvmpipe dominates RSS and PSS in CI.

## Acceptance
- [ ] `scripts/measure/budgets.json` is the only place the figures above appear. It is
      keyed by target triple, and each of the four entries gives the three limits and
      the metric path each memory limit reads. Both scripts read it, from
      `$BUDGETS` when that is set and from the file next to the script otherwise.
- [ ] Every result JSON, `measured` or `unmeasured`, has a `budget` object with keys
      `binary_bytes`, `idle` and `after_request`. Each key is
      `{metric, limit, value, within}`: `metric` is the source field path (e.g.
      `"private_kib.idle"`), `within` is `value <= limit`, and both are `null` when
      `value` is `null`. The JSON also has `within_budget`: `false` if any `within`
      is false, `true` if all three are true, `null` otherwise. When the target has
      no entry, `budget` and `within_budget` are both `null`. No existing key changes.
- [ ] Exit codes in both scripts: 2 for usage errors, as now. 1 for `unmeasured`, as
      now, written with the `budget` object. 3 for `measured` with `within_budget`
      false, after the JSON is written and a one-line `over budget: <key> <value> >
      <limit>` per failing key on stderr. 0 otherwise.
- [ ] `measure.yml` sets `continue-on-error` per matrix row: `true` for
      `x86_64-apple-darwin` only, `false` for the other three. An over-budget or
      unmeasured result on arm64, Windows or Linux fails the run; a failing x64 job
      stays tolerated, as today. The rows' other steps, the build settings and the
      upload step are unchanged.
- [ ] A negative check, recorded in Notes with its output:
      `BUDGETS=<scratch file with the aarch64-apple-darwin binary limit 1>
      TARGET=aarch64-apple-darwin scripts/measure/measure.sh /bin/sleep <out>.json`
      exits 1 and writes `budget.binary_bytes.within: false` and
      `within_budget: false`. No cargo build is needed.
- [ ] `gh workflow run measure.yml --ref task/65-platform-budgets` has run, and Notes
      records its run id and head commit. In that run the arm64, Windows and Linux
      jobs conclude `success`, and their artefacts show `status: "measured"`,
      `within_budget: true` and the three `budget` values. The x64 job's result is
      recorded as found (likely failed or unmeasured, no memory figures). The run
      conclusion is `success`.

## Out of scope
- Writing the budgets into `docs/architecture.md` or the spec. Only `scribe` writes
  docs (CLAUDE.md rule 6). The spec (docs/plan/specs/2026-10-29-http-client-v1.md:216-217)
  already points to `docs/architecture.md` for budgets, so the spec needs no edit.
  At /record the scribe replaces the "Set budgets" table and its paragraph at
  docs/architecture.md:197-211 with the table above. It also names
  `scripts/measure/budgets.json` as the enforced copy, notes that x64 RSS is
  unmeasured (not a failure) while the runner hangs, and records the run id from Notes.
- Any build setting (`Cargo.toml`, `.cargo/config.toml`, Linux backends).
- Fixing or diagnosing the x86_64-apple-darwin hang, or the job-level failure that
  leaves it with no artefact.
- The committed records in `scripts/measure/results/`.
- Other workflows, or a trigger other than `workflow_dispatch` for `measure.yml`.
- Opening a PR or pushing to `main`. The PR is opened at /record.

## Notes

- Negative check (local): `BUDGETS=<scratch, aarch64 binary limit 1> TARGET=aarch64-apple-darwin
  SETTLE_SECONDS=1 scripts/measure/measure.sh /bin/sleep <out>.json` exited 1 (unmeasured) and wrote
  `budget.binary_bytes {limit 1, value 166512, within false}` and `within_budget: false`.
- CI run 41636483942, head commit 4ba28c1617f65b1643d4135aed7aac4a7477d192: run conclusion `success`.
  arm64, Windows and Linux jobs `success`, status `measured`, `within_budget: true`:
  - arm64: binary 10,420,080; idle 73,040; after 77,824 KiB.
  - Windows: binary 13,450,240; private_kib.idle 35,900; after 40,608.
  - Linux: binary 23,353,896; rss_anon_kib.idle 53,660; after 45,592.
  - x64: job `failure` (tolerated), result `unmeasured` (no 'WISP_EVENT completed' within 30 s); budget.binary_bytes 12,884,616 within, memory values null.

## Review round 1 — findings

Reviewed HEAD: 37d6170.

- [STYLE] scripts/measure/measure.ps1:1-24 — the header comment does not mention `BUDGETS`, `budgets.json`, the `budget`/`within_budget` keys or exit code 3. Match measure.sh:25-27's "Budgets: ... Exit codes: ..." paragraph, and keep the env-var list (:2-3) and exit behaviour (:17-18) complete.
