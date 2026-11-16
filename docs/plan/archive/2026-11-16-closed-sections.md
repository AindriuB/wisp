# Archived from PLAN.md on 2026-11-16

Closed sections moved verbatim when PLAN.md passed its 100 KB limit. Source: PLAN.md at b6e4671 (main): the v0.3.0 Wave 0, Wave 1 contracts, Wave 1 complete, H1 and Wave 2 bullets, and the "v0.1.0 is tagged" and "v0.1.1 wave" sections.

- **Wave 0 complete:** 129 (the three openapi wall-clock tests count work, `substitute` is linear; PR #78) and theme-palette (PR #79).
- **Wave 1 contracts merged (PRs #78-#84):** 130 explain core report, 131 assertions core and storage, 132 environment info and hint, 134 create collection and environment, 135 `wisp-cli` scaffold and `inspect`. Main CI green on all three platforms.
- **Wave 1 complete (PRs #86-#90, main CI green on all three platforms):** 133 `storage validate`, 136 measure wave 1, 137 history provenance fields, 138 `wisp-core` at opt-level "z". Windows GUI is now 15,179,264 B against the 15,500,000 B gate (headroom 320,736 B), so the roadmap's "raise the gate to 16 MB at 0.4.0 if headroom is under 100 KB" does not trigger on current numbers.

### v0.1.0 is tagged
Tagged 2026-11-03 before the Windows hand test; the macOS Apple-silicon candidate was hand-tested and passed. See HISTORY. The Windows build was hand-tested afterwards and passed.

### v0.1.1 wave
Patch release after v0.1.0. Done: tasks 86 (streamed File and Multipart bodies resent on 307/308), 84 (secrets redacted from Debug output in wisp-core), 85 (storage rejects Windows-unsafe and aliasing names), 83 (ask before discarding unsaved changes on quit and close), 88 (narrow-window layout limits and Cookies scrollbar) and 87 (Linux window icon, app id and desktop launcher; Linux hand checks still pending) and 89 (workspace version 0.1.1); see HISTORY. Remaining before tagging: a release-candidate dry run, the user's hand tests (the checklist in task 83's HISTORY entry), then tag and publish with the user's approval.

- **H1 history hardening merged (task 144, PRs #94, #103):** its open items are under "Follow-ups from task 144".
- **Wave 2 merged and recorded:** 139 UI seams (PR #97), 140 Explain pane and context strip (PR #99), 141 assertions editor and Checks pane (PR #100), 142 `wisp-cli validate` and `explain` (PR #95), 144 history hardening, 145 DNS classify unit test (PR #93), 146 README portable-mode sentence (PR #92), 150 SigV4 access key mask (PR #96), 151 Explain hidden-set consistency (PR #98), 143 save and close (PR #101), 152 history-tab sidecar overwrite (PR #102).
