# Archived from PLAN.md on 2026-11-17

Closed entries moved verbatim when PLAN.md was pruned to open work (task 153). Source: PLAN.md at 55d55ba (main). Each group names the section it came from; the lines are unchanged. The first group holds the v0.3.0 section lines that were rewritten the same day to record the user decisions of 2026-11-17.

### v0.3.0 (in progress) bullets, superseded by the 2026-11-17 rewrite
Explain Request, Save and close with the environment guard, assertions with history provenance, and a read-only `wisp-cli`. Spec: `docs/plan/specs/2026-11-14-wisp-0-3-0.md` as amended by its Amendment 1; roadmap: `docs/plan/roadmap.md`.
- **Waves 0 and 1 complete (PRs #78-#90):** 129, theme-palette, 130-138; main CI green on all three platforms. Windows GUI 15,179,264 B against the 15,500,000 B gate (headroom 320,736 B), so the roadmap's "raise the gate to 16 MB at 0.4.0 if headroom is under 100 KB" does not trigger. Detail in `docs/plan/archive/2026-11-16-closed-sections.md`.
- **Wave 2 merged and recorded (PRs #92-#102):** 139-146, 150-152. Detail in `docs/plan/archive/2026-11-16-closed-sections.md` and HISTORY.
- **147 and 148 recorded (PRs #109 and #108):** assertion and history wiring, and the new collection and environment UI. Follow-ups are under "Follow-ups from task 147" and "Follow-ups from task 148".
- **149 measured wave 2 (PR open):** Windows GUI 15,494,656 B, 94,656 B over the 15.4 MB soft line and 5,344 B under the 15,500,000 B gate (wave-2 delta +315,392 B on 15,179,264 B). Nothing is cut. **Decision with the user, not yet made:** the cut order (148's sidebar buttons then the rest of 148; the `suggest_confirm` hint; Checks expected/actual detail, 141; Explain's shadowed-layer list, 140). Detail: `scripts/measure/results/wave-2-size.json`.
- **Next:** the user's cut-order decision, then the macOS hand test.

### v0.2.0 (published 2026-11-14) pointer section
### v0.2.0 (published 2026-11-14)
Closed. The wave summaries are in `docs/plan/archive/2026-11-15-closed-sections.md` and HISTORY.

### Closed items inside follow-up sections
- 54-CI (done, tasks 122/123): the Windows `cargo test` + clippy job exists.
- (d) CLOSED by PR #52: plain `load_environment` now ignores the secrets file in store modes (115 AC2 still covers the app wiring).
- [CLOSED by PR #49] [P3] `put` (manager.rs ~259) still holds a borrowed guard across save/forget; nothing cancels it today, but a future caller dropping a `put` future could reopen the overlap. Move an owned guard in, as `bearer` does.
- [CLOSED by PR #49] [P3] `bearer`'s inline spawn_blocking duplicates the JoinError message from `Inner::blocking` (manager.rs ~208); route both through one helper.
- [Note, 49-P3] On refresh grants a failed refresh save keeps the token in memory by design (user decision); no further action.
- [CLOSED by task 110] [P2 -> wave 3 keychain] Store calls run synchronously on engine workers (default 1 worker, max 2); a blocking keychain call (access prompt) stalls all engine I/O. Run store calls via spawn_blocking with the tokio store lock held across the await before a keychain store ships.
- [CLOSED by PR #49 round 2] Failed save during re-authorize -> forget path has no test: a failed save now returns before any forget.
- [P2] CI builds only ubuntu and macos; the Windows `File::lock` bug in PR #35 would have shipped. Add at least a Windows `cargo check`/test job (or a cross `cargo check --target x86_64-pc-windows-msvc`).

### Reqwest 0.13 follow-ups (task 50), closed by tasks 51 and 56
### Reqwest 0.13 follow-ups (task 50)
Optional, none blocking. Closed by 51: the runtime TLS test (`crates/core/tests/tls.rs`) and the `webpki-root-certs` `default-features = false` consistency fix. The `engine.rs:87`, `install_crypto_provider` and `https://localhost` residuals were closed by task 56 (see HISTORY).
Budgets after 50: binary 19,114,864 B of 20,971,520; idle RSS 97.2 MiB; after one request 102.3 MiB.

### Open budget and measurement questions: done or resolved entries
**Per-platform budgets (done, task 65):** set by the user on 2026-11-03 and enforced by `measure.yml` through `scripts/measure/budgets.json` (table in `docs/architecture.md#budgets`). Both Linux backends (x11 and wayland) are kept. Intel macOS was dropped from 0.1.0 on 2026-11-03 (task 71).
**06 (resolved):** the method and environment selects now show chevrons (task 37, hand-checked 2026-11-02). UI automation needs the window frontmost.

### Carry-forward: crates/app, hand-check and measurement records
**measure.sh (21):** the `WISP_EVENT` line was not separately confirmed in the output; status "measured" implies it.
- *Measurement:* superseded by the task 35 figures under M2 (the shell measured about 96 MiB idle and binary 15.3 MB when it landed).
- *Hand-checked in task 35:* ⌘O from A to B (with dirty tabs: Discard, Open panel) closes A's tabs and resets the environment; ToggleTheme and ToggleLayout from the palette write only `theme` and `split` to settings.toml; theme="system" follows macOS Light/Dark live. The dev build wrote `~/Library/Application Support/wisp/settings.toml` during the check.
- *Mislabelled autosend error (fixed in task 54):* the failure line now reads `WISP_EVENT failed kind=Resolve problems=<names>`; see HISTORY.
- *Hand-checked in task 35:* env switching, rename (file and tab renamed) and delete (prompt names the item; file and sidecar removed). Tree order was wrong and rename/delete left stale `order` names; fixed in task 39 at the storage layer, running-app check passed in task 41. Create and move were not separately reported.
- *Hand-checked in task 35:* URL and Params live sync, highlight, hover tooltip ("baseUrl = … · collection"), status colouring. Params checkbox resync while the URL is focused fixed in task 40, hand-checked in task 41 (typing `?a=1&b=2` with no caret jump; untick and re-tick while focused resync). Not reported: secret `••••` hover, Cancel while sending.
- *Hand-checked in task 35:* JSON sidecar edit and ⌘S writes the sidecar only; Bearer, Inherit, Bearer restores the token (masked, reveal toggle); "Inherits Bearer from collection" line; Format. Multipart Text plus File part send hand-checked in task 41 (file part sha256 matched).
- *Hand-checked in task 35:* all passed (`gu`, `theme`, `layout`, ⌘E with dev pre-selected, Escape, shortcuts afterwards). Focus loss after other unmounts fixed in task 38, hand-checked in task 41.
- *Hand-checked in task 35:* Pretty keeps key order; a 20.3 MB response shows Raw with Pretty disabled; two Set-Cookie rows; header cell click and ⌘C copy. Hand-checked in task 41: Raw prefetch while scrolling, over-limit Pretty tooltip, Save output sha256. The 4 MB Raw cap line was verified in task 42 (see below).
- *Raw scrollbar and jump keys (42, hand-checked 2026-11-02):* Raw and Pretty show a draggable vertical scrollbar; End/⌘↓/Ctrl+↓ load to the 4 MB cap and jump to the bottom, Home/⌘↑/Ctrl+↑ jump to the top (any modifier works with End/Home; keys are handled on the BodyPane focus handle, and with the URL input focused End moves its text cursor). The 4 MB cap line ("Showing the first 4 MB — Save to read the rest") is verified on a 250,000-line body.
- *Scrollbars (43-45, hand-checked 2026-11-02):* sidebar, response Headers and Cookies and KvTable now have draggable scrollbars. Budgets: combined binary 19,014,912 B. See the pane-scrollbar defects below.
- *Measurement:* binary 18,981,728 B (+16.5 KB over main), idle RSS about 97.3 MiB, after-request about 101.9 MiB.
- *Hand-checked in task 35:* select chevrons, checkbox ticks, eye, × and search icons render. Tip: launching the bare binary may not expose a window to computer use until `WISP_AUTOSEND_URL` is set; wrap it in a scratch `.app` bundle with a bundle id.
