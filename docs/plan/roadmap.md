# Wisp roadmap: 0.3.0, 0.4.0, 0.5.0

Agreed 2026-11-15 on main 7b756b9, from the 2026-11-14 analysis and the user decisions in "Decided" below. Effort and size figures other than the 0.3.0 spec's are GUESS.
The 0.3.0 scope is amended in `docs/plan/specs/2026-11-14-wisp-0-3-0.md` (Amendment 1).
Citations: audit sections are named by heading (the source audit, `PRODUCT-AX-AUDIT.md`, untracked and cited by name only);
code is `path:line`. Scale: effort S (<= 1 task), M (2-3), L (4-6), XL (7+).
Size risk is for the Windows GUI exe (gate 15,500,000 B; 0.3.0 spec estimates headroom ~318 KB from 15,181,824 B).
A separate binary (`wisp-cli`) does not count. The theme rework (branch task/theme-palette) is unmeasured: re-baseline before 0.3.0 wave 1M.

## Positioning

Headline: Wisp is the small, git-native HTTP client you can trust and explain. Agents are a side effect:
every agent-facing contract (report type, CLI JSON, snapshot) exists because a human needs it first
(user decision, 0.3.0 spec "User decisions"; agents are deferred, see Decided). No scripting, no accounts, no network service.

## The size problem (read first)

0.3.0's agreed GUI features cost +145 to +230 KB (spec "Size gate"), leaving ~90 to ~170 KB. Candidates for later
GUI work are the request-vars editor (+25-40 KB), response diff (+40-60 KB), GraphQL body mode (+20-35 KB), onboarding
actions (+10-20 KB). That does not fit under 15.5 MB. 0.4.0 is therefore built as a mostly non-GUI release
(service layer, CLI run, hardening are size-neutral or outside the gate). 0.5.0's GUI items need either a
gate decision (recommend raising to 16,000,000 B, still under the 16 MiB in `scripts/measure/budgets.json`) or a
paired size-reduction task. This is decision D-1 below (decided: hold the gate through 0.3.0, decide at 0.4.0 planning); it does not touch 0.3.0.

## Item register (every item: verdict, value, effort, size risk, deps, user decision)

### A. Audit UX/product items

| Item | Verdict | Value | Eff | Size risk | Depends | User decision |
|---|---|---|---|---|---|---|
| A1 Explain Request (audit §1) | KEEP 0.3.0 (agreed) | Answers "why this value/header/auth" before another send | L | Med +50-75 KB | none; defines `ResolutionReport` | none |
| A2 Pre-send context strip (audit "Specific UX gaps" 2; `workspace.rs:1104-1138`) | KEEP 0.3.0 (agreed, part of A1) | Shows env/auth/history scope before send | S | in A1 | A1 `explain_auth` | none |
| A3 Save-and-close (audit "Specific UX gaps" 3; `root.rs:50-62`) | KEEP 0.3.0 (agreed) | Removes the Discard/Cancel detour | M | Low +8-15 KB | task 128 checked save | none |
| A4 Per-request vars editor + `DirtySections.vars` (audit gaps 1; `draft.rs:5-16`) | KEEP 0.4.0 | Closes "request dirty with nothing highlighted"; `Request.vars` stops being file-only | M | Med +25-40 KB (reuse `ui/kv.rs`) | 0.3.0 must add `assertions` to `DirtySections` first (one frozen-type edit at a time) | none; gate decision D-1 |
| A5 Onboarding: New collection / New environment (audit "Primary journeys" first request; `sidebar.rs:881` is the only action; storage has no create-collection fn, only `save_environment` at `environment.rs:268`) | KEEP 0.3.0 (changes agreed scope, see D-A) | User-reported dead end: no in-app way to start | M | Low-Med +10-20 KB | one new storage fn (write `collection.toml`, format 1) | D-A (decided: in 0.3.0; sidebar empty state and palette; first cut if the soft line trips) |

### B. Docs

| Item | Verdict | Value | Eff | Size | Deps | Decision |
|---|---|---|---|---|---|---|
| B1 docs drift (audit "Architecture-to-UX alignment" tail) | DONE for 0.2.0 (PR #75); the 0.3.0 spec lists the remaining drift for the scribe | agents/readers trust the entry doc | S | none | none | none |
| B2 README macOS "no portable mode" mismatch (`README.md:84` vs `data_dir.rs:56`, `settings.rs:54`, which honour the marker on every OS) | KEEP 0.3.0 | Docs contradict code | S | none | none | D-D (decided: README fix only, no code change) |

### C. Platform / agent surfaces

| Item | Verdict | Value | Eff | Size | Deps | Decision |
|---|---|---|---|---|---|---|
| C1 Shared application-service layer out of `workspace.rs` (audit §2 "Mutation safety", "Architecture-to-UX alignment"; spec non-goals row 1) | KEEP 0.4.0 | One send/prepare/history path for GUI and CLI; subsumes AC52 | XL | Low (moves code; target net 0 to +10 KB) | 0.3.0 done (it edits the serial hot file `workspace.rs`, 4,822 lines) | D-E: accept a 0.4.0 whose headline is mostly internal, mitigated by `run` shipping on it |
| C2 CLI read-only (validate/inspect/explain) | KEEP 0.3.0 (agreed) | Terminal/CI/agent use of the same explanation | L | none (separate bin) | A1 report, `validate.rs` | none |
| C2b CLI `run` (audit §2 "Structured CLI") | KEEP 0.4.0 | Also closes README "command-line runner"; CI use with exit codes (0 ok, assertion fail, transport fail kept separate per audit) | L | none for GUI; CLI budget revisit (4 MiB limit, `reqwest` now linked for real) | C1, assertions, E3, E4; may need `lexopt` (spec: switch when subcommands > ~5) | D-F: keychain secrets in `run` (`--keychain` opt-in, no prompts by default) |
| C3 Safe redacted context snapshot (audit §3) | KEEP 0.4.0 as static export only: `wisp-cli snapshot` (persisted state, bounded, DTO-only). DEFER live IPC | Agent/human can paste accurate state; reuses `inspect` DTOs | S-M | none | A1 report type -> CLI explain -> snapshot; `inspect` DTOs | none |
| C3-live Live desktop state over IPC | DEFER. Trigger: an agent/user need to see unsaved drafts that file reads cannot answer, plus C1 done | n/a | L | Med (listener in GUI) | C1 | none |
| C4 Reviewable change sets plan/apply/undo (audit §4) | DEFER. Trigger: an MCP/edit tool is actually scheduled (change sets exist to serve agent writes) | Safe agent edits | L | Med if surfaced in GUI | C1, E1 (rename-safe refs), revision hashes | none |
| C5 env safety: per-user guard | KEEP 0.3.0 (agreed) | Accident guard on production | S-M | Low +6-12 KB | A2 strip | none |
| C5b agent approval/send policy (audit §5) | DEFER. Trigger: C6 scheduled | Scoped approval | L | Med | C1, C5 | none |
| C6 MCP adapter (audit §6) | DEFER. Trigger: C1+C4+C5b shipped, or users report wrapping the CLI JSON in their own MCP shim. Interim: document `wisp-cli --json` as the agent interface (a scribe doc, no code) | Direct tool use by agents | M | none if separate crate | C1, C2b, C3, C4, C5b | D-G |

### D. Evidence

| Item | Verdict | Value | Eff | Size | Deps | Decision |
|---|---|---|---|---|---|---|
| D1 History provenance (env name + assertion results; audit §7 "Implementation sketch") | KEEP 0.3.0 (agreed) | Reproducibility context | M | Low +4-8 KB | assertions | none |
| D2a Assertions `[[assert]]` | KEEP 0.3.0 (agreed) | Pass/fail without scripting | L | High +80-130 KB (largest, least certain) | none | none |
| D2b Response diff / run comparison (audit §7) | KEEP 0.5.0 | Compare two runs/envs without copy-paste; the main non-agent value in the audit | L | High +40-60 KB | D1, stored-body completeness flags, user-chosen ignore list | D-1, D-H (diff scope: JSON structural only) |
| D3 Request metadata (description, expected outcome, operation id; audit "Architecture-to-UX alignment") | KEEP 0.4.0, file + CLI + read-only in Explain; GUI editing DEFER. Only `description` and `expected_status` in 0.4.0; drop `operation id` (importers can put it in description) | Humans and agents see intent; Postman/OpenAPI import already has descriptions to land | S-M | Low +5-10 KB | additive `Request` field (do it with C1 to avoid a second frozen-type sweep) | none |

### E. Identity and execution safety

| Item | Verdict | Value | Eff | Size | Deps | Decision |
|---|---|---|---|---|---|---|
| E1 Durable request IDs (audit "Architecture-to-UX alignment"; spec non-goals) | DEFER. Trigger: change sets (C4) or import idempotency (E2) is scheduled; today history is path-keyed and rename already handled by scope | Rename-safe history | M | Low | format-additive `id` key | none |
| E2 Import idempotency (audit "Primary journeys" migration) | DEFER. Trigger: E1 shipped and a user reports re-import duplicates | Re-import updates instead of suffixing | L | Low | E1 | none |
| E3 Deterministic previews for dynamic values (audit "Important distinctions") | KEEP 0.4.0 | `run`, Explain and send agree: dynamic values generated once per run and reported | S-M | Low | C1 (one resolution point) | none |
| E4 Execution-safety semantics (`outcome_unknown`, no auto-retry of POST; audit §5 guardrails) | KEEP 0.4.0, scoped to `run` and history result vocabulary | CI and agents must not retry unsafe writes blindly | M | Low | C1, `run` | none |

### F. Non-audit work

| Item | Verdict | Value | Eff | Size | Deps | Decision |
|---|---|---|---|---|---|---|
| F1 Flaky wall-clock tests (34-19; `known-flakes.md:7-9`, `openapi.rs:621-749` use `Instant` asserts at 638, 678, 703, 749) | KEEP 0.3.0, first task before wave 1 (4 failing tests, windows CI leg red 3+ times) | CI trust | S | none | none | none |
| F2 Save racing the watcher (PLAN "Follow-ups from task 128") | KEEP 0.3.0; folded into 2E `save_request_if_unchanged` (spec feature 2a invariant 2) | data loss | M (in 2E) | in 2E | task 128 result | none |
| F3 Half-written external edit ignored on a clean tab (`draft.rs:76-80`: `Some(Err(_))` + not dirty -> Ignore) | KEEP 0.3.0 as a small separate task with F2 (a clean tab whose file turns unparseable should show the banner or be marked, so Save cannot overwrite) | data loss, same class as F2 | S | Low | 2E (same files) | none |
| F4 `mark_saved` does not reset `DeletedOnDisk` (`tab.rs:209-213`) | KEEP 0.3.0 with 2E (same file; Save-and-close depends on it) | correctness | S | none | 2E | none |
| F5 Windows/Ubuntu hand-test catch-up | REPLACED by mac-first release testing (D-I): tag after the macOS hand test; Windows and Ubuntu are tested later, may skip versions, and the release notes say which were tested. Not an exit criterion | Linux/Windows pass is unverified for 0.2.x UI changes; the notes say so | S | none | RC build | D-I |
| F6 Intel Mac build | DROP: macOS 27 is Apple-silicon-only and the x64 CI hang was never fixed (`2026-10-29-http-client-v1.md:23`); reopen only if the runner hang is fixed AND users ask | none | XL | n/a | n/a | none |

### G. README "still missing" (README line 17)

| Item | Verdict | Reason / trigger |
|---|---|---|
| Scripting | DROP. v1 table says None (`2026-10-29-http-client-v1.md:36`), conflicts with size and portability; assertions cover the common case. Revisit trigger would be "capture a response value into a variable" asked repeatedly: build that declaratively (a `[[capture]]` table), not an interpreter. DEFER the capture feature (L, High size). |
| GraphQL | KEEP 0.5.0 as a body mode (query + variables JSON, pretty-print, no introspection, no schema explorer). M. +20-35 KB. Value: parity feature users ask for first. Needs D-1. |
| Global variables | DEFER. Fights per-collection, git-reviewable data and the secrets model; 8 open collections is the pressure. Trigger: 2+ user requests for cross-collection values. Interim: collections already share `WISP_DATA_DIR`; no work. |
| CLI runner | KEEP 0.4.0 (C2b). |
| TCP/TLS timing split | DEFER. 0.2.0 shows DNS and connect+TLS together; a split needs a custom connector in `core` (reqwest does not expose phases). L, Med size. Trigger: a user debugging TLS latency who cannot use the existing split. |
| Postman script translation | KEEP 0.5.0 narrowed: translate only a closed set of `pm.test` / `pm.response.to.have.status` / header checks into `[[assert]]`; everything else keeps the existing unsupported-feature warning. M, storage-side so size ~+8-12 KB. Depends on assertions shipped. Full script translation: DROP. |
| Intel Mac build | DROP (F6). |
| "OAuth2 tokens never saved", "Windows exe not signed" | n/a: signing is H below. |

### H. Distribution (user-side, non-engineering)

| Item | Verdict | Notes |
|---|---|---|
| SignPath configuration (`scripts/release/sign-windows/README.md`) | KEEP, target prerequisite for the 0.3.0 tag so both exes are signed (D-C). Fallback if the user's application is still pending: ship unsigned with the notice. User action | D-C, D-J |
| winget submission (`packaging/winget/README.md`) | KEEP now: the user opens the winget PR (0.2.0 is published); confirm PackageIdentifier first. Second nested alias for `wisp-cli` is a 0.3.0 packaging task (3B) | D-J |
| Microsoft Store (`packaging/msix/STORE-SUBMISSION.md`) | DEFER until SignPath and winget are live. Store package excludes the CLI (spec "Release packaging"); trigger: a user who cannot install from zip/winget | D-J |

### I. Task 99 deferred hardening, grouped by theme (AC numbers from `99-hardening-0-2-0.md`; AC39 closed with 0.2.0)

| Theme | ACs | Verdict | Version | Eff |
|---|---|---|---|---|
| H1 History: data loss and secrets | AC50 (shared-id eviction deletes live bodies, :63), AC8, AC9 (:49-50), AC48 symlinked lock file (:61), AC17 lock timeout (:58); PLAN 62-1 (scrub old history on open) and 62-2 (Location URLs) | KEEP | 0.3.0, wave 2 after 1B (same file `history.rs`) | M |
| H2 History: robustness leftovers | AC49 (quadratic `mask_bare_refs`, :62), AC12 (uncapped req body, :53), AC15 folded into AC49; PLAN 62-n1/n2 | KEEP | 0.4.0 | S-M |
| H2-drop | AC11, AC13, AC14, AC20, AC24, AC25, AC37 (test-only or edge) | DROP as stand-alone items: fix opportunistically when the file is touched | n/a | n/a |
| H3 Secret store and keychain binding | AC53 (data loss), AC54, AC55, AC56, AC59, AC66, AC67, AC68, AC69, AC72, AC75 | KEEP | 0.4.0 | L |
| H3b binding-registry deep cases | AC60, AC62-AC65, AC70, AC71 | KEEP | 0.5.0 (non-GUI, no size cost); DEFER if keychain use by git-worktree users never surfaces | M |
| H4 Importers: credential leakage | AC19, AC42, AC43, AC26, AC22, AC36, AC32, AC33 | KEEP | 0.4.0 | M |
| H4b Importers: perf and shape | AC21, AC27, AC28, AC29, AC34, AC35 | KEEP | 0.5.0 with the Postman translation task (same files) | M |
| H5 Collection and data dir | AC4 (id race; matters once CLI `run` and GUI coexist), AC5, AC7 | KEEP | 0.4.0 | S |
| H6 Windows platform | AC77 (8.3 test silent), AC78 (`same_file_entry` no-op), AC76 (owner-only ACL on data dir) | KEEP AC77, AC78 in 0.4.0; AC76 in 0.4.0 (README already admits the gap; `README.md:84`) | 0.4.0 | M |
| H7 Engine | AC2 (`hash_file` reopens by path, ignores cancel) | KEEP | 0.4.0, with `run` (same send path) | S-M |
| H8 App/GUI | AC3 (SigV4 access key unmasked, `auth.rs:219`) | KEEP | 0.3.0, a one-line task | S |
| H9 Routing/cookies | AC52 (history routes by id re-read from disk), AC51 (cookie parse error text) | KEEP | 0.4.0, delivered by C1 (history recording moves) | S |
| Other PLAN follow-ups (winget/MSIX/SignPath lists, importer 60-x, task 116 list) | Triage once per version by the scribe; only 🔒 items are scheduled | KEEP 🔒 only | 0.4.0 / 0.5.0 | M |

## Versions

### 0.3.0 "Understand and trust a request" (scope agreed; additions marked +)

Theme: a developer can see where every value of a request came from, check the response against declared expectations,
confirm risky sends, and inspect collections read-only from a terminal.

Features: A1, A2, A3, C5 guard, D2a assertions, D1 provenance, C2 read-only CLI (agreed). Additions: + F1, + A5 onboarding,
+ F3/F4 with F2, + H1 history hardening, + H8, + B2.

Waves (from the spec, plus additions):
- Wave 0: F1 flake fix (S, before everything); decide and merge theme-palette first so new UI (2B, 2C, A5) uses theme tokens and 1M measures the post-theme head.
- Wave 1 contracts (parallel): 1A explain core, 1B assertions core/storage, 1C CLI storage, 1D CLI crate, 1M measure. + A5 storage fn (create collection) rides in 1C-adjacent disjoint file.
- Wave 2: 2A UI seams, then 2B/2C/2D/2E parallel; 2E includes F2/F3/F4; 2F wiring; + A5 app task (sidebar empty state, palette actions) on `sidebar.rs`/`palette.rs`; + H1 history hardening after 1B; 2M measure and hand test.
- Wave 3: 3A guard, 3B packaging, 3C docs (+ B2), 3M dry run.

Size plan: spec's +145-230 KB, plus A5 +10-20 KB = +155-250 KB vs ~318 KB headroom. Soft line 15,400,000 B and the cut order (suggest_confirm hint, Checks detail, shadow list) stand; add A5's palette-only variant (no sidebar buttons) as the first cut. CLI 4 MiB limit, measured in 1M.

Exit criteria: all items above work on macOS (hand-tested) and pass CI on Windows and Linux; GUI exe <= 15,500,000 B; CLI within budget; no secret value in any
report/CLI/history/log canary test; CI green including Windows; F1 gone from `known-flakes.md`; SignPath artifact configuration updated so both exes are signed (or, if the application is still pending, the unsigned notice accepted); macOS hand test incl. three-button prompts; macOS signed CLI verified; release notes state which platforms were hand-tested. No Windows or Ubuntu hand test is required before the tag.

### 0.4.0 "One engine, headless" 

Theme: the desktop app and the terminal share one send path, so a saved request can run in CI with the same result and a clear
outcome vocabulary.

Features: C1, C2b `run`, E3, E4, C3 static snapshot, A4 vars editor, D3 metadata, hardening H2/H3/H4/H5/H6/H7/H9.

Waves:
1. Service layer extraction (serial on `workspace.rs`; behaviour-preserving, ends with the existing suite green and a size measure showing net <= +10 KB). Includes AC52/AC51 and the dynamic-value-once rule (E3).
2. In parallel: `run` + E4 + AC2 (crates/cli and core engine); hardening storage themes (H2-H5), Windows themes (H6); D3 field (additive) + snapshot command.
3. A4 vars editor and D3 display (the only GUI work, `ui/` files, behind D-1); docs; measure; release dry run; hand test.

Size plan: gate holds at 15,500,000 B through 0.3.0; at 0.4.0 planning raise to 16,000,000 B only if 0.3.0 closes with under 100 KB headroom, otherwise keep and cut. GUI budget target <= +70 KB (A4 +25-40, D3 +5-10, C1 0..+10, rest storage at opt-level z). If 0.3.0 closes with < 100 KB headroom, A4 is cut to the palette-only edit or moves to 0.5.0 behind the gate decision. CLI: `run` links the network; raise its limit once measured (user approves).

Exit criteria: GUI and CLI send through the same service function (grep check: no second `Engine::send` caller outside it); `run` returns distinct exit codes for transport failure, assertion failure and HTTP status; `outcome_unknown` documented and tested; every 🔒 theme H1-H7 AC closed; CLI canary and read-only tests extended (run never logs a secret; `run` writes nothing outside history if enabled); macOS hand test (Windows and Ubuntu later, notes say which were tested); GUI size within gate or the 0.4.0-planning gate decision made.

### 0.5.0 "Compare and migrate"

Theme: compare responses across runs and environments, bring more of other tools' work in, and cover GraphQL.

Features: D2b response diff, GraphQL body mode, Postman test -> `[[assert]]` translation, H3b, H4b, 0.4.0 carry-overs. Optional stretch (only if time):
document `wisp-cli --json` as the agent interface (scribe doc, no code). Everything agent-write (C4, C5b, C6) stays deferred.

Waves: 1. history completeness flags (core/storage, ignore-list model); 2. in parallel diff UI, GraphQL editor, importer translation + hardening; 3. measure, docs, hand test.

Size plan: GUI +60-95 KB (diff +40-60, GraphQL +20-35). Needs D-1 resolved; otherwise one of the two is cut.

Exit criteria: diff states truncation/missing evidence explicitly and applies user ignore list; a GraphQL request round-trips to TOML; the importer translates the closed set and warns on the rest; size gate per D-1; macOS hand test (others as available, notes say which); 🔒 items for H3b/H4b closed.

## Dependency chain

- Theme-palette merge -> 0.3.0 wave 2 UI -> 1M measure baseline.
- Task 128 result -> 2E (checked save) -> F2/F3/F4 -> A3 Save-and-close -> A5/3A (shared `root.rs`, `url_bar.rs`).
- A1 `ResolutionReport` (core, no Serialize) -> CLI explain (DTO map) -> C3 snapshot (reuses inspect + explain DTOs) -> (deferred) MCP read tools.
- `[[assert]]` + D1 history fields -> `run` result vocabulary/exit codes (0.4.0) -> D2b response diff and Postman translation (0.5.0).
- A2/C5 guard (GUI-only, 0.3.0) -> C1 single send path -> C5b enforced policy (deferred).
- C1 -> {C2b `run`, E3, E4, C3-live, AC52} ; C1 -> C4 -> C6; E1 -> {C4, E2}.
- `DirtySections` frozen type: 0.3.0 adds `assertions`; A4 adds `vars` after (never concurrently).
- SignPath config -> signed `wisp-cli.exe`; winget identifier confirmed -> CLI alias in manifest -> Store (last).
- 0.3.0 CLI limit (4 MiB, `cli-budgets.json`) -> 0.4.0 `run` re-measure.

## Decided (2026-11-15 session, all settled)

Ids are kept so the register above still resolves.

### 0.3.0 scope
- D-A A5 New collection / New environment is in 0.3.0 (sidebar empty state and palette). It is the first cut if the size soft line trips.
- D-B F1 flake fix is wave 0, first. F2-F4 close the save data-loss gaps (save racing the watcher; a half-written outside edit never raising the banner) with the Save-and-close task 2E. H1 history hardening follows the assertions/history task 1B. H8 (AC3) is a one-line task.
- D-C SignPath set up is a target prerequisite for the 0.3.0 tag so both exes are signed. Fallback if the user's application is still pending: ship unsigned with the notice.
- D-D B2: the README says macOS uses `WISP_DATA_DIR` for portable data; the portable marker file is for Windows and Linux. Docs only, no code change.

### Direction
- D-E 0.4.0 leads with the service-layer extraction (behaviour-preserving), then `wisp-cli run` (distinct exit codes for transport failure, assertion failure and HTTP status; `outcome_unknown`; no auto-retry of POST), E3, C3 static redacted export, A4 variables editor, D3 metadata, hardening H2-H7 and H9.
- D-F `run` secrets: no prompt by default, `--keychain` opt-in.
- D-G Agent features (MCP, change sets, approvals) are deferred. `wisp-cli --json` is documented as the agent interface.
- D-H 0.5.0 response diff is structural JSON plus status/header diff with a user ignore list. Text diff is deferred.
- D-1 Size gate: hold 15,500,000 B through 0.3.0. At 0.4.0 planning raise it to 16,000,000 B only if 0.3.0 closes with under 100 KB headroom; otherwise keep it and cut.

### Drops and defers
- D-J Store submission is deferred until SignPath and winget are live. The user opens the winget PR now.
- D-K 0.5.0 adds GraphQL body mode and a narrow Postman `pm.test` to `[[assert]]` translation. General scripting and full Postman script translation are DROPPED.
- D-L Intel Mac is DROPPED. Deferred with triggers (see the register): global variables, TCP/TLS timing split, durable request IDs, import re-import updates, live IPC export, change sets, approvals, MCP, value capture.
- D-I Release testing is mac-first: tag and publish after the macOS hand test. Windows and Ubuntu are tested later, may skip versions, and the release notes state which platforms were tested. No exit criterion requires a Windows or Ubuntu hand test before a tag.

## Caveats

- Effort and size figures other than the 0.3.0 spec's are my estimates (GUESS); line-count heuristics from the spec (30-70 B/line).
- The theme-palette branch was not read; its size effect and conflict surface with new UI are unknown.
- Task 128's merged shape (F2) was not checked beyond PLAN's follow-up list.
- Not verified: winget two-portable-file schema, SignPath failure mode for a second pe-file (both flagged in the spec as GUESS).
