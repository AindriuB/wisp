# 168 — History hardening and a read-only history reader (2C, H2)

**Repo:** .
**Round:** 0
**Depends on:** 161, 165
**Owns:**
- crates/storage/src/history.rs
- crates/core/src/redact.rs
- crates/storage/tests/history.rs

## Goal
Close H2: history masking is linear, stored request bodies are capped, the
credential-name heuristic stops matching ordinary words, and response header
names no longer share the request headers' `redacted` namespace. Add
`HistoryStore::open_read_only` so `snapshot` can list recent entries without
migrating, locking or writing anything.

## Context
- docs/plan/specs/2026-11-17-wisp-0-4-0.md "Feature 7" H2 row (:418), Feature 5 `recent` (:372-374), Waves 2C (:469).
- docs/plan/tasks/99-hardening-0-2-0.md AC12 :53, AC15 :56, AC49 :62 (line refs are 0.2.0; `mask_bare_refs` is now `history.rs:540`).
- docs/plan/PLAN.md:264 (62-n1), :265 (62-n2).
- crates/storage/src/history.rs:25 `BODY_CAP`, :540 `mask_bare_refs`, :871 `open` (migrates, creates lock and marker files), :993 `lock_file`, :1035-1048 append cap, :1214 `list`.
- crates/core/src/redact.rs:5-35 fragments, `is_credential_name`, `contains_credential_fragment`; users: `storage/src/import/mod.rs`, `storage/src/history.rs`, `core/src/engine.rs`, `core/src/resolve/report.rs`.
- Worktree: from the main checkout, `./.claude/scripts/wt-new.sh . 168 history-hardening`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-168`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 (AC49) `mask_bare_refs` scans forward over variable-grammar characters only and then requires `}}`: one linear pass with no per-`{{` allocation. Test: a 1 MiB JSON body of repeated `{{` is stored (no wall-clock assert).
- [ ] AC2 (AC15) Restoring masked refs is a single pass. Test: 10,000 refs in a 1 MiB body round-trip correctly (linearity proved by reading the code).
- [ ] AC3 (AC12) The request body in `.req.json` is cut to `BODY_CAP` with a `truncated` flag (serde default false). Test: a 2 × `BODY_CAP` request body is stored cut with `truncated: true`; an old `.req.json` without the flag loads.
- [ ] AC4 (62-n1, final rule per Amendment 4) A name is a credential when any of the 17 `SECRET_FRAGMENTS` occurs as a substring of an alphanumeric token (lowercased, no camelCase, digit or acronym split), unless the whole token is on the allow-list `bypass`, `compass`, `design`, `signal`, `assignment`. Table test in `redact.rs`: `X-Bypass-Cache`, `X-Assignment-Id`, `design`, `signal`, `compass` are not credential names or fragments; every existing row still holds; the run-together names (`apikey`, `accessToken`, `jsessionid`, `OAuth2Token`, `JWTToken`, `githubtoken`, `passcode`, `Authentication`, `webhooksignature`, `awscredentials` and others) are credential names. History test (`h2_` prefix): query names `%GG%74oken=secret`, `ab%GG%74oken=secret` and `%G%74oken=secret` are stored redacted. The original and Amendment 1 and 3 wordings (token boundaries, strong suffixes) were superseded; their text is in Amendments 3 and 4 below.
- [ ] AC5 (62-n2) Redacted response header names no longer land in the names the app treats as redacted request parts (`HistoryEntry.redacted`, read on resend at `crates/app/src/workspace.rs:1042`, `:1226`); they are kept apart in an additive, serde-default form. Test: a request with an empty-valued `X-Foo` header and a response with a redacted `X-Foo` header gives an entry whose request-side list has no `X-Foo`; a 0.3.0 index line still loads. The app's existing resend tests pass unchanged.
- [ ] AC6 `HistoryStore::open_read_only(dir) -> Option<ReadOnlyHistory>` with `list(limit)` never migrates, takes no lock and never creates, renames or writes a file. With no current scrub marker it returns an empty list and a "history not yet migrated; open Wisp once" note. Tests: (a) an unmigrated dir gives the note and its listing, sizes and mtimes are identical before and after; (b) a migrated store lists its newest entries up to `limit` and nothing in the dir changes.
- [ ] AC7 A test outside Owns that fails because of AC4 is not edited: stop and report it. No file outside Owns changes; fmt, clippy and the workspace suite pass.

## Checks
- `history-h2` command — AC1, AC2, AC3, AC4, AC5, AC6
  run: `cargo test -p wisp-storage --locked --test history h2_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([8-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
  (prefix the new tests in `tests/history.rs` with `h2_`.)
- `redact` command — AC4
  run: `cargo test -p wisp-core --lib --locked redact:: 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. ([3-9]|[1-9][0-9]+) passed; 0 failed; 0 ignored'`
- `scope` command — AC7
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/storage/src/history.rs$' -e '^crates/core/src/redact.rs$' -e '^crates/storage/tests/history.rs$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC7
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `linear` read — AC1, AC2, AC6
  how: the reviewer confirms both passes are single forward scans and that `open_read_only` calls no write, rename, create or lock API.

## Out of scope
- The `delivery` field (161); `snapshot` itself (173); importer behaviour (170), even where AC4 changes which names importers treat as secret.

## Wave stopped — step 2 — orchestrator-routed review finding at e78aca2
- [P2] crates/core/src/redact.rs:36-46: AC4's token-boundary rule stops redacting run-together credential names the old substring match caught (`jsessionid`, `JSESSIONID`, `sessiontoken`, `newpassword`, `adminpassword`, `OAuth2Token`, `JWTToken`, `githubtoken`, `oauthtoken`, `bottoken`, `passcode`), so their values reach history in plaintext (tension with spec :9). The `drop_malformed` decode rescues only `%GG%74oken`; `ab%GG%74oken=secret` and `%G%74oken=secret` now leak. Widening the heuristic (more joiners, suffix-matching strong fragments, digit/acronym splits) is a choice the task did not make, and it also changes importer secret detection.
Held: [P3] history.rs:412-426 Form/Multipart bodies are not capped.

## Amendment 1 (user, 2026-11-17)
Reason: the [P2] under "Wave stopped" above. Pure token-boundary matching stopped redacting run-together credential names the old substring match caught, and the encoded query names `ab%GG%74oken=secret` and `%G%74oken=secret` leaked to history.
- AC4 widened (user decision): letter/digit and acronym splits, plus suffix matching for the strong fragments `token`, `password`, `passwd`, `secret`, `session`, `passcode`. The table test gains the run-together names; the non-credential rows stay.
- The 0.2.0 encoded-query-name guarantee is restored and tested (`h2_` test; `history-h2` minimum raised to 8).
- This also changes which names the importers treat as secret, through `redact.rs`. Accepted; importer code stays out of scope (170).
- Fix round also carries the held [P3]: Form and Multipart request bodies are capped with `truncated` too (AC3).

## Wave resumed — user accepted the orchestrator recommendations (Amendment 1)

## Review round 1 — findings
Reviewed HEAD e78aca24ad5efbfe194e3056f4e53b68cc3ba323 (full branch, after Amendment 1).
- [P2] crates/core/src/redact.rs:36-46 — `token_is_credential` matches a fragment only as a whole token or next to a listed joiner. It has no suffix match for the strong fragments and no letter/digit or acronym split. Run-together credential names are therefore not redacted, and the amended AC4 says they must be.
  route: implementer — amended AC4 and Amendment 1 specify the widening.
  input: query `?jsessionid=abc` or `?newpassword=abc`, or header `JWTToken: abc` or `githubtoken: abc`, then append → the value is stored in `.req.json` and the index in plaintext. The same happens to `ab%GG%74oken=secret`: `decode_query_name` gives "ab%GGtoken" or "abtoken", and neither tokenises to a credential.
- [P2] crates/storage/src/history.rs:413-416 — `cap_request_body` returns false for `Body::Form` and `Body::Multipart`, so those bodies are stored at full size with `truncated: false`. Amendment 1 adds them to AC3.
  route: implementer.
  input: a Form body whose fields total 2 × BODY_CAP, then append → `.req.json` holds all 2 MiB and `truncated` is absent.
- [P2] crates/storage/tests/history.rs:1840-1973 and crates/core/src/redact.rs:127-150 — the tests do not cover the amended contract: only 7 `h2_` tests (the `history-h2` check needs 8 or more); no `h2_` test stores `%GG%74oken=secret`, `ab%GG%74oken=secret` or `%G%74oken=secret` and asserts they are redacted; the `redact.rs` table has none of the amended run-together rows (`jsessionid`, `JSESSIONID`, `sessiontoken`, `newpassword`, `adminpassword`, `OAuth2Token`, `JWTToken`, `githubtoken`, `oauthtoken`, `bottoken`, `passcode`).
  route: implementer — AC4 test list as amended and the Checks minimum.
  input: run the current suite against HEAD → it passes while `jsessionid=abc` is stored in plaintext.

## Wave verified — 0e4325e4858368502ee17b17294a37b75509b7c2

## Wave stopped — step 5 — new flake on Windows CI at PR #131 head 02326a8
`h2_read_only_unmigrated_gives_note_and_changes_nothing` (crates/storage/tests/history.rs) failed on windows-latest in run 44201380433 (pull_request) and passed on windows-latest in run 44201364399 (push), same head 02326a81. The before/after snapshot differs only in the mtime of `history/<id>` (dir): 134394329929050772 vs 134394329929070956 (equal to `index.jsonl`'s mtime) — likely NTFS updating a directory's last-write time lazily after setup, not a write by the read-only reader. Not in known-flakes.md. Options: accept as a new known flake; or a fix round that leaves directory mtimes out of the snapshot on Windows (or settles the mtimes before snapshotting).

## External review — findings
Key sha256:28d5a8a48ca7 · pasted · reviewed head 02326a81dc6d610f391afb0618dd3642974adfbe (PR #131) · round 2

Reviewed [PR #131](https://github.com/AindriuB/wisp/pull/131). One actionable finding:
[P2] Protect history resend from truncated request bodies — `crates/storage/src/history.rs:1228`
The new body cap truncates stored requests, but opening history discards `StoredEntry.request_truncated`. A 2 MiB text request can therefore reopen as a 1 MiB draft and be resent without warning. Form requests can likewise resend shortened or emptied fields. Carry the flag into the resend flow and require repair or explicit acknowledgement.
Overall impact: Changes requested. The storage hardening can cause history resend to silently submit incomplete payloads.
Reviewed target: `main`
* Base: `f9e79b89d23684c8b1a130df855ea59770b50327`
* Head: `02326a81dc6d610f391afb0618dd3642974adfbe`

## Wave stopped — step 7 — external P2 (round 2) touches files outside Owns
The history-resend fix needs `request_truncated` carried into the app's reopen/resend flow, with a warning or acknowledgement before a truncated body is resent. That is crates/app (outside Owns), so it is the user's call: amend Owns to add the app resend path, or record a residual and open a follow-up task. Settle it together with the Windows flake above.

## Wave resumed — user: fix round for the Windows flake (leave directory mtimes out of the snapshot); external P2 accepted as a residual, follow-up task for the app resend warning
Accepted residual (user, 2026-11-17): history drops `request_truncated` on reopen, so a capped body can be resent incomplete without warning. The fix lives in crates/app (outside Owns) and goes to a new follow-up task; 168 stays storage-only.

## Review round 3 — findings
Reviewed head 0e4325e4858368502ee17b17294a37b75509b7c2 (CI on PR #131 head 02326a8).
- [P2] crates/storage/tests/history.rs:1968 (`h2_read_only_unmigrated_gives_note_and_changes_nothing`, and the same snapshot helper in `h2_read_only_migrated_lists_newest_up_to_limit`) — the before/after snapshot compares directory mtimes; on Windows (NTFS) a directory's last-write time can update lazily after setup, so the test fails intermittently with no write by the reader.
  route: implementer — user decision: leave directory mtimes out of the snapshot (keep file path, size and mtime, and the directory listing), so AC6's "changes nothing" is still asserted for files.
  input: windows-latest, run 44201380433: `history/<id>` mtime 134394329929050772 before vs 134394329929070956 after → assertion failed; run 44201364399 at the same head passed.

## Wave verified — 6e0cb519e9986cbda16bf2c76a2dd28486fbb587

## Outcome
Merged at 6e0cb51, then fix rounds to d577cdb (PASS + APPROVE; AC1-AC7 met). History masking and restore are single forward passes, request bodies (JSON, text, Form and Multipart) are capped at `BODY_CAP` with a `truncated` flag, and response header names are kept apart from the request-side `redacted` list. `redact.rs` ends on the Amendment 4 rule: substring matching of the 17 fragments over alphanumeric tokens, with a whole-token allow-list (`bypass`, `compass`, `design`, `signal`, `assignment`) for known false positives; token-boundary, acronym and suffix matching were tried first and removed because each kept failing open. `HistoryStore::open_read_only` lists without migrating, locking or writing. The external P2 (history resend of truncated bodies) is an accepted residual, covered by task 178. Follow-ups are in PLAN.md.

## Amendment 3 and the final rounds (summary)
(Superseded by Amendment 4.) Amendment 3 widened AC4 so run-together names stay redacted: a token ending in `key`, `pass`, `pwd`, `auth`, `sig`, `jwt` or `cookie`, with an optional plural `s`, is a credential; the whole tokens `bypass` and `compass` are exempt; `code` joins the joiner list.  The full text is below. Round 5 found one P2: the AC4e test asserted the value `abc` was absent from every history file, but `abc` can appear in a random hex uuid in `index.jsonl`. Attempt 6 replaced it with `zeroth-secret`, which contains the fragment `secret` and so left the Text body leg unproven; the fix changed the value to a non-hex, non-fragment one (`zz-hunter`).

## Wave verified — 63c28f0cb68d380a202abe1684b48b2bb3e0b3d4

## Amendment 4 and the last rounds (summary)
Amendment 4 (user decision after a third external review) replaced the token-boundary rule with substring matching: credential names match when any of the 17 fragments occurs in an alphanumeric token, as before 168, plus a whole-token allow-list (`bypass`, `compass`, `design`, `signal`, `assignment`) for the known false positives. Amendment 3's suffix and joiner rules were removed. Round 8 found that `old_true_names_still_match` did not cover every true row of `credential_names`, and Attempt 9 fixed two h2 tests that still used fragment-bearing or hex values (`secret`, filler `a`) by switching to `zz-hunter` and `z`. `x-vercel-protection-bypass` is stored in clear because `bypass` is allow-listed (accepted under AC4g).

## Wave verified — d577cdbfcc2556b1bb154f100a34092703d10a50

## Amendment 3 — AC4 strong suffixes
User decision (131-1 on PR #131, 132-1 on PR #132): run-together credential names stay redacted. This adds to AC4; everything AC4 already matches still matches. Line refs are to `crates/core/src/redact.rs` at 6e0cb51.
- AC4a Suffix rule. In `token_is_credential` (:42-59), a lowercase token (after the existing trailing-digit trim, :43) is a credential when it ends with `key`, `pass`, `pwd`, `auth`, `sig`, `jwt` or `cookie`, optionally followed by one `s` (`masterkeys`, `rootcookies`). Fail safe: words such as `monkey`, `hockey`, `turkey` now count as credential names, and that is accepted.
- AC4b `code` joins `JOINERS` (:29-32), so `authcode` (and, fail safe, `keycode`) matches through the existing joiner rule. Reason: `authcode` does not end in a fragment, so the suffix rule alone cannot catch it.
- AC4c Exceptions. The whole tokens `bypass` and `compass` are not credentials under AC4a; they are pinned false in `ordinary_words_are_not_credentials` (:159-170). The exception list holds only those two tokens and applies only to the suffix rule. No other existing false row flips: `X-Assignment-Id`, `design`, `signal`, `page`, `Content-Type`, `region` and `""` match no suffix.
- AC4d The `credential_names` true table (:127-152) gains `privkey`, `adminpass`, `rootpwd`, `masterkey`, `authcode`, `rootpass` and `X-Masterkey` as `true`. Every existing true and false row is unchanged, and `ordinary_words_are_not_credentials` keeps all five names false for both `is_credential_name` and `contains_credential_fragment`.
- AC4e History test (`h2_` prefix, `tests/history.rs`): query `?rootpass=hunter2`, header `X-Masterkey: abc` and Text body `masterkey=abc` are stored redacted in `index.jsonl` and `.req.json` (131-1's failing input).
- Checks: AC4a-AC4d are covered by the existing `redact` check and AC4e by `history-h2`. The minimum counts are unchanged.
- Owns is unchanged: the fix touches only `crates/core/src/redact.rs` and `crates/storage/tests/history.rs`. No test outside Owns asserts that a name with these suffixes is non-credential. `engine.rs:1214-1217` checks only `accept*`, `content-type` and `user-agent`, which stay false. AC7 still applies: if a test outside Owns fails, stop and report it.
- Out of scope: importer secret detection (`import/mod.rs:455`) widens with this rule. That is expected, and no importer change belongs here (170).

## Amendment 4 — substring matching with an allow-list
User decision (131-1, third external review): token matching keeps failing open, so the match goes back to the pre-168 substring rule (`f9e79b8:crates/core/src/redact.rs:26-35`), which fails safe, plus a closed allow-list for the known false positives. This replaces AC4's matching rule (token boundaries, Amendment 1's splits and suffixes) and Amendment 3's AC4a-AC4c. AC4's history test, AC4d's true rows and AC4e stay. Line refs are to `crates/core/src/redact.rs` at 63c28f0.
- AC4f Substring rule. `is_credential_name(name)` and `contains_credential_fragment(text)` lowercase the input and return true when any of the 17 `SECRET_FRAGMENTS` (:5-23, unchanged) occurs as a substring of a token that is not allow-listed (AC4g). A token is a maximal run of alphanumeric characters, lowercased; there is no camelCase, digit or acronym split, no digit trim and no plural stripping. Percent-decoding of query names stays as it is (`history.rs:141-174`, both `drop_malformed` modes). Every name the token rule at 63c28f0 caught is caught (neither rule treats an allow-listed token as a credential), and every name the pre-168 rule caught is caught unless its only fragment-bearing tokens are allow-listed.
- AC4g Allow-list. A `const ALLOWED_WORDS` in `redact.rs` holds exactly: `bypass`, `compass`, `design`, `signal`, `assignment`. A token equal (case-insensitive, whole token) to an entry contributes no match; the same token elsewhere in a run does not count (`bypassCache`, `signals`, `assignmentId` are credential names, fail safe). Each entry is a word from an existing false row that the substring rule would flip: `bypass`/`X-Bypass-Cache`/`compass` (`pass`), `design`/`signal`/`X-Assignment-Id` (`sig`). The other false rows (`page`, `Content-Type`, `region`, `""`, body `hello`) contain no fragment and need no entry. Adding an entry is a spec change.
- AC4h Removed: `EXTRA_WORDS` (:26), `JOINERS` (:29-32), `STRONG_FRAGMENTS` (:36-38), `SUFFIX_FRAGMENTS` (:42), `SUFFIX_EXCEPTIONS` (:45), `token_is_credential` (:49-71) and `split_run` (:74-93). Substring matching subsumes all of them: every word they match contains a fragment (`authorization` has `auth`, `passphrase` has `pass`). Fail safe as before: `monkey`, `hockey`, `surpass`, `keycode` are credential names.
- AC4i Tables. `credential_names` (:138-174) keeps every row; it gains `Authentication`, `webhooksignature` and `awscredentials` as `true`. `ordinary_words_are_not_credentials` (:177-200) keeps all six names false for both functions and its true list. A new test `old_true_names_still_match` asserts `is_credential_name` and `contains_credential_fragment` are true for every name in the pre-168 tables (`Authorization`, `  X-Api-Key `, `access_TOKEN`, `client_secret`, `Set-Cookie`, `PASSWD`, `my_credentials`, `x-signature`, `session_id`, `x-sig`, `my_pass`, `db_pwd`, `id-jwt`, `bearer_x`, `DPoP`, `saml_assertion`) and for every `true` row of `credential_names`.
- AC4j Bodies. A Text or XML body (`history.rs:604`) is cleared when `contains_credential_fragment` finds a fragment in any non-allow-listed token (`masterkey=zz-hunter` cleared; `design signal` kept; `monkey` cleared). h2 test values stay non-hex and fragment-free (`zz-hunter`, not `abc` or anything containing a fragment); an h2 body meant to be stored intact must contain no fragment substring.
- AC4k History test (`h2_` prefix, `tests/history.rs`): header `Authentication: zz-hunter`, query `?webhooksignature=zz-hunter&awscredentials=zz-hunter` are stored redacted in `index.jsonl` and `.req.json`. AC4's three percent-encoded query rows and AC4e still pass.
- Owns unchanged (`crates/core/src/redact.rs`, `crates/storage/tests/history.rs`, `crates/storage/src/history.rs`). AC7 holds: the substring rule makes more names secret than 63c28f0 did, so a test outside Owns (importer, engine, report, app) that now fails is not edited; stop and report it.
- Checks: AC4f-AC4j are covered by the existing `redact` check and AC4k by `history-h2`. The minimum counts are unchanged.
