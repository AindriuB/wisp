# Plan

What is open, in priority order. Only `scribe` edits this file.

Each item: one line of what, one line of why it matters, and its blocker if it
has one. When an item is in flight, mark it with its task ids. Delete items you
no longer want rather than letting them rot — a plan nobody trusts is worse
than no plan.

---

## Now

<!--
### <title>
<What it is.> <Why it matters now.>
**Blocked by:** <or omit>
**In flight:** 01, 02
-->

### v0.2.0
Importers, history, OAuth2/Digest/SigV4, optional keychain secrets, multiple collections, phase timings and a persisted cookie jar. Spec: `docs/plan/specs/2026-11-09-wisp-0-2-0.md`. Each wave ends with a measure run and fails if the Windows binary exceeds 15.5 MB.
- **Wave 1:** per-user data dir then history rewrite; importers (curl, Postman, OpenAPI); Digest and SigV4. **Complete (tasks 91-98); size gate passed, Windows 14,514,688 B of 15,500,000 (run 42823763145).** Done: 91 (dependencies and storage stubs), 92 (per-user data dir and collection ID), 93 (history rewrite into the per-user data dir), 94 (importers), 95 (Digest and SigV4 auth), 96 (Import dialog), 97 (history panel, Clear and Resend), 98 (size gate); see HISTORY. Wave 2 is next.
- **Wave 2:** **Complete (tasks 100-108); size gate passed, Windows 15,136,256 B of 15,500,000, headroom 363,744 B (run 42890358687).** OAuth2 (client credentials, auth code + PKCE); cookie jar; phase timings. Done: 100 (wave-2 surface types and seams: OAuth2 through model, resolve, codec and history redaction; timings and cookie-jar types, engine refuses OAuth2 until 103); 101 (settings follow WISP_DATA_DIR); 102 (persisted cookie file store in storage; app wiring is 107); 103 (OAuth2 engine: client credentials, auth code + PKCE, refresh); 106 (OAuth2 UI: editor, Authorize via the system browser, status strip); 104 (in-memory cookie jar per collection, bundled public-suffix list); 105 (phase timings: DNS, connect+TLS); 107 (cookie manager UI: per-collection persistence, Cookies panel); 108 (size gate); see HISTORY. Wave 3 is next, then task 99.
- **Wave 3:** multiple collections; keychain (Linux spike first). Done: 112 (up to 8 open collections, each with its own tree, watcher, environment, id, history and tabs; the open list is kept in settings.toml; palette Switch and Close; quit saves every jar; a same-id clone is refused); 113 (sidebar collection switcher with the active name, the open list with a check mark, path tooltips and × close through root.rs's shared close_collection_at); see HISTORY. Remaining: keychain.

### v0.1.0 is tagged
Tagged 2026-11-03 before the Windows hand test; the macOS Apple-silicon candidate was hand-tested and passed. See HISTORY. The Windows build was hand-tested afterwards and passed.

### HTTP client v1
Build the portable GPUI HTTP client in three milestones, in this order. Spec: `docs/plan/specs/2026-10-29-http-client-v1.md`. Product is Wisp; crate naming is settled in `docs/conventions.md#naming`.
- **M0 — Walking skeleton (complete).**
- **M1 — Core and storage (complete):** tasks 10-20 landed. The full model, `resolve`, body store, full event set and cancel, `toml_edit` storage with secrets and watcher, and the fixture integration test all pass without a window. See HISTORY.
- **M2 — UI v1 (complete):** tasks 21-37 landed (layout, tabs, dirty/save, env switcher, auth and body editors, response viewer, palette, settings, icons, integration and budgets). Window hand check passed on 2026-11-02; see HISTORY. Three defects were found there and all three are fixed and hand-checked in the running app (tasks 38, 39, 40, 41; see HISTORY).
**Budgets (task 35, commit add9842, macOS 27.0.1 arm64, hardware renderer):** binary 18,965,216 B of 20,971,520 (headroom about 2 MB); idle RSS 99,808 KiB (about 97.5 MiB) of 120 MiB; after one request 104,784 KiB (about 102.3 MiB) of 128 MiB. Measured with a 64 KB response; RSS was about 127 MiB with a 20 MB response loaded.
- **M3 — v1 hardening (complete):** tasks 52-61 landed (request sub-tab scrolling, autosend resolve error, Cookies and failed-status fit, tab close button, engine roots and TLS cleanup, storage hardening, history and environment fixes, streamed File and Multipart bodies, integration and budgets); see HISTORY. Task 61 added a 720×480 minimum window and fitted the URL row and status lines to narrow panes; its hand check also closed the task 53 failed-line check.
**Budgets (task 60, commit f116a69, macOS 27.0.1 arm64, hardware renderer):** binary 19,181,072 B of 20,971,520; idle RSS 70,496 KiB (68.8 MiB) of 120 MiB; after one request 75,648 KiB (73.9 MiB) of 128 MiB. RSS fell about 28.4 MiB since task 50 although the binary grew; the macOS version changed between the runs, which likely explains it, so do not read it as a broken measurement.
**Residuals (53):** at 600 px the "Expires / Max-Age" header and typical dates are cut (accepted); below 584 px the rows' vertical scrollbar starts off-screen until you scroll sideways; the Flags width (256 px, `FLAGS_TEXT_PX` 250) was measured against the macOS system font only.
**Residuals (55):** closing a dirty tab had no prompt; fixed by task 83.
**Residuals (54):** a `,` or `)` in a variable name can make the autosend `problems=` list ambiguous (nothing parses past the prefix); if autosend ever runs after an environment is selected, an unresolved `{{x}}` inside a secret's value would print `x`.
**Residuals (57):** a streamed body over 1 MiB is now resent on 307/308 (task 86); PUT/PATCH on 301/302 still shows the 3xx as-is. Special files (/proc entries reporting size 0, FIFOs, /dev/stdin) are sent empty or refused with "not a regular file"; a read-to-EOF fallback was rejected by the user. Bytes bodies still pass user Content-Length/Transfer-Encoding through. No test checks the 64 KiB chunk limit. `block_in_place` runs on every response chunk. The 20 MiB tests compare bytes but do not measure client memory.
**Residuals (56):** the `install_crypto_provider` unit test does not exercise the Err branch.
**Residuals (59):** a true swap (one var moving each way in the same save) cannot be made safe with two writes and is accepted; `gains_moved_name` treats an unparseable other file as empty (harmless while `render` parses first); the files are re-read after `render`; a dangling non-.toml symlink logs a skip warning.
**Residuals (58):** rejecting a symlink diamond takes about 7-10 s before the error; a Windows trailing-dot or trailing-space `.toml` alias passes (`request.rs:221`); the name check at `request.rs:224-226` is unreachable.

**Budgets (task 61, macOS arm64, hardware renderer):** binary 19,181,072 B (+0); idle RSS 98.7 MiB of 120 MiB; after one request 103.6 MiB of 128 MiB. RSS is about 30 MiB above task 60's figures; the change cannot explain it, so it is probably run-to-run noise. Re-measure main on the same machine to confirm the baseline.
**Residuals (61):** at 720 px wide the sidebar can still take 480 px and panes can be dragged to 100 px (clipping covers it); the minimum size was checked on macOS arm64 only. Do not re-add `overflow_hidden` to the `resizable_panel()` calls: it clips the splitter's 9 px grab band to 5 px.

**Residuals (72):** the Windows archive ships LICENSE, NOTICE and THIRD-PARTY-NOTICES with CRLF endings (Windows checkout); a `.gitattributes` `eol=lf` would fix it. Re-running a failed tag-push run creates a second draft for the same tag; delete the stale one before publishing. The tag-push path has not been exercised. The packaged `Wisp.app` launched with a window but nobody has looked at it; user hand test pending at release.

**Residuals (70):** the Bundled assets header in `scripts/notices/generate.sh:37` hard-codes "19 SVG icons" and the gpui-kit-assets and gpui-component versions, so it can go stale without the CI `notices` job noticing; `about.toml` accepts OpenSSL and BSL-1.0 though nothing uses them (OpenSSL contradicts the rustls-only rule in `docs/conventions.md#dependencies`).

**Residuals (67, optional P3):** `safe_sidecar` (`ops.rs:416-420`) still compares with `to_lowercase` rather than `fold_case`; it errs toward keeping a sidecar, so no data is lost. Task 67's head has not run on Linux; its own PR CI is the first run.

**Residuals (66):** the "Request failed" fallback heading is written in three places (`mod.rs:50`, `mod.rs:167`, `body.rs:727-729`); a shared helper would keep them in step. The macOS arm64 hand check of the error panel passed on 2026-11-03 (with task 69's sub-tab scroll check); only the Windows and Linux hand checks remain for the user.

**Residuals (62):** (P3) `clean()` (`ops.rs:83-87`) rejects a leading dot or `environments` with `to_lowercase`, while the new check in `clean_node` (`ops.rs:172-177`) uses `fold_case`. Paths through `clean()` only (move destinations, create targets) still use plain lowercasing; on Linux `move_node(r, "a.toml", "environment\u{17f}")` gives Io NotFound while `delete_node` of the same name gives Invalid. Fix: switch `clean()` to `fold_case` and drop the duplicate block; candidate for the next polish wave. The comment at `ops.rs:172` reads as if that block were the Linux fix, but the `collection.toml`/`folder.toml` check was. `delete_refuses_wide_case_fold_alias` (`ops.rs:1397-1400`) could now assert Invalid unconditionally. No test covers a non-existent reserved source spelling for rename or move (only delete).

### v0.1.1 wave
Patch release after v0.1.0. Done: tasks 86 (streamed File and Multipart bodies resent on 307/308), 84 (secrets redacted from Debug output in wisp-core), 85 (storage rejects Windows-unsafe and aliasing names), 83 (ask before discarding unsaved changes on quit and close), 88 (narrow-window layout limits and Cookies scrollbar) and 87 (Linux window icon, app id and desktop launcher; Linux hand checks still pending) and 89 (workspace version 0.1.1); see HISTORY. Remaining before tagging: a release-candidate dry run, the user's hand tests (the checklist in task 83's HISTORY entry), then tag and publish with the user's approval.

### Storage-safety follow-ups (from task 85, not v0.1.1)
- The Windows test `sidecar_rejects_8dot3_short_name_of_sibling_toml` fails rather than skips on a volume with 8.3 names off; it should return early when `B~1.TOM` does not exist.
- No test fails if the symlink-to-`.toml` check in `sidecar_path` is disabled; Unix is covered by the dev/ino check.
- The reserved-name rule also rejects display names like "CON.txt" whose file slug would be harmless (decided).
- A folder containing `con.toml` can still be moved, while moving `con.toml` itself is refused.

### Follow-ups from task 113 (not 0.2.0 wave 3)
- collections.rs:66-72 re-derives names and walks collection_root(i) instead of using OpenCollection::name() and Workspace::collections; drift risk.
- collections.rs list_open stays true when the count drops to 1, so the list reappears expanded when a second collection opens.
- The duplicate-name test only checks that paths pass through.

### Follow-ups from task 112 (not 0.2.0 wave 3)
- Settings write on activate is now synchronous file I/O on the UI thread; move persist_collections off-thread and coalesced, or skip when unchanged.
- Startup restore scans up to 8 trees synchronously.
- AC8 clone refusal closes the refused collection's tabs without a dirty check, so a tab edited before the id lands is discarded silently.
- The history test covers history_target but not that append_history passes sent.root.
- Clone refusal makes the last collection active rather than the previously active one.
- Collection paths are stored with to_string_lossy; a non-UTF-8 path on Linux is dropped on restart.
- set_cookie_persistence reads the active collection's id inside its callback, so switching while an id is being created could apply to the wrong collection.
- measure.ps1 changes are unrun and unlinted until the first Windows measure run.
- The refused-argv-path active fallback has no test.
- 50-4 `quit-check.sh:53` still runs Wisp on the real data dir (now restores and rewrites the collection list); use a per-launch temp WISP_DATA_DIR as in measure.sh.
- 50-5 Palette Switch/Close resolve-by-root is untested; add a pure helper test with shifted indices.
- 50-6 `settings.rs:149` corrupt `open_collections`/`active_collection` is untested, and one bad element drops the whole list; keep valid strings and test each key.
- 50-7 `settings.rs:574` restore dedup branch is untested; duplicate missing paths are reported twice.
- 50-8 (task 99, AC52) `workspace.rs:527` history re-reads the collection id from disk while cookies and tokens use the cached id; an on-disk id change routes history into another collection's store. Build HistoryJob from the known id.
- 50-9 Scratch tabs become unreachable once a collection opens but still count as dirty at quit.
- 50-10 Restore silently forgets a remembered collection that exists but fails to open.
- 50-11 A settings-save failure during restore is emitted before the window subscribes.
- 50-12 Jar changes after the close-time flush are never persisted.
- 50-13 Watcher batches from background collections rescan trees on the UI thread.
- 50-14 `release_tab` leaves id waits; a closed tab can still send or authorize under the wrong partition (relates to 47-4).
- 50-15 A clone whose id lands first is briefly live, and its refusal can save the shared jar.
- 50-16 (nit) `measure.ps1:117` fail path kills the app without WaitForExit, so temp dir removal can fail.
- 50-17 (nit) Palette collections mode does not preselect the active collection.
- 50-18 (nit) SwitchCollection has no root handler.
- 50-19 (nit) Repeated tab-to-collection lookups in send/authorize; use one helper.
- 50-R1 Residual, accepted by the user: close-collection and clone-clash decisions are tested via plan_clash/plan_close/confirmed_open, but the dispatch in close_collection and the clash handler is not (mutations there pass every test); no GPUI test harness exists.
- 50-R2 Add a GPUI test harness (gpui TestAppContext) so Workspace flows get end-to-end tests.
- 50-P3a `sidebar.rs:529-535` after a confirmed delete in a non-active collection, collapsed/selected are trimmed by relative path in the shown collection (cosmetic).
- 50-P3b `import.rs:610` new test sits before the module's use lines (idiom nit).

### Follow-ups from task 86 (not for 0.1.1)
- An API key sent in a custom header (for example `X-Api-Key`) still crosses origins on a redirect; only the headers reqwest strips are removed on a streamed resend.
- The streamed resend checks only each file's length, not its contents, so a file edited to the same size between sends is resent with the new bytes.
- `EngineError` messages include the request URL. Fix with `reqwest::Error::without_url()` so a URL carrying a secret does not reach the UI.

### Follow-ups from task 107 (not 0.2.0 wave 2)
- [ACCEPTED residual, user 2026-11-10] AC10: copies/clones of a collection (same id) share history, OAuth2 tokens, the in-memory jar and cookies/<id>.json.
- [Residual -> task 99] AC11: CookieFile::load symlink check-then-open race and a symlinked cookies/ parent (data dir is per-user 0700).
- [P3] Quit only saves the open collection's snapshot; jobs still queued (Disable/Enable/Save from a just-switched collection) are dropped at quit; window is one file write.
- [P3] Load-merge reads snapshot then load()s (two jar locks); a Set-Cookie between them is lost; needs an atomic merge in core CookieJar.
- [P3] A single failed save pauses saving for that collection until toggled off/on; consider auto-retry on the next change.
- [P3] Cookies set by a request still in flight for the old collection after a switch go to the old jar and are not saved this run unless reopened.
- [P3] Tests: `cancel_drops_a_send_waiting_for_the_id` does not exercise cancel(); `quit_save_only_when_saving_and_dirty` guards nothing in take_quit_save.
- [P3] WISP_AUTOSEND_URL with a collection path at launch: send returns Ok(None) while the id is read, so no WISP_EVENT line (measure.sh does not use a collection).
- [Low] 47-3: cookie panel rebuilds rows only on jar generation change, so expiry labels go stale and expired cookies stay listed (crates/app/src/ui/cookie_jar.rs ~215); rebuild on time too.
- [Low] 47-4: close_tab doesn't drop the tab's id_waits entry, so a pending Send can still go out (workspace.rs ~1235).
- [Low -> task 99] 47-5: serde parse errors from a corrupt cookie file can echo file content into the log and notice (workspace.rs ~2225); map to a fixed message with line/column.
- [Low] 47-6: loading bumps jar generation, so each persisted collection rewrites its cookie file 2 s after opening (workspace.rs ~2311); reset debounce to post-load generation.
- [Low] 47-7: Enable failure leaves no persistent banner in the panel (AC5) (workspace.rs ~2234).
- [P3] 47-P3: a cookie save/delete job already running (not queued) at quit isn't ordered against the quit future; relies on gpui's 200 ms shutdown wait.
- [NOTE] A running Save and the quit save both take WRITE_LOCK; an older snapshot could in theory overwrite a newer one.

### Follow-ups from task 105 (not 0.2.0 wave 2)
- [P3] tests/timings.rs AC6 cancel test can't detect a leftover registry entry (second request uses a fresh port/token); add a cfg(test) registry length check.
- [P3] h2 test asserts `accepted >= 1` (concurrent sends open extra connections before the first is established); a warm-up send would allow `== 1`.
- [P3] 46-1 Cargo.toml:38 new deps tower-layer, tower-service, http, h2, bytes omit `default-features = false` (conventions: Dependencies).
- [P3] 46-2 tests/timings.rs:81 the concurrent test blocks on a Barrier with no timeout (hangs instead of failing); wrap in a 10 s timeout naming the accepted count.
- [P3] 46-3 tests/timings.rs:110 keep-alive tests assume the pooled connection is idle before the next connect wins (small race).
- [P3] 46-4 tests/timings.rs:163 the DNS-error test depends on the host resolver failing `.invalid` (NXDOMAIN rewriting breaks it); test classification with a failing test resolver.
- [Nit] 46-5 timing.rs:24 the Timings doc doesn't say what phases mean through a proxy.
- [NOTE] Registry MAX_ENTRIES=64 FIFO: >64 unclaimed concurrent connects could evict a real entry (reported None); unreachable today (no runner/bulk send). Intermediate redirect hops and lost-race connects are never claimed and rely on the cap.
- [NOTE] With reqwest-followed redirects the timings describe the final connection; with the engine's own loop, the first hop that opened a connection.

### Follow-ups from task 104 (not 0.2.0 wave 2)
- [P3] cookies.rs: a Set-Cookie with UTF-8 values is silently dropped (`to_str`), and one non-ASCII value drops the whole Cookie header; use `from_utf8` / `HeaderValue::from_bytes` like reqwest's Jar.
- [P3] The generation bumps on unchanged load/clear and on a rejected Set-Cookie; task 107 may save more often than needed.
- [P3] `load` truncates remaining lifetime to whole seconds; a record expiring less than 1 s after now is stored expired.
- [P3] Each jar clones the parsed public-suffix list (a hash-map clone per jar/load).
- [NOTE] cookie_store logs non-Secure cookie values via `log::debug!`; no log backend exists today, so it would leak if a log/tracing-log bridge is ever added at debug level.
- [NOTE] Loopback hosts count as a secure context (Secure cookies are sent over http://localhost), matching browsers.
- [P2 -> task 108 / release] The public-suffix list snapshot (crates/core/data/public_suffix_list.dat, 2026-11-09) ages; refresh it before each release.
- [P3 -> task 99] 45-9 cookies.rs `load()` accepts ';', CR/LF and userinfo hosts in records; reject invalid cookie-octets and host mismatch.
- [P3 -> task 99] 45-10 plain-http responses can set or overwrite Secure cookies; `__Host-`/`__Secure-` prefixes are not enforced.
- [P3 -> task 99] 45-11 unlisted single-label suffixes (test, local) are accepted as Domain from responses (load rejects them); apply `is_registrable` to responses too.
- [P3] 45-6 `publicsuffix` is declared outside workspace deps; IDN suffix rejection relies on punycode enabled only via reqwest; add an IDN suffix test.
- [P3] 45-8 the PSL parse `expect()` has no invariant comment; a bad refresh aborts on first send; add a dedicated parse test.
- [P3] 45-12 Cookie header order is arbitrary, not longest path first.
- [P3] 45-13 generate.sh notice URL serves the latest list, not this snapshot; version is hard-coded twice; read VERSION/COMMIT from the file and add a permalink.
- [P3] 45-16 every Set-Cookie response scans the whole jar under the mutex (`needs_eviction`); keep running counts; the "no allocation" doc comment is wrong; the 3100-domain test takes about 5-7 s in debug.
- [P3] 45-14 (Nit) `PARSE_COUNT` ships in release (`cfg(test)` it); sync tests are marked async. Item 42-8 (jar `debug_redacts` test) was done in task 104 and is retired.

### Follow-ups from task 106 (not 0.2.0 wave 2)
- [P3] auth.rs:156 `parse_port` silently turns an invalid redirect port (81234, 8l23) into 0 ("any free port"); reject or show a notice.
- [P3] workspace.rs:553 Authorize on a history tab with a redacted body shows "Cannot authorize: Syntax(Body)" without Send's explanation; reuse `refused_body_message`.
- [P3] workspace.rs:490 vs :1074 the collection partition is set in two places (`authorize_command` and `send`); use one helper.
- [P3] The Authorize button and status strip sit at the bottom of the scrolling auth form, below the response split on a normal window, so "Authorized"/"Failed" can be off-screen; keep the strip visible.
- [P3 -> task 99] 44-1 auth.rs:329 a literal client_secret can be saved into committed request TOML with no hint; offer placeholder `{{client_secret}}` or a muted hint when no `{{` is present.
- [P3 -> task 99] 44-4 tab.rs:82 AuthorizeState derives Debug over the URL holding OAuth state; hand-write a redacting Debug, add a `debug_redacts` test, fix the doc comment.
- [P3] 44-2 auth.rs:603 changing auth type while Waiting hides Cancel; the listener stays bound until timeout.
- [P3] 44-3 auth.rs:772 tests: the secret-slot test doesn't check masking; round-trip tests duplicate each other and skip the client_auth label mapping.
- [P3] 44-5 workspace.rs:568 "Cancelled" is detected by matching the engine message string; use a shared const or ErrorKind::Cancelled.
- [P3] 44-6 workspace.rs:873 "Authorized" survives an environment switch though tokens are keyed by environment; also survives edits to client id, scope or URLs (earlier NOTE). Clear or recheck on change.
- [P3] 44-7 workspace.rs:1189 Cancel leaves the tab Waiting, so a queued AuthUrl can still open the browser.
- [P3] 44-8 workspace.rs:2357 AC8 test builds the expected Send without tree or env, so collection client settings aren't compared.
- [P3] 44-9 (Nit) auth.rs:332 the shown authorization URL is editable but never read back; make it read-only.
- [P3] 44-10 (Nit) workspace.rs:1176 the re-authorize cancel branch is unreachable from the UI and untested.
- [NOTE] Authorize resolves the whole request (same path as Send), so an unresolved variable in the URL or body also blocks it; the button appears only for the request's own auth-code auth, not inherited auth.

### Follow-ups from task 103 (not 0.2.0 wave 2)
- [ACCEPTED residual, user 2026-11-10] After a re-authorize, a send that read the old refresh token just before it can still POST that RT to the IdP once; the result is discarded (generation check) and nothing old is used or persisted. Closing it means holding the store lock across the token POST.
- [P2 -> wave 3 keychain] Store calls run synchronously on engine workers (default 1 worker, max 2); a blocking keychain call (access prompt) stalls all engine I/O. Run store calls via spawn_blocking with the tokio store lock held across the await before a keychain store ships.
- [P3] Failed save during re-authorize -> forget path has no test (needs a store whose save can fail after the first authorize).
- [P3] The stale marker is in-memory only: a failed forget/save leaves the old RT in the store after restart (wave 3).
- [P3] engine.rs:972-975 a 401 for a send that started with AT_X, arriving after a re-authorize, invalidates the new AT_Y; invalidate only when the cached token equals the one sent.
- [P3] No single-flight on refresh; concurrent refreshes may get a rotating RT family revoked.
- [P3] Token fetch uses the full request timeout before `began`, so a send can take about 2x timeout.
- [P3] tests/oauth2.rs:1348 orders the paused-save test with a 700 ms sleep, not a state wait.
- [NOTE] TokenKey excludes client_secret: after editing the secret, a cached client-credentials token is reused until expiry.
- [NOTE] Public `Clock` trait and `Engine::start_with_clock` were added for expiry tests.
- [P3 -> task 99] 43-4 manager.rs:291 token endpoint inherits the request's verify_tls=false, so secrets go to the IdP unverified; always verify TLS for token/auth endpoints, or show it.
- [P3 -> task 99] 43-5 loopback.rs:51 listener spawns unbounded connection tasks (now also kept listening after ignored callbacks); cap in-flight connections (~16).
- [P3 -> task 99] 43-6 token.rs:258 error-body redaction covers only three token keys; add client_secret, code, code_verifier, camelCase; scrub sent secrets.
- [P3] 43-7 token.rs:225 negative expires_in means never expires; 0-30 s tokens never reused; treat negative as zero; margin min(30 s, lifetime/2).
- [P3] 43-8 manager.rs:305 memory path reads epoch and refresh token under separate locks; can burn a fresh RT.
- [P3] 43-9 manager.rs:333 a rejected refresh token (invalid_grant) is kept and re-posted every send; forget it under the generation check.
- [P3] 43-10 engine.rs:186 Cancel and AuthorizeCancel share one map and can cancel each other's operations.
- [P3] 43-12 tests: Basic client auth never tested with reserved characters (a:b c&d+e).
- [P3] 43-13 tests/oauth2.rs:740 oversized-head test uses a 1 s wall-clock timeout; widen to ~3 s.
- [P3] 43-14 tests/oauth2.rs:845 port-closed assert can race parallel tests reusing the port.
- [P3] 43-15 (Nit) pkce.rs:10 random bytes from UUIDv4; use getrandom directly; rename the state test.
- [P3] 43-16 (Nit) token.rs:155 clone_without_url doesn't clone; rename.
- [P3] 43-17 (Nit) storage tests/history.rs:656 test name says var refs but uses literals.
- [P3] 43-18 (Nit) manager.rs:134 per-key generation and store_locks maps never pruned.
- [P3] loopback.rs:71 a callback with a non-matching state (e.g. a stale browser tab) gets the "You can close this tab" success page while Wisp keeps waiting and later times out; show a neutral "this sign-in link has expired, try Authorize again" page instead.
- [P3] 43-19 manager.rs:470: an IdP error callback sent without `state` is now ignored, so the user waits the full 120 s authorize timeout with no message; show the ignored-callback count (or "a sign-in error was received without a matching state") in the timeout error.

### Follow-ups from task 102 (not 0.2.0 wave 2)
- [P3] cookies.rs:202-203 `save` doc doesn't mention the domain/path caps or soonest-expiring trimming.
- [P3] cookies.rs:85-88 MAX_DOMAIN_BYTES/MAX_PATH_BYTES are declared after `acceptable`; group them with the limits at :20-24.
- [P3] cookies.rs:171 Parse error uses serde_json's message, which can echo a mistyped scalar from a hand-edited file (array-form entry); use a fixed message.
- [P3] cookies.rs:224 with more than 3,000 input cookies, the first 3,000 in input order are kept before expiry trimming; keep the latest-expiring.
- [NOTE for task 107] `save` checks `is_enabled` then writes with no lock, so a concurrent `disable` can re-enable persistence; `load` checks the symlink then opens (race) and does not refuse a symlinked `cookies/` parent.
- [P3 → task 99] 41-3 cookies.rs:207: symlink check then write through a path that follows symlinks; use a non-following write path.
- [P3 → task 107] 41-4 cookies.rs:183: load accepts any domain scope and control characters from the file; validate here or in the jar.
- [ACCEPTED, user 2026-11-10] 41-5 / 42-12: copies or clones of a collection (same id) share history, OAuth2 tokens, the in-memory jar and the cookie file.
- [P3] 41-6 cookies.rs:266: no fsync before rename; a crash can leave a truncated file; sync_all before rename and define recovery on Parse.
- [P3] 41-7 tests: no test that enable keeps cookies or refuses a symlink; error test only checks is_err.
- [P3] The in-process WRITE_LOCK mutex is not covered by a deterministic test (the file lock is).

### Follow-ups from task 100 (not 0.2.0 wave 2)
- [P3] No app test drives `Event::Completed` with non-empty timings through on_event/append_history, or the ignored auth events; task 105 has made timings real.
- [P3] No test that commit/save leave an OAuth2 draft untouched in the auth editor; add before task 106 reworks it.
- [NOTE] Literal OAuth2 `client_secret` is written to committed request TOML, like Digest password and SigV4 secret_key; values are normally `{{vars}}` (spec :58).
- [NOTE] Until task 106, the auth type list doesn't offer OAuth2, so switching an OAuth2 request away can't be undone in the UI.
- [NOTE] Task 100 added no deps; 103/104 bring serde_json/getrandom/tokio net and cookie crates and must recheck the lockfile rule and size.
- [P3 → task 103] 42-3 core auth/oauth2.rs:26: RefreshTokenStore is sync with no Result; keychain failures would be lost and prompts would block the engine. Return Result; document spawn_blocking use.
- [P3 → task 103] 42-7 storage history.rs:266: history empties non-secret OAuth2 fields (token/auth URLs, client id, scope), so Resend loses them; redact only client_secret.
- [P3] 42-4 app ui/request/auth.rs:123: build_auth maps OAuth2 kinds to Inherit, guarded only by an early return elsewhere.
- [P3] 42-5 storage codec.rs:439: redirect_port read error and stale-key removal are untested.
- [P3] 42-6 core engine.rs:459: serde round-trip test omits AuthorizeStart and AuthorizeCancel.
- [P3] 42-9 (Nit) model.rs:167: AuthorizeStart timeout None has no stated meaning; document as the 120 s default.
- [P3] 42-10 (Nit) app workspace.rs:1135: inner auth-event arm is unreachable.
- [NOTE] Round-1 fix for PR #42: TokenKey now carries collection id and environment; each collection has its own client and CookieJar (ClientSettings.collection); EngineConfig.cookies was removed. Task 106 must pass `environment` and `client.collection` on AuthorizeStart.

### Follow-ups from task 97 (not 0.2.0 wave 1)
- [P3] workspace.rs:1006-1018: Save on a from-history tab does not clear `from_history`, so later sends of the saved request still show the "Resent..." notice.
- [P3] core body.rs:206: a borrowed history body deleted by Clear or eviction while its tab is open reads as "unknown body handle: BodyId(n)"; show "history body was removed".
- [NOTE] Resend tests exercise the scope helper, not `Workspace::send` (no gpui test harness).
- [P3] 38-5 workspace.rs:647: storing a resend loses the original redacted list, so a later resend gives no warning.
- [P3] 38-6 history.rs:163: after a collection switch the panel shows old rows until reload finishes; set Loading at reload start.
- [P3] 38-7 root.rs:181: toggling the panel closed drops it without restoring focus.
- [P3] 38-8 workspace.rs:812: with history_bodies off the body is still read on the UI thread.
- [P3] 38-9 ui/history.rs:228: each render deep-clones up to 500 entries for click closures.
- [P3] 38-10 settings.rs:381: round-trip test doesn't assert the written file text.
- [P3] 38-11 ui/history.rs:434: storage test lives in the app UI module with a pid temp dir.
- [P3] any redacted `auth.*` field resets auth to Inherit, including non-secret fields (ApiKey header name, SigV4 region/service) because storage over-redacts them; fix the redaction in storage.
- [P3] tab.rs:87-89: the Resend notice keeps listing disabled headers after the user re-enables and fills them.
- [P3] core body.rs:210-214: on Windows a pending-delete history body opens with PermissionDenied, not NotFound, so it shows a generic Io error instead of Removed.
- [P3] storage history.rs:741-760: no test covers clear's best-effort deletes and later orphan sweep.
- [P3] workspace.rs empty_json_leaves: keys compared in raw escaped form vs decoded names stored by the redactor (escaped keys never match); the `{{` branch skips to the first `}}` regardless of content.
- [P3] workspace.rs test `long_key_with_many_empty_leaves_is_fast` asserts < 1 s wall-clock; may flake on slow CI.
- [IDEA] Export storage's `mask_bare_refs` and use serde_json in the app for the redacted-body check instead of the hand-rolled scanner.
- [P3] r2 38-1 storage history.rs:752: clear ignores every delete error and reports success; bodies may stay on disk; lock-file comment wrong. Tolerate only in-use/pending-delete errors, report leftovers, fix comment. (→ task 99 when split)
- [P3] r2 38-2 tab.rs:95: `stored_body` is unused by the refusal and its doc describes a rule no longer implemented; remove or use it and fix the doc.
- [P3] r2 38-3 url_bar.rs:279: URL bar previews the collection scope while Send uses the original folder scope, so it shows false problems; share scope_path_for with refresh.
- [P3] r2 38-4 workspace.rs:162: the scope guard compares only the URL template; a request var override can send folder credentials to a new host; also treat changed vars as edited, or compare the resolved host. (→ task 99 when split)
- [P3] r2 38-5 workspace.rs:452: a non-JSON Text/Xml body is refused for good; Body::None handled inconsistently; refuse a failed scan only for Body::Json.
- [P3] r2 38-6 workspace.rs:1414: body refusal is a Syntax problem detected by message prefix; use a dedicated SendError variant.
- [P3] r2 38-7 (Nit) workspace.rs:238: with both userinfo parts redacted, url.password is reported as unhandled.

### Follow-ups from task 96 (not 0.2.0 wave 1)
- [P3] 37-5 import.rs:435: warnings are shown verbatim; a malformed curl header can echo its token (fix at source, task 99 AC19).
- [P3] 37-6 import.rs:448: the secrets note mentions Postman collection variables for curl imports; pick the text by format.
- [P3] 37-7 import.rs:156: the folder count omits the created target; a failed write leaves it behind and the error does not say the import is partial.
- [P3] 37-8 import.rs:431: each render re-walks the result and rebuilds warnings; Import deep-clones the result on the UI thread.
- [P3] 37-9 root.rs:464: the overlay does not block clicks behind it; switching collection behind it retargets the import.
- [P3] 37-11 import.rs:407: collection switched or closed mid-write hides the Done report or misattributes it; keep the write root and render Done from it, or block switching while writing.
- [P3] 37-12 import.rs:208: missing-folder branch can import into a renamed folder (api-2) with no mention; refuse or report when create_folder returns a different name.
- [P3] 37-13 (Nit) import.rs:206: reserved target gives two different error messages; inputs stay editable while writing.
- [P3] import.rs:141-148: read_capped checks metadata then opens by path (TOCTOU to a FIFO); open first, then call file.metadata().
- [P3] import.rs:194-205: the probe ignores remove_dir's result; a stray empty probe folder can remain.
- [P3] import.rs:704-710: `write_failure_returns_an_error` only reaches the not-a-folder refusal, not a failure inside import::write.
- [IDEA] storage ops should expose a public path validator (e.g. ops::check_dir) so the dialog stops probing with create_folder/remove_dir.
- [P3] Escape only closes the dialog while focus is inside it; clicking the workspace behind the overlay sends Escape to CancelRequest.
- [NOTE] import::write writes collection defaults and environments before requests, so a mid-import failure leaves a partial import (already tracked under task 94).

### Follow-ups from task 94 (not 0.2.0 wave 1)
- [P3] 34-43 curl.rs:355: unknown options fail open; their argument can become the committed URL. Prefer the `://` positional, or fail with a Parse error naming the option.
- [P3] 34-44 mod.rs:271: `split_userinfo` misses lenient forms `https:///u:p@h` and `http:/u:p@h` that reqwest accepts.
- [P3] 34-45 mod.rs:358: the heuristic misses jwt, bearer, dpop, assertion and pass header names (to be fixed once in the shared `wisp_core::redact` list).
- [P3] 34-46 curl.rs:404: the extra-URL warning cuts at `?` or `#` before stripping userinfo, leaking part of a password; echo only the host.
- [P3] 34-50 curl.rs:1228: the new header-name tests skip terminated and whole-value checks; the "X Api Key" case passes without the fix.
- [P3] 34-47 (Nit) curl.rs:1047: the every-option test omits preproxy, json and post-7.84 entries; iterate the const.
- [P3] Collection- and folder-level Postman auth and variables are warnings only. Mapping them needs new `ImportResult` fields; most Postman collections put auth there.
- [P3] Postman folders count toward no cap (about 870k empty folders fit in 10 MiB).
- [P3] An environment name that is a reserved device name (for example server description "con") aborts the import partway through.
- [P3] openapi.rs:451-458 (34-22, Nit): the example budget is charged after `to_string_pretty`, so a deep example briefly allocates about 60 times its size (728 MB peak for 9.8 MiB).
- [P3] 34-34 postman.rs:65: generated secret names can shadow collection variables, which they outrank at resolve time; reserve collection var keys in SecretSink first.
- [P3] 34-35 postman.rs:195: unsupported auth types fall back to Inherit and get overwritten by URL userinfo, contradicting the warning; apply userinfo only when no auth object exists.
- [P3] 34-36 tests/import.rs:199: credential end-to-end test skips URL userinfo and the secrets .gitignore.
- [P3] openapi.rs: an OpenAPI server `description` becomes the environment file name, so credentials typed into a description reach a file name.
- [P3] `unique` in write.rs is quadratic in the number of colliding names; use a HashSet plus a per-base counter, as postman.rs does.
- [P3] `key_string` (yaml.rs) silently collapses null, sequence and mapping keys.
- [P3] 34-9 curl.rs:467: `-d` without Content-Type becomes text/plain; curl sends urlencoded.
- [P3] 34-10 curl.rs:477: `--digest`/`--ntlm`/`--negotiate` with `-u` imported as Basic (security).
- [P3] 34-11 curl.rs:401: warnings echo raw headers, extra URLs and fields (security).
- [P3] 34-12 curl.rs:385: dropped constructs give no warning.
- [P3] 34-13 write.rs:54: validate `folder_rel` before any write.
- [P3] 34-14 yaml.rs:185: YAML merge keys stay literal; nameless parameters are dropped silently.
- [P3] 34-15 openapi.rs:163: `substitute` is still quadratic within the 64 KiB cap.
- [P3] 34-16 openapi.rs:526: the depth test never reaches `check_json_depth`.
- [P3] 34-17 Cargo.toml:52: saphyr-parser lacks `default-features = false`.
- [P3] 34-18 yaml.rs:84: `TooDeep` matches saphyr's error text; pin `=0.3.3`.
- [P3] 34-19 yaml.rs:221: wall-clock asserts in tests risk flakes.
- [P3] 34-20 write.rs:207: `unique_stem` is quadratic when names share a slug.
- [P3] 34-21 write.rs:122: warn on `{{var}}` used but not defined by the import (security).
- [P3] 34-27 openapi.rs capped-names test accepts both Ok and TooLarge and asserts under 900 ms; pin one outcome, drop or loosen the timing assert.
- [P3] 34-28 openapi.rs:27 visit work shares the 64 MiB example budget; large legitimate specs may fail with a vague error. Use a separate visit counter scaled to input.
- [P3] 34-29 (Nit) openapi.rs:44 `capped_text` duplicates `truncate_bytes`; the two MAX_NAME_BYTES constants differ (100 vs 80).
- [P3] mod.rs `secret_ref`: a value containing `{{` anywhere is kept literally, so a credential mixed with a ref (`Bearer live-tok {{x}}`) stays in TOML.
- [P3] postman.rs: collection-variable keys go into secret names unsanitised; a key with `}}` or trailing whitespace makes an unresolvable ref.
- [P3] `split_userinfo`: a password containing raw `/`, `?` or `#` stays in the URL (curl does not treat it as userinfo either).
- [STYLE] mod.rs:441 test `split_userinfo_ignores_at_after_authority` sits above `use super::*;`.
- [NOTE] All non-empty Postman collection variables (including e.g. baseUrl) now go to the imported secrets environment and resolve only when it is selected; the Import dialog's secrets note covers this (task 96).
- [NOTE] User decision 2026-11-10: imported file parts become disabled empty parts (never a path); URL userinfo becomes Basic auth via secrets, explicit auth wins.
- [NOTE] Re-review: an unsafe file part is saved as `""` and loads as the request's directory, so Send fails with a read error rather than "no file selected". On Windows, nested 80-byte folder names can exceed MAX_PATH for other tools.
- [NOTE] `write` is not atomic; an error partway through leaves a partial import.
- [NOTE] saphyr-parser 0.3.3 buffers whole flow collections (scanner.rs:866-880); bounded by the 2 MiB YAML cap to about 450 MB. Accepted.
- [P3] 34-40 curl.rs:969: the expand-options test asserts only that credentials are absent, not that they were routed to secrets/auth.
- [P3] 34-41 (Nit) openapi.rs:251: the userinfo warning re-parses the host with `split_once` and can name the wrong host; return the authority from `split_userinfo`.
- [P3] curl.rs:21: arg-taking options newer than curl 7.84 are still missing (`ipfs-gateway`, `keepalive-cnt`, possibly `sigalgs`, `knownhosts`, `parallel-max-host`); the list was built from local curl 7.84 `--help all`. Regenerate from a current curl.
- [P3] curl.rs:381: `-h <category>` does not consume its argument (`curl -h all http://h/x` imports `all` as the URL); harmless.
- [NOTE] `is_credential_header` uses a broad substring heuristic (auth, token, secret, key, session, cookie, password, passwd, credential, signature), so headers like X-Monkey move to secrets; accepted, fail-safe.
### Follow-ups from task 95 (not 0.2.0 wave 1)
- [P3] `engine.rs:814-815`: the engine follows 9 redirects; reqwest follows 10.
- [P3] `engine.rs:834-838`: `transfer-encoding` is not stripped when the body is dropped on a rewrite.
- [P3] The 301/302 method-rewrite behaviour differs from reqwest's for SigV4/Digest requests (allowed by RFC 9110).
- [P3] `sigv4.rs:142-146`: a user-supplied `x-amz-*` header is sent alongside the added one, so values are duplicated.
- [NOTE] `replay` copies the body for every SigV4 request; it could be limited to `own_redirects || digest`.
- [NOTE] No test covers A→B→A; the code is correct.
- [NOTE] The S3 canonical URI and the `+` handling in queries are unproven against AWS.
- [NOTE] No test covers a 303 or a POST→GET rewrite on the own-redirect path (reviewer's suggestion).
- [P3] 33-6 `digest.rs:95`: the challenge locator matches `digest` inside a quoted value and gives up; params with bad whitespace are dropped.
- [P3] 33-7 `engine.rs:613`: Digest refuses File bodies, though the engine can reopen and resend them; the message is wrong.
- [P3] 33-8 `engine.rs:772`: an unsupported Digest algorithm or qop turns the 401 into an engine error that hides the response. Return the 401 with a warning, and truncate the echoed algorithm.
- [P2, security] 33-9 `engine.rs:844`: a cross-origin hop keeps user-set `x-amz-*` and `Host` headers. Same family as the task-86 custom-header follow-up.
- [P3] 33-11 `sigv4.rs:148`: `sign` returns `Result<_, String>` and `DigestError` hand-implements `Error`; derive thiserror errors per the conventions.
- [P3] 33-12 `tests/auth.rs:146`: hand-made temp files leak on panic; use `tempfile::NamedTempFile`.
- [P3] 33-13 `ui/request/auth.rs:219`: the access key field is unmasked while Debug redacts it.
- [NOTE] 33-15 `tests/auth.rs:306`: use `assert_eq!`.
- [P3] `sigv4.rs:114` `canonical_query` decodes lossily, so `%FF` is signed as `%EF%BF%BD`. Fix: percent-encode the decoded bytes directly.
- [P3] No test signs a multipart body for a non-S3 service, so the boundary bytes are covered by no test.
- [NOTE] `hash_file` reopens the file by path, leaving a gap between hash and send for a same-length replacement. Hashing is not covered by the request timeout and continues after a cancel.

### Follow-ups from task 93 (not 0.2.0 wave 1)
- [P3] A `Set-Cookie` with no `name=` before the first `;` (e.g. `Set-Cookie: abc123token`) is stored in plaintext; Amendment 1 only redacts after the first `=`.
- [P3] `history.rs:246`: data-dir creation failure shows "History entry dropped" instead of "History is off: <reason>" and never disables history.
- [P3] `tests/history.rs` `evicts_oldest_over_the_byte_cap` does not assert bytes <= cap or that the survivors are the newest run.
- [IDEA, user 2026-11-10] Opt-in setting to keep `Set-Cookie` values unredacted in history (developer tool); off by default.
- [P3] 35-5 `workspace.rs:612`: up to 1 MiB response body copied on the UI thread even when history is off; check history state first, read spilled bodies in the background.
- [P3] 35-6 `history.rs:293`: request body in `.req.json` is uncapped; one large body evicts all other history. Cap at BODY_CAP with a truncated flag.
- [P3] 35-7 `history.rs:357`: every append re-reads the index and stats every body file; store byte count in `HistoryEntry`.
- [P3] 35-8 `tests/history.rs:58`: redaction tests cover only Bearer and two headers; add a table test over every Auth variant, Cookie, mixed case, `Bearer {{t}}`.
- [P3] 35-9 (Nit) `history.rs:123`: `keep_var_ref` round-trips through `Auth::Bearer` to reach a private core helper; make core `keep_var_ref` pub; dedup names with `eq_ignore_ascii_case`.
- [P3] `history.rs` `redact_json_body`: once anything is redacted the JSON body is re-serialised compactly (keys re-sorted, formatting lost, integers beyond i64/u64 become lossy f64).
- [P3] `history.rs` `redact_json_body`: restoring masked `{{refs}}` uses one `text.replace` per ref, O(refs x body); do a single pass.
- [P3] `history.rs`: path vars (`Request.path`) are not redacted.
- [P3] 35-21 `history.rs:370`: `mask_bare_refs` is quadratic with allocation on bodies full of `{{` while both locks are held; scan forward over grammar chars only, then require `}}`.
- [P3] 35-22 `history.rs:665`: duplicate ids in the index can delete files a surviving line still uses; skip deletion for ids still present.
- [P3] 35-23 core `redact.rs:5`: public fixed-length array type; history repeats the body-matching logic inline. Expose a slice or `contains_credential_word`.
- [P3] 35-24 `tests/history.rs:590`: no positive tests that padded, dotted, `$` and Unicode refs survive, or uppercase Text secrets.
- [P3] 35-25 (Nit) `redact.rs:29`: test name `credential_names` is not a sentence.
- [P3] 35-27 (Nit) `import/mod.rs:370`: `is_credential_header` is a one-line passthrough; call the core fn directly.
- [NOTE] `history.rs`: `lock()` blocks without timeout while `APPEND_LOCK` is held, so a stalled second Wisp process stalls appends for every collection in this one.
- [NOTE] Name heuristic over-redacts (e.g. `monkey`, `auth_url`, `token_url`); accepted.
- [P2] CI builds only ubuntu and macos; the Windows `File::lock` bug in PR #35 would have shipped. Add at least a Windows `cargo check`/test job (or a cross `cargo check --target x86_64-pc-windows-msvc`).
- [P3] 35-13 `history.rs:524`: name heuristic not applied to response headers (`X-Auth-Token`, `X-Subject-Token` stored).
- [P3] 35-14 core `redact.rs`: `SECRET_FRAGMENTS` misses sig, pass, pwd, jwt, bearer, dpop, assertion (Azure SAS `sig=` stored). One shared list now serves history and (after #34) importers.
- [P3] 35-15 `history.rs:399`: duplicate JSON key with a ref last keeps the earlier literal secret; detect duplicates and fail closed.
- [P3] 35-16 `history.rs:159`: unquoted `{{ref}}` under a secret key is emptied instead of kept.
- [P3] 35-17 `history.rs:511`: lock file opened with create and follows symlinks; use O_NOFOLLOW or a `symlink_metadata` check.
- [P3] 35-19 (Nit) `tests/history.rs:355`: "1234" leak check can match the random hex entry id.
- [P3] `history.rs:617-622`: index lines with no valid id are dropped silently on rewrite; add a comment or test.
- [P3] `tests/history.rs` `sweep_keeps_entries_with_unparseable_index_lines`: set a low entry cap so eviction runs and the verbatim-rewrite path is covered.
- [NOTE] Text/XML bodies containing any credential fragment are not stored (fail closed); a bare `{{ref}}` with characters outside letters, digits, `_`, `-`, `.` makes a JSON body drop. Accepted, safe side.

### Follow-ups from task 92 (not for 0.2.0 wave 1)
- [P3] `ensure_collection_id` (`collection.rs:242-257`) has a read-modify-write race: two instances adding an ID at once means the last rename wins. `save_collection_defaults` has the same window.
- [P3] `ensure_private_dir` (`data_dir.rs:92-101`): a trailing slash (e.g. `WISP_DATA_DIR="/x/link/"`) gets past the symlink refusal on macOS. Normalise before the lstat.
- [NOTE] Hand-written IDs that differ only in case collide on case-insensitive file systems. Consider requiring lowercase.
- [NOTE] An existing data dir wider than 0700 is not tightened (task 93 may care).

### Pane-scrollbar defects (tasks 46-49, fixed and hand-checked 2026-11-02)
All four defects found hand-checking 43-45 are fixed (see HISTORY). Combined binary 19,014,912 B (+0 B).
**Residuals:** many inherited headers can squeeze the request-header table (the inherited list keeps natural height per spec). (Path, Auth, Settings, Multipart and File sub-tabs now scroll; task 52.) Do not add `.flex()` to the `div().w_full()` wrapper around KvTable virtual rows (46).
**Header limit (47, resolved by 50):** the limit is now 1,000 headers; see the Task 50 residuals below.

### Reqwest 0.13 follow-ups (task 50)
Optional, none blocking. Closed by 51: the runtime TLS test (`crates/core/tests/tls.rs`) and the `webpki-root-certs` `default-features = false` consistency fix. The `engine.rs:87`, `install_crypto_provider` and `https://localhost` residuals were closed by task 56 (see HISTORY).
Budgets after 50: binary 19,114,864 B of 20,971,520; idle RSS 97.2 MiB; after one request 102.3 MiB.

### Open budget and measurement questions (from M0)
**Per-platform budgets (done, task 65):** set by the user on 2026-11-03 and enforced by `measure.yml` through `scripts/measure/budgets.json` (table in `docs/architecture.md#budgets`). Both Linux backends (x11 and wayland) are kept. Intel macOS was dropped from 0.1.0 on 2026-11-03 (task 71).
**Residuals (71):** if the screenshot times out or `import` fails silently, the log line is a bare `screenshot: ` with no reason (`measure.sh:236`); `dpkg -S` diversion lines would become a bogus package entry in `packages.txt` (none today); the `unowned:` branch of the packages step has never executed in CI.
**Residuals (65):** the Windows over-budget path (`within: false`, exit 3) has never executed; the logic reads correct and the first real over-budget run is its test.
**Residuals (64):** Linux idle RssAnon is about 4.8 MB higher with the link flags (58,168 vs 53,332 KiB for panic/opt-level alone) while after-request is lower; likely noise, recheck when it next shows in a run. `.cargo/config.toml` target rustflags are replaced by any `RUSTFLAGS` env and need lld (GNU ld rejects `--icf=all`). The responsiveness hand check was done on macOS arm64 only (same speed); Windows and Linux builds were not hand-checked. `panic = "abort"` means any panic ends the process.
**09 review P3s:** `WISP_TRACE window opened` prints from inside the `open_window` build closure, before it returns; `measure.ps1`'s final catch block (`:184-192`) prints only the message, not the stderr tail nor whether stderr was empty.
**Residuals (68):** `measure.ps1` has never run locally (no pwsh); its CIM-failure path (`Get-PrivateWs` returning null) and its bad-budgets and one-element-array checks have not executed at runtime, only been read. The Windows CI job covers the normal path.
**Residuals (73):** macOS portable mode is unsupported by decision, but `settings.rs:70-75` still honours a `portable` marker inside `Wisp.app/Contents/MacOS` (which edits the signed bundle), so the code is out of step with the README. The README's claim that Finder, not `unzip`, sets quarantine is unverified until the user's macOS hand test. Its platform-testing statements describe the state at release and must be confirmed before tagging. It needs a line for the app icon when task 76 lands and must drop any console or runtime caveats when task 77 lands; none are present today.
**Residuals (77):** in a Windows release launched from Explorer all stderr is lost (settings warnings, window-open failures, panic messages under `panic = "abort"`); a crash/error log file or attaching to the parent console would be a future feature. A local `cargo build --release` on Windows without `--target` applies `crt-static` to build scripts and proc-macros too (works on MSVC, noted only).
**Residuals (75):** `scripts/release/check-glibc.sh` discards objdump's own error text on failure, and its header's exit-2 description is narrower than its use. Linux RssAnon rose about 14% on ubuntu-24.04 (idle 61,132 vs 53,376 KiB; after request 51,256 vs 45,096) with no explanation; likely different mesa and libraries, still within budget.
**Residuals (76):** `generate.sh --check` does not flag stale extra files; shellcheck SC2209 false positive at `scripts/icon/generate.sh:18` (`mode=write`) makes shellcheck exit 1; `package.sh:58` `assetutil | grep -q` could SIGPIPE under pipefail on a very large catalog (task 78 will harden it); the macOS runner depends on the unpinned default Xcode having Icon Composer support; Linux window icon and `.desktop` file done by task 87; the user's visual check of default, dark and tinted is part of the release hand test. The README gains an icon line only if it discusses appearance (it does not).

**Residuals (78):** the saved keychain search list is not restored if it was empty (harmless on a fresh runner); the `notarytool` timeout JSON shape is unverified; Windows code signing (SmartScreen) is not done, candidate follow-up Azure Trusted Signing; a macOS portable-mode marker inside the bundle would break the notarised signature (see Residuals (73)).
**Residuals (74):** the non-macOS platform modifier is labelled "Win" where Linux calls it Super (no current binding uses it); only enter, escape and tab get display names, other named keys are uppercased.
**Residuals (63):** `measure.sh:131-133` vulkaninfo fallback may name a different device than GPUI picked, and the JSON does not say which source produced `renderer_device`. `measure.ps1` was not parse-checked locally (no pwsh); only CI covers it.
**06 (resolved):** the method and environment selects now show chevrons (task 37, hand-checked 2026-11-02). UI automation needs the window frontmost.

### Carry-forward: crates/core
**Debug redaction follow-ups (84):** multipart text values (`PreparedPartValue::Text`) still print in Debug; URL path and userinfo secrets are not redacted in `PreparedRequest` Debug (only the query is); `EngineError` messages include the URL; `ResponseCookie` derives Debug and holds cookie values; a proxy password with an unencoded `/`, `?` or `#` still prints in `ClientSettings` Debug (reqwest rejects such proxies anyway). None are in 0.1.1.
**resolve nits (11):** a verbose-mode regex `(?x)...#c` can be misreported as `InvalidPattern`; problem dedupe keys on the Debug string; a test helper has a leftover `eprintln`; a scheme-less URL's `://` search can land in the query (the engine rejects such URLs anyway).
**Template helpers (22):** no resolve-level test pins `apply_path_params` to `path_range_plain` (an unclosed `{{` in the authority). `query_start_is_linear` asserts wall-clock < 100 ms and could flake on loaded CI. `expand` allocates all var refs before the budget check (linear, not unbounded). `lookup_var("")` is Unresolved while `resolve` reports Syntax for `{{ }}`. `query_start` can return a `?` inside a fragment.
**Types test (10, optional P3):** the File/Multipart unsupported test checks only the error kind, not the message.
**Body store (12):** the uid-ownership check on the spill directory has no test (it needs root). Three body tests spill into the real `<temp_dir>/wisp-<uid>` rather than a test base directory. The non-unix spill-directory code is not compiled or tested on this Mac.
**Client cache (16):** `ClientCache` builds a client while holding a std `Mutex`. It is tested only through `ClientCache::get`, not `Engine::submit`.
**Engine tests and limits (16):** the multipart test leaves a temp file behind if an assertion fails. The app's `BODY_LIMIT` is now 64 KiB (first 64 KiB read from `bodies()`).

### Carry-forward: crates/app
**Hand check (69):** the macOS arm64 check of the `.id("…-editor")` removal passed on 2026-11-03 (long Path, Auth and Settings sub-tabs scroll, checked with task 66's build); only the Windows and Linux hand checks remain. Possible flake, unconfirmed: wisp-core `a_20_mib_file_body_streams_with_content_length` failed once in a heavily parallel full-workspace run and passed alone; watch for it.
**Residuals (79):** the Touch Bar Dock-Quit crash root cause is upstream (`accesskit_macos` subclass restore versus KVO) and not reported upstream yet. Cmd-Q and window close now ask before discarding drafts (task 83); Dock Quit and system shutdown still do not (documented in `docs/architecture.md#app-lifecycle`). The `WISP_CLOSE_WINDOW_AFTER_MS` hook calls `quit_app` directly, so `quit-check.sh` does not exercise the close button (hand-tested). No About menu item (GPUI 0.3.10 `OsAction` lacks it). Hide, Hide Others and Show All were added by task 83 on macOS. shellcheck SC2010 (`ls | grep`) in `quit-check.sh`.
**Escape (21):** Escape is bound with context None to `CancelRequest`. Hand-checked in task 41: Escape inside the focused URL input cancels a send (status "Cancelled").
**measure.sh (21):** the `WISP_EVENT` line was not separately confirmed in the output; status "measured" implies it.
**KvTable (29):** hand-checked in task 35 (typing in Params updates rows and the URL; Tab goes name, value, next name). Accepted residual: above the 50-row virtualisation threshold Tab cannot reach rows outside the viewport, because unpainted rows have no focus handle; scroll-to-focus is deferred. The table only virtualises if its container gives it a definite height (task 31). Optional: `locate` in `crates/app/src/ui/kv.rs` is O(n) per keystroke.
**Folder order (39, optional):** a list naming a request in both forms (`["a","a.toml"]`) has only its first match edited by rename, move and delete. `a.toml` is ambiguous when both `a.toml` and `a.toml.toml` exist. Hand-checked in task 41: rename and delete in the running app update `folder.toml` and the tree follows its order.
**Storage ops tests (23, optional):** `case_alias_sidecar_counts_as_shared` only exercises identity on delete and is vacuous on non-case-folding file systems.
**Palette and highlight (25, optional P3):** `palette.rs:136` lowercases only the first char of a label, so chars with a multi-char lowercase (such as "İ") never match. Write the hover " (uses a secret)" suffix format into the spec for task 31.
**Formatting (26/36, optional P3):** `sniff_kind` strips the BOM after the UTF-8 check, so a sniff window cut inside a BOM reads as Text (not a real-world issue).
**Settings (27):** ToggleTheme and ToggleLayout (task 28) surface `settings::save` errors to the user, because save now returns Err on an unparsable or unreadable file instead of overwriting it.
**Draft and URL sync (24):** accepted residual: `Request.vars` has no `DirtySections` flag (vars change only through external edits in v1), so a draft can be dirty with no section highlighted. `merge_parsed_params` aligns old and parsed params with a 3-level order-preserving LCS (name+value, name, same slot); it is O(n*m) per keystroke, fine at real param counts (about 8 MB at 1000 params). Names containing `=` or `&`, and empty names, do not round-trip (documented on `display_url`). Optional cosmetic: group the test helpers `pairs` and `kv` at the top of the `url_sync` tests.
**Workspace shell (28):**
- *Measurement:* superseded by the task 35 figures under M2 (the shell measured about 96 MiB idle and binary 15.3 MB when it landed).
- *Hand-checked in task 35:* ⌘O from A to B (with dirty tabs: Discard, Open panel) closes A's tabs and resets the environment; ToggleTheme and ToggleLayout from the palette write only `theme` and `split` to settings.toml; theme="system" follows macOS Light/Dark live. The dev build wrote `~/Library/Application Support/wisp/settings.toml` during the check.
- *Mislabelled autosend error (fixed in task 54):* the failure line now reads `WISP_EVENT failed kind=Resolve problems=<names>`; see HISTORY.
- *Gaps:* (tab close button landed in task 55.) Routing, history and watcher wiring have no end-to-end tests because gpui test support is not enabled; the open-collection test covers only the `open_needs_confirm` helper.
- *Optional P3:* `bind_keys_indices_match_names` does not prove `key(i)` binds the right action.
**Sidebar (30):**
- *Hand-checked in task 35:* env switching, rename (file and tab renamed) and delete (prompt names the item; file and sidecar removed). Tree order was wrong and rename/delete left stale `order` names; fixed in task 39 at the storage layer, running-app check passed in task 41. Create and move were not separately reported.
- *Optional:* a node created inside a collapsed folder is selected but hidden, because the parent is not expanded. An environment literally named "No environment" clashes with the `NO_ENV` entry. The delete-prune `collapsed.retain` has no unit test (correct by inspection).
**URL bar and sub-tabs (31):**
- *Hand-checked in task 35:* URL and Params live sync, highlight, hover tooltip ("baseUrl = … · collection"), status colouring. Params checkbox resync while the URL is focused fixed in task 40, hand-checked in task 41 (typing `?a=1&b=2` with no caret jump; untick and re-tick while focused resync). Not reported: secret `••••` hover, Cancel while sending.
- *Decision on resync:* the "only on DraftReplaced" rule holds for Headers and Settings. Params, the URL input (when unfocused) and Path also resync on `DraftEdited`, behind equality guards so an edit never echoes back.
- *Optional:* a non-numeric or zero timeout is silently ignored in Settings (`settings.rs:73-75`). The URL bar also refreshes on `CollectionChanged` (harmless). A URL that does not round-trip through `apply_url_text` (an enabled param with an empty name) can still produce one extra edit.
**Auth and body editors (32):**
- *Hand-checked in task 35:* JSON sidecar edit and ⌘S writes the sidecar only; Bearer, Inherit, Bearer restores the token (masked, reveal toggle); "Inherits Bearer from collection" line; Format. Multipart Text plus File part send hand-checked in task 41 (file part sha256 matched).
- *Lesson for later UI work:* gpui-component 0.8.1 `InputState::set_value` emits no Change event, so any programmatic set must update the model or draft by hand (the cause of the round-1 P1/P2).
- *Optional:* `path.exists()` stats the filesystem on every render of the File body; a hand-typed relative File path is checked against the process cwd rather than the collection dir, so "File not found" can be wrong; a Choose… pick into a row deleted while the dialog is open is silently dropped.
**Palette and environment switcher (34):**
- *Hand-checked in task 35:* all passed (`gu`, `theme`, `layout`, ⌘E with dev pre-selected, Escape, shortcuts afterwards). Focus loss after other unmounts fixed in task 38, hand-checked in task 41.
- *Lesson:* in gpui-pre 0.3.10 key bindings dispatch before key-down listeners, and a context-less binding (such as escape to CancelRequest) beats component-context ones. Overlays must handle such actions with `on_action` on their own div, and an overlay that takes focus must restore it on dismiss.
- *Optional:* with more than 50 environments the active one past the limit is not pre-selected. ⌘K or ⌘E while the palette is open reloads it in place and clears the query rather than toggling it closed.
**Response viewer (33):**
- *Hand-checked in task 35:* Pretty keeps key order; a 20.3 MB response shows Raw with Pretty disabled; two Set-Cookie rows; header cell click and ⌘C copy. Hand-checked in task 41: Raw prefetch while scrolling, over-limit Pretty tooltip, Save output sha256. The 4 MB Raw cap line was verified in task 42 (see below).
- *Raw scrollbar and jump keys (42, hand-checked 2026-11-02):* Raw and Pretty show a draggable vertical scrollbar; End/⌘↓/Ctrl+↓ load to the 4 MB cap and jump to the bottom, Home/⌘↑/Ctrl+↑ jump to the top (any modifier works with End/Home; keys are handled on the BodyPane focus handle, and with the URL input focused End moves its text cursor). The 4 MB cap line ("Showing the first 4 MB — Save to read the rest") is verified on a 250,000-line body.
- *Optional P3 (42):* the `jump_still_valid` asserts sit inside the key test and could be split; when the 4 MB boundary falls mid-line the cap line follows a partial final line. Budgets measured: binary 18,998,304 B (+33,088), idle RSS 100,064 KiB, after request 104,512 KiB.
- *Scrollbars (43-45, hand-checked 2026-11-02):* sidebar, response Headers and Cookies and KvTable now have draggable scrollbars. Budgets: combined binary 19,014,912 B. See the pane-scrollbar defects below.
- *Decision:* headers are per-cell selectable divs with ⌘C copy, not the gpui-component `Table`. There is no partial-text selection and no keyboard cell navigation.
- *Optional:* header selection is cleared on every `ResponseChanged`, which sub-tab and Pretty/Raw switches also emit (consider a separate view-change notify). `failed_create_keeps_existing_target` silently skips its assertions when run as root. `File::create` truncates before writing, so a failed Replace leaves no original (not an atomic replace).
**Icon assets (37):**
- *Measurement:* binary 18,981,728 B (+16.5 KB over main), idle RSS about 97.3 MiB, after-request about 101.9 MiB.
- *Facts:* 19 icons embedded from gpui-kit-assets 0.8.1 (Apache-2.0, NOTICE kept) in `crates/app/assets/icons/`. If dialogs or notifications are used, add CircleCheck, CircleX and Info. The walk test fails naming any `IconName` used in app sources that is not embedded.
- *Hand-checked in task 35:* select chevrons, checkbox ticks, eye, × and search icons render. Tip: launching the bare binary may not expose a window to computer use until `WISP_AUTOSEND_URL` is set; wrap it in a scratch `.app` bundle with a bundle id.
- *Optional:* `main.rs`'s run line was split into `let app = ...with_assets(...); app.run(...)` to avoid a rustfmt reflow.

### Carry-forward: crates/storage
**Codec (13):** `write_vars` never reorders existing keys. Flipping an explicit `enabled = true` to false may drop a trailing comment on that line. New entries in multi-line inline arrays get `toml_edit` default spacing. The Windows readonly-target save path in `atomic_write` is untested. `write_auth` keeps its signature, so callers skip the call when the value equals that file's default and the file had no `[auth]`; use `atomic_write_private` for files that hold secrets.
**Watcher (14):** nested directory renames within the same 100 ms window may emit one spurious `Changed` for a directory-derived path (accepted); the M2 reload must re-check the disk and tolerate `Changed` for non-file paths. On Linux notify follows symlinks into existing linked directories by default; consider `follow_symlinks: false`. The event-time `p.is_dir()` follows symlinks while `skip_directory` does not (harmless).
**History (15):** Concurrent writers could race rotation of `history.jsonl`; v1 has one writer.
**Request files (17):** Method parsers unified in task 58 and `.toml` sidecar names are rejected. Task 85 closed sidecars aliasing sibling request files, trailing dot/space names, reserved device names and Unix hard links; hard links are not detected on Windows (std has no stable identity API). Windows `::$DATA` aliases are not covered. The drive-prefix check rejects any name with `:` as its second byte. Sidecar symlinks have a check-then-use race. `bare_relative_request_path_allows_inside_symlink` uses the process-global `set_current_dir`; safe today because all other tests use absolute paths, but a future relative-path test could flake.
**Collection tree (18, walk fixed in 58):** the cycle rule is now ancestor-only with a 10,000-folder whole-walk budget; directory symlinks pointing outside the root are followed; a broken `folder.toml` fails the whole open; `scope_for` with a non-canonical absolute root returns no folders (callers pass relative paths).
**Environments (19):** names with spaces are now rejected, so an existing environment with a space in its name will not load; on case-insensitive filesystems `Dev` and `dev` map to the same file. A symlinked `environments/dev.toml` loads but is not listed, because `file_type` does not follow links; one `file_type` error aborts the whole listing. The plain file is written before the secrets file, so a save that half-fails can lose a var that is moving between them.
**Integration test (20, accepted):** the JSON body fixture has no comment because JSON cannot hold one.

## Next

### Follow-ups from task 83 (not v0.1.1)
- No automated test sends the real `Quit` action with a window active. It needs gpui `test-support` in `crates/app/Cargo.toml`, or `quit-check.sh` sending the action instead of AppleScript `terminate:`. The review P1 (the handler did nothing while a window was active) got past the tests for this reason.
- The open-collection prompt (`ui/root.rs:193`) and the delete-from-disk prompt (`ui/sidebar.rs:678`) do not set `prompt_up`, so a quit during one of them with a dirty tab queues a second "Discard unsaved changes?" sheet on macOS. No data is lost.

### Follow-ups from task 88 (not v0.1.1)
- The Cookies column header now scrolls up with the rows instead of staying pinned (accepted by the user, 2026-11-09). Fix: a separate header x-scroller that follows the rows' horizontal offset.
- When the Cookies table is tall, sideways scrolling works only with the pointer over the rows.

## Someday
