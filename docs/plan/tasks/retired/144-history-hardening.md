# 144 — Harden the history store: eviction, lock, clear race and stored secrets (H1)

**Repo:** .
**Depends on:** none (131 and 137 merged)
**Owns:**
- crates/storage/src/history.rs
- crates/storage/tests/history.rs

## Goal
Close the H1 theme of the 0.3.0 roadmap: evicting an entry never deletes body
files a surviving entry uses, a stalled or symlinked lock file cannot hang or
redirect an append, clear and append cannot leave orphans, and secrets that
older builds wrote, or that ride in redirect headers, are not kept.

## Context
- docs/plan/roadmap.md register I, row H1; docs/plan/specs/2026-11-14-wisp-0-3-0.md Amendment 1 (H1)
- docs/plan/tasks/99-hardening-0-2-0.md:28 (AC8/AC9 are regression tests only), :49 AC8, :50 AC9, :58 AC17, :61 AC48, :63 AC50
- docs/plan/PLAN.md:182 62-1 (old builds' secrets stay until eviction), :183 62-2 (Location URLs stored)
- crates/storage/src/history.rs:614 `open`, :648 `lock_file`, :668 `append`, :791 `evict`, :893 `clear`, :95 `redact_url`, :578 `redact_credential_headers`
- The app calls `open`, `append`, `list`, `load`, `clear`; it handles an `append` error with a notice already (crates/app/src/workspace.rs `append_history`), so public signatures stay as they are.
- Worktree: `./.claude/scripts/wt-new.sh . 144 history-hardening`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-144`. Run every command in the foreground; leave nothing running.

## Acceptance
- [ ] AC1 (99/AC50) `evict` skips deleting body files of any id still present in the kept entries. Test: an index whose oldest and newest lines share an id, eviction of the oldest forced; `load` succeeds for the survivor and its body file exists.
- [ ] AC2 (99/AC17) The lock is taken with a bounded wait (at most 5 s); on timeout `append` returns `Err` and writes nothing. Test: another handle holds the lock; `append` returns `Err` within the bound and the index is unchanged.
- [ ] AC3 (99/AC48, unix) A lock path that is a symlink is refused (`O_NOFOLLOW` or an lstat check before open); `append` returns `Err`. Test: the lock path symlinks to a file outside the dir; `Err`, and the target's bytes and mtime are unchanged.
- [ ] AC4 (99/AC8) Test: `clear` and `append` in parallel threads, repeated; afterwards the bodies dir holds no file the index does not list.
- [ ] AC5 (99/AC9, unix) Test: with the dir read-only so the index rewrite fails, `load` succeeds for every entry `list` returns.
- [ ] AC6 (62-2) `Location`, `Content-Location` and `Refresh` response header values pass through `redact_url` before storage. Test: a `Location` with a credential-named query parameter and userinfo stores neither value.
- [ ] AC7 (62-1) Opening a store scrubs, once, entries written before the current redaction rules: stored request copies and headers are re-run through the current redaction and rewritten atomically; a marker records that the scrub ran so later opens skip it. Test: an index and `.req.json` written in 0.2.0 shape holding a credential header value and a Location token; after `open` neither value is in any file under the store dir, and a second `open` rewrites nothing (mtimes unchanged).
- [ ] AC8 No public signature in history.rs changes; no secret value in any new error message or log line; full suite, fmt and clippy pass.

## Checks
- `history` command — AC1-AC7
  run: `cargo test -p wisp-storage --test history 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. [1-9]'`
- `history-lib` command — AC1-AC7 (inline tests, if any are added)
  run: `cargo test -p wisp-storage history::`
- `api-stable` read — AC8
  how: `git diff main...HEAD -- crates/storage/src/history.rs` changes no `pub fn` signature or public type; new errors name paths and causes, not values.
- `suite-and-lint` command — AC8
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`

## Out of scope
- H2 items: AC49 `mask_bare_refs`, AC12 uncapped request body (0.4.0).
- Any app change (`ui/history.rs` and `workspace.rs` belong to 147 and the serial queue).
- The 137 follow-up about the vacuous canary scan (not scheduled).

## PR #94 review round 1 — findings

- [P2] history.rs redact_url: scheme-relative (`//`, `\\`, mixed) userinfo kept credentials in URLs and Location/Content-Location/Refresh. Fixed via `authority_start`, Refresh `url=` split.
- [P2] history.rs scrub_legacy: bodies/ enumeration errors ignored but marker written. Fixed: enumeration errors propagate; marker only after full success.

## Review round 2 — findings (reviewed HEAD 3995ac8)

- [P2] `crates/storage/src/history.rs` `redact_url_headers` (Refresh): only a literal `url=` was found, so scheme-relative userinfo survived in `0;URL='//u:p@h'`, `0; url="//u:p@h/"`, `0; URL = //u:p@h`, `0; url= //u:p@h`, `0;//u:p@h`. Fix per HTML refresh parsing; add each input to the tests.
- [P2] `crates/storage/tests/history.rs` `append_times_out_when_the_lock_is_held` asserted `elapsed() < 5s` (wall-clock asserts are banned, `docs/conventions.md:106`). Remove the clock; keep the error and unchanged-index asserts.

Follow-ups (not this round):
- P3: `authority_start` ignores single-slash special schemes (`https:/u:p@h/` kept).
- P3: no-scheme `a@b` / `mailto:a@b` Location values are mangled (orchestrator decision later).
- P3: global `APPEND_LOCK` queues appends across collections (N x 5 s).

## Review round 3 — findings (reviewed HEAD 25d60b2)
- [P2] redact_refresh returned the value unchanged without `;`/`,`; whitespace separators leaked.
- [P2] first `;`/`,` cut inside the URL (`0 url=https://alice:secret@h/a;b`).
- [P2] no whitespace-separator Refresh tests.
- NOTE: text after a closing quote, a second `url=`, `0; url //alice:secret@h` kept credentials.
Decision: redact userinfo at every authority start in the value (every `://`, `//`, `\\` run, and after `url`[ws][=]); credential-free values unchanged.

## Review round 4 — findings (user P1, reviewed HEAD fd31cb9)
- P1: `redact_refresh` kept credential-named query pairs (`Refresh: 0; url=https://example.com/?access_token=secret`), on append and in legacy scrub. Fixed in 19e2fc2/82f2bca; code-reviewer P2 (query cut at `'`, `;`, whitespace) fixed in 33e6c75.

## Review round 5 — findings (user review, reviewed HEAD before 33e6c75)
- P2 (already fixed in 33e6c75): Refresh query cut at `;` (`0; url=https://h/?x=1;2&access_token=secret`).
- P2: single-slash special-scheme URLs skip authority detection (history.rs:96-113). `Location: https:/alice:secret@example.com/` stores both credentials. WHATWG accepts this form for special schemes.
Fixed in 6160fbf.

## Review round 6 — findings (code-reviewer, reviewed HEAD 6160fbf)
- P2 (older than this branch, same class): ASCII tab/CR/LF inside the scheme or slash run hides the authority; WHATWG and the `url` crate delete them. Leaks: Location `https:\t/alice:secret@h/`, `https:/\t/alice:secret@h/`, `ht\ttps:/alice:secret@h/`; Refresh `0; url=https:\t/alice:secret@h/`.
Fixed in 4bf4084 (+ 9aca60a non-vacuous CR test).

## Review round 7 — findings (user review, reviewed HEAD 9aca60a)
- P2: leading ASCII whitespace/C0 before the scheme hides the authority (history.rs:109-123). Location/Content-Location ` https:/alice:secret@h/` stores credentials, on append and in legacy scrub. WHATWG strips leading and trailing C0 control or space.
Fixed in ccf415c (with 327c57f query-name decoding, 0f1b1c6 scheme-specific backslash rules).

## Review round 8 — findings (code-reviewer, reviewed HEAD 0f1b1c6)
- P2 (regression from 0f1b1c6): `authority_start` (history.rs:122-133) opens a `file:` authority only after exactly `//`; `file:\\alice:secret@h/`, `file:/\alice:secret@h/` in Location/Content-Location/request URL are now stored unredacted (redacted at 9aca60a). Refresh still redacts them. Fix: treat `file` like the special-scheme branch when slashes(rest) >= 2.

## Outcome
Merged 2026-11-15 as PR #94 (54f1141) and 2026-11-16 as PR #103 (f63f16e, the round-8 `file:` backslash fix). AC1-AC8 met; the review rounds above are the record of what each found. Open items are in PLAN "Follow-ups from task 144"; the summary is in HISTORY "History hardening, H1 (task 144)".
