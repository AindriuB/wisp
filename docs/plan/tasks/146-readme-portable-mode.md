# 146 — Correct the README's portable-mode sentence (B2)

**Repo:** .
**Depends on:** none
**Executor:** `scribe` (docs only; no implementer, no code)
**Owns:**
- README.md (the "Where Wisp writes" paragraph, :84, only)

## Goal
The README says macOS has no portable mode, while the code honours the
`portable` marker on every OS. Per the user's decision D-D the code stays as it
is and the README tells macOS users to use `WISP_DATA_DIR` for portable data,
keeping the marker file as the Windows and Linux mechanism.

## Context
- docs/plan/roadmap.md register B2 and decision D-D
- README.md:84 the sentence "macOS has no portable mode; ..."
- docs/architecture.md:171-180 the settings and data-dir lookup order (`WISP_DATA_DIR` absolute override first)
- crates/storage/src/data_dir.rs:9 `OVERRIDE_ENV = "WISP_DATA_DIR"`; crates/app/src/settings.rs:51-89

## Acceptance
- [ ] AC1 README.md:84 no longer says macOS has no portable mode; it says that on macOS portable data is set with an absolute `WISP_DATA_DIR`, and that the `portable` marker file is the Windows and Linux mechanism.
- [ ] AC2 The sentence matches the lookup order in docs/architecture.md:171-180 (an absolute override wins; a relative or empty one is ignored).
- [ ] AC3 Only README.md changes, and only that paragraph.

## Checks
- `scope` command — AC3
  run: `git diff --name-only main...HEAD | grep -vx README.md | wc -l | grep -x 0`
- `wording` read — AC1, AC2
  how: the reviewer reads the changed paragraph against docs/architecture.md:171-180.

## Out of scope
- Any code change to the marker handling (D-D: none).
- The README CLI section and the "Unsaved changes" wording for Save and close (wave 3 docs, 3C).
