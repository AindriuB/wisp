# 107 — Cookie jar in the app: per-collection persistence toggle, Cookies manager, debounced save

**Repo:** `.`
**Depends on:** 101, 102, 104, 106 (106 for `workspace.rs` and `ui/mod.rs`: serial)
**Size impact:** low (GPUI code only, no new crate)
**Owns:**
- crates/app/src/workspace.rs (jar wiring, load/save/flush, banners)
- crates/app/src/ui/cookie_jar.rs (new: the Cookies manager panel)
- crates/app/src/ui/mod.rs (one `pub mod cookie_jar;` line)
- crates/app/src/ui/root.rs (mount the panel like the history overlay; action handlers)
- crates/app/src/actions.rs, crates/app/src/palette.rs, crates/app/src/ui/palette.rs (one action and one command each, plus their tests)

## Goal
The app passes task 104's `CookieJars` to the engine and keeps a clone, so each collection's cookies carry between its requests in a session and the app can reach that collection's jar. Per collection, the user can turn on "Keep cookies between sessions". While it is on, persistent cookies (never session ones) are saved through task 102's `CookieFile`: debounced after changes, and on collection switch and quit. They are loaded again when the collection opens. A Cookies manager lists the jar's cookies, deletes one, clears all and holds the toggle. The response-side Set-Cookie table is unchanged.

## Context
- docs/plan/specs/2026-11-09-wisp-0-2-0.md#7-cookie-jar-core (off by default, 0600 file, session cookies dropped, debounced and on exit)
- Task 102 (`CookieFile`: the toggle is the file's existence; `StoredCookie`), task 104 (`CookieJars::jar(Option<&str>)`, `EngineConfig.cookies`; `CookieJar`: `snapshot`/`load`/`remove`/`clear`/`generation`; `CookieRecord`)
- crates/app/src/workspace.rs:474-479 `collection_partition` (calls `ensure_collection_id` on the UI thread on every Send, 42-11); :120-131 `HistoryJob::open` and its function-local `RESOLVE` lock; :990-994 where `send` sets `client.collection`.
- PLAN.md "Follow-ups from task 102" NOTE: `save`/`disable` race (closed by 93f5167 on task 102's branch, a writer lock with an `is_enabled` re-check), `load` symlink check-then-open race, symlinked `cookies/` parent.
- crates/app/src/workspace.rs:612-630 `Workspace::new` / `Engine::start(EngineConfig { .. })`; :681 `open_collection`; the history store's `history_job` / background-executor pattern and its "History is off" banner. Copy that for "Cookies are not saved: <reason>".
- crates/app/src/ui/root.rs:180-230, :532, :598-610 — the history panel overlay, Clear-with-confirm and toggle notice patterns
- crates/app/src/palette.rs:19, :63, :80 — `CommandId` list and labels; crates/app/src/actions.rs:23
- App launch rules (hand check): run only the binary you built, with `WISP_DATA_DIR` set to a fresh `mktemp -d` (task 101 isolates `settings.toml` this way). Stop only the pid you started; never `pkill` or `killall`. A bundle, if built, uses a bundle id other than `io.github.aindriub.wisp`. Never write the user's real `settings.toml` or data dir. Build with `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-107`.

## Acceptance
- [ ] AC1 One `CookieJars` is created in `Workspace::new`, passed as `EngineConfig.cookies`, and a clone is kept in `Workspace`. The open collection's jar is `jars.jar(Some(<partition>))` (AC10). Opening a different collection first flushes the old collection's jar (if persistence is on), then loads the new collection's file into its jar (if on) the first time that collection is opened in this run; reopening it later keeps its in-memory jar and does not reload. No jar is cleared on switch.
- [ ] AC2 The `CookieRecord` ↔ `StoredCookie` mapping is a pure function. It drops records with `expires: None` (session) when saving. A unit test shows a session cookie never reaches the `StoredCookie` list and that the other fields survive a round trip.
- [ ] AC3 With persistence on, a save runs on the background executor no sooner than 2 s after the last jar `generation` change, with at most one in flight. A final flush runs on collection switch and through `cx.on_app_quit`. The debounce decision is a pure helper with unit tests (fake times, no sleeps).
- [ ] AC4 Turning the toggle on calls `CookieFile::enable` and saves the current persistent cookies. Turning it off asks "Delete saved cookies for <collection>?" (Cancel keeps it on), then calls `disable`. The in-memory jar is untouched either way.
- [ ] AC5 No collection open, no collection ID, an unwritable data dir or a load or save error: the toggle is disabled or the action does nothing, and a banner says "Cookies are not saved: <reason>". Sending and everything else still work. A corrupt file is not overwritten until the user turns the toggle off and on.
- [ ] AC6 The Cookies manager (palette "Show cookies", action `ShowCookieJar`) lists name, domain, path, expiry ("Session" when none), Secure and HttpOnly. Values are hidden. Each row has Delete, and the panel has Clear all with a confirm and the toggle. The list refreshes when `generation` changes while the panel is open.
- [ ] AC7 No cookie value is logged, shown in the manager or written outside `cookies/<id>.json`.
- [ ] AC8 Hand check (macOS; agent or user), in the isolated launch above, against a local server that sets `Set-Cookie: p=1; Max-Age=600` and `Set-Cookie: s=2`. (a) Both are sent on the next request. (b) Turn the toggle on and quit: `$WISP_DATA_DIR/cookies/<id>.json` exists with mode 0600, holds `p` and no `s`. (c) Relaunch with the same `WISP_DATA_DIR`: `p` is sent and `s` is not. (d) Toggle off and confirm: the file is gone. Record the results in the PR.
- [ ] AC9 (42-11) The collection id is cached in `Workspace` per open collection. On open it is read with `read_collection_id` on the background executor (no write). If there is none, the first Send (or toggle-on) creates it with `ensure_collection_id` on the background executor, then submits. Every resolve takes the same lock as `HistoryJob::open` (its `RESOLVE` static hoisted to one module-level static). `send` and the render path call none of `ensure_collection_id`, `read_collection_id` or `collection_partition` on the UI thread.
- [ ] AC10 (42-12, 41-5) Decision, accepted: the partition is the committed collection id. Two copies or clones of a collection share history, OAuth2 tokens, the in-memory jar and `cookies/<id>.json`, as they already share history. Keying by id plus canonical root would split the in-memory jar from the cookie file and history, which are keyed by id alone. A pure helper `cookie_jar_partition(id, root)` returns the id string, or the canonical root path only when the id cannot be read or created; a unit test covers both. Its doc comment states that copies share, and the PR lists this as an accepted residual for the scribe.
- [ ] AC11 (task-102 NOTE) The base's `CookieFile::save` re-checks `is_enabled` under its writer lock (93f5167); if it does not, stop and report. All `save`/`enable`/`disable` calls for one collection run as background jobs with at most one in flight (AC3), so a debounced save cannot overtake a disable. The `load` symlink race and a symlinked `cookies/` parent are not fixed here (the data dir is per-user, 0700); the PR lists them as residuals.

## Checks
cargo writes no JUnit XML here, so tests are `command` checks.
- `app-unit` command — AC2, AC3, AC6, AC10
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-107; test "$(cargo test -p wisp-app --locked cookie_jar -- --list 2>/dev/null | rg -c ': test$')" -ge 6 && cargo test -p wisp-app --locked`
  (new tests live in `ui::cookie_jar` or have `cookie_jar` in their name)
- `lint` command — all
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-107; cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `no-deps` command — size
  run: `git diff --exit-code main...HEAD -- Cargo.lock`
- `wiring` read — AC1, AC4, AC5, AC7, AC9, AC11
  how: flush-then-load-once order on switch with no clear; the off path confirms before delete; no value reaches a label or a log; no collection-id read or write on the UI thread, and one shared resolve lock; one cookie-file job in flight; the PR lists the AC10 and AC11 residuals.
- `hand` read — AC8
  how: the PR lists (a)-(d) with the observed results, run with a temp `WISP_DATA_DIR`.

## Out of scope
- Editing a cookie's value or adding cookies by hand.
- More than one collection open at once (wave 3).
- Fixing `CookieFile::load`'s symlink race or the symlinked `cookies/` parent (storage, a follow-up).
- `crates/app/src/cookies.rs` and `ui/response/cookies.rs` (the response Set-Cookie view).
- Any change to `crates/core` or `crates/storage`.
