# 135 — Scaffold `wisp-cli` with `inspect` and the boundary check

**Repo:** .
**Depends on:** 129
**Owns:**
- crates/cli/** (new crate, package and binary `wisp-cli`)
- scripts/check-boundaries.sh (new)
- .github/workflows/ci.yml (one step in the `check` job, Linux leg only)
- Cargo.lock (the `wisp-cli` package entry only)

## Goal
Add the read-only console binary `wisp-cli` (no GPUI, no network, hand-rolled
argument parsing) with `inspect`, versioned JSON through dedicated DTOs, and a
script that enforces the crate boundaries in CI. `validate` and `explain`
land in 2D once tasks 130, 132 and 133 have merged.

## Context
- docs/plan/specs/2026-11-14-wisp-0-3-0.md "Feature 4": Commands and output, Crate and module layout, Argument parsing, Redaction and read-only rules (all 8), Boundary check (items 1-2), Tests
- crates/storage/src/collection.rs:432 `open_collection`, `Node`, `RequestMeta`; environment.rs:84 `list_environments`; request.rs `load_request`
- Cargo.toml:8-22 release profile (`panic = "abort"`); members glob `crates/*` already covers the crate
- .github/workflows/ci.yml:14-50 `check` job
- Worktree `wt-new.sh . 135 cli-scaffold-inspect`; `CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-135`

## Acceptance
- [ ] AC1 `crates/cli/Cargo.toml` depends only on `wisp-core`, `wisp-storage`, `serde`, `serde_json` (workspace versions; `tempfile` allowed as a dev-dependency); `[[bin]] name = "wisp-cli"`; no `windows_subsystem` attribute. The root `Cargo.toml` is unchanged and `Cargo.lock` gains only the `wisp-cli` entry.
- [ ] AC2 Layout: `main.rs`, `args.rs`, `dto.rs`, `output.rs`, `cmd/inspect.rs`. Hand-rolled parser over `std::env::args_os` for `validate <collection>`, `inspect <collection> [--env <name>]`, `explain <collection> <request-path> [--env <name>]`, `--json`, `--version`, `--help`, `--flag=value` and `--`. A table-driven test covers every form and every usage error.
- [ ] AC3 Exit codes: 0 success; 2 usage error (including `validate` and `explain`, which print "not available in this build" until 2D); 3 collection unreadable.
- [ ] AC4 `inspect` prints the collection (id if present, format, counts), environment names, and the tree up to 2,000 nodes (path, kind, name, method, URL origin and path with no query and no userinfo, auth kind, header names, valid flag) with `truncated: true` beyond; every name is capped at 200 characters. No variable, header or body value is printed.
- [ ] AC5 `--json` prints one document `{"schema":"wisp-cli/1","command":"inspect",...}`. Every serialisable type is in `dto.rs`; `output.rs` emits only through a sealed `Dto` trait implemented in `dto.rs` only.
- [ ] AC6 No `unwrap`, `expect` or `panic!` in `crates/cli/src` (crate-level `deny` of `clippy::unwrap_used` and `clippy::expect_used`); output goes through `writeln!`; `BrokenPipe` exits 0; no tracing subscriber is installed.
- [ ] AC7 Canary test through the real binary (`CARGO_BIN_EXE_wisp-cli`): a fixture with a secrets-file value, a plain variable `api_token`, an `Authorization` header literal and a keychain-mode environment; for `inspect` in text and JSON the canary strings are in neither stdout nor stderr. Fixtures are written by the test into a tempdir (no CRLF-sensitive checked-in files).
- [ ] AC8 Read-only test: a read-only fixture copy, all commands run, file tree and mtimes unchanged; a collection without an id is reported without one and not given one.
- [ ] AC9 Bounded: a 2,001-node fixture reports `truncated: true` and exactly 2,000 nodes.
- [ ] AC10 `scripts/check-boundaries.sh` (POSIX shell, `grep` not `rg`) fails if `cargo tree -p <c> -e normal,build --target all --all-features --locked` for `wisp-core`, `wisp-storage` or `wisp-cli` contains `gpui`, `gpui-component`, `gpui-platform`, `gpui-pre`, `gpui-pre-platform` or `winresource`; or if `crates/cli/src` contains any of the path segments `net::`, `TcpStream`, `UdpSocket`, `process::Command`, `Command::new`, `File::create`, `OpenOptions`, `fs::write`, `fs::remove`, `#[path`, `include!`, `include_str!`, `include_bytes!`, `reqwest`, `tokio`, `Engine`, `keychain::`, `SecretStore`, `ensure_collection_id`, `atomic_write` or `create_dir` (names adjusted to the real APIs per Decision 2026-11-15; the bare word `keychain` is allowed); or if any `grep` or `cargo` invocation errors (exit status ≥2, e.g. a missing `crates/cli/src`). The source check is a tripwire alongside the dependency check, not a proof. It passes on the branch head; the PR shows it failing on a local injected violation of each kind.
- [ ] AC11 `ci.yml` runs the script in the `check` job on the Linux leg only; no other CI change.

## Checks
- `cli` command — AC2-AC9
  run: `cargo test -p wisp-cli`
- `boundaries` command — AC10
  run: `scripts/check-boundaries.sh`
- `suite-and-lint` command — AC6
  run: `cargo test --workspace --locked && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `manifest` command — AC1
  run: `git diff --exit-code main...HEAD -- Cargo.toml crates/core crates/storage crates/app`
- `dto-and-ci` read — AC1, AC5, AC11
  how: only `dto.rs` derives `Serialize`; the lockfile diff is one package entry; the `ci.yml` diff is one Linux-gated step.

## Out of scope
- `validate`, `explain`, environment secrets mode and names, assertion counts in `inspect`: 2D.
- Release packaging, `release.yml`, signing, the CLI size step: 1M (task 136) and 3B.
- Any `crates/core`, `crates/storage` or `crates/app` file.

## Decisions
- 2026-11-15, orchestrator, code review round 1 of PR #80. Technical tightening of the boundary check within the spec's intent; recorded so the spec can be amended at record.
  - (a) The dependency check runs `cargo tree -p <c> -e normal,build --target all --all-features --locked`, so platform-gated and optional GPUI/`winresource` dependencies are caught.
  - (b) The source check matches path segments, not import lines: it fails on any of `net::`, `TcpStream`, `UdpSocket`, `process::Command`, `Command::new`, `File::create`, `OpenOptions`, `fs::write`, `fs::remove`, `#[path`, `include!`, `include_str!`, `include_bytes!`, `reqwest`, `Engine`, `keychain::`, `SecretStore`, `ensure_collection_id` in `crates/cli/src`. Adjust names to the real APIs; the list's intent is no network, no process spawning, no filesystem writes, no code pulled from outside `crates/cli/src`, no keychain/secret-store access, no collection-id writes. It stays a tripwire alongside the dependency check, not a proof.
  - (c) The bare word `keychain` is no longer forbidden (2D must print the secrets mode `keychain` and "keychain: not loaded by the CLI"); only `keychain::` paths are.
  - (d) The script fails if `grep` or `cargo` errors (exit status ≥2), e.g. a missing `crates/cli/src`.

## Review round 1 — findings (HEAD dda2e98)
- [P2] `scripts/check-boundaries.sh:9` `cargo tree` misses platform/feature-gated deps — settled by Decision (a).
- [P2] `scripts/check-boundaries.sh:23-29` fixed-string grep bypassed by grouped imports, renames, `#[path]`, `include!` — settled by Decision (b).
- [SPEC] `scripts/check-boundaries.sh:23` bare `keychain` conflicts with spec :264 — settled by Decision (c).
- [P3] `scripts/check-boundaries.sh:24` grep error passes silently — settled by Decision (d).
- [P3, follow-up, 🔒] `output.rs:26` text mode prints untrusted names raw (terminal escape / forged lines) — schedule in 2D.
- [P3, follow-up] `inspect.rs:180` environment list uncapped.
- [P3, follow-up] `inspect.rs:106-109` invalid count after the node cap.
- [P3, follow-up] `args.rs` no `NotUtf8` usage error.
- [NOTE] Read-only test doesn't check exit codes.
- [NOTE] `inspect` loads bodies it never prints.
- [NOTE] JSON `method` uses `Debug`.

## PR #80 review round 1 (user, HEAD dda2e98)
80-5 and 80-6 (boundary script) are already fixed at 03489df. Lows/Nits are follow-ups.
- [80-1, Medium] `crates/cli/tests/cli.rs:131` Windows CI red: `read_only_and_no_id_added` compares `DirEntry::metadata` mtimes, stale for directories on Windows. Use `fs::metadata(path)`.
- [80-2, Medium, 🔒] `inspect.rs:38` `safe_url` keeps userinfo when the URL has no `://` (`u:p@h/x`, `//u:p@h/x`).
- [80-3, Medium, 🔒] `output.rs:26` text output writes untrusted names/paths/URLs raw (ANSI/OSC, forged lines, bidi); escape them.
- [80-4, Medium] `inspect.rs:106` `collection.invalid` undercounts when truncated: emit null / "unknown (truncated)".
- Follow-ups (not fixed): 80-7 denylist misses vault::, ops::, save_; 80-8 symlinked folders escaping root; 80-9 parser table lacks NotUtf8; 80-10 BrokenPipe untested; 80-11 NodeInfo implements Dto; 80-12 inspect loads whole requests; 80-13 test names; 80-14 read-only test leaves undeletable tempdir.

## Review round 2 — findings
(reviewed HEAD 7c2f625)
- [P2] crates/cli/src/cmd/inspect.rs:38-46 — 80-2 only partly fixed. `safe_url` still prints userinfo when the scheme is followed by one slash, three slashes, or backslashes: `http:/u:p@h/x`, `http:///u:p@h/x`, `http:/\u:p@h/x` print unchanged. The WHATWG parser (reqwest::Url::parse, used by the engine at crates/core/src/engine.rs:697) reads all three as userinfo `u:p` at host `h`.
