# History

What was built, newest first. Append-only. Only `scribe` edits this file.

**Read this before redesigning anything.** The `Cost` line on each entry is the
point of the file — it is where the sessions that already tried the obvious
approach tell you what happened.

Do not load this file to find out whether something exists. It grows without
bound. `HISTORY-INDEX.md` carries one row per entry; scan that, then grep this
file for the exact heading it names. Every entry added here gets its index row
in the same commit.

<!--
## YYYY-MM-DD — <what landed>
<Two or three sentences: what it does now that it did not before.>
**Cost:** <what was hard, what was tried and abandoned, what not to retry.>
-->

## 2026-10-29 — Minimal core Engine and CI/release workflows
wisp-core now has an Engine that owns a small tokio runtime and one shared rustls reqwest client. `Send` and `Cancel` commands go in, and each request ends in exactly one `Completed` or `Failed` event on an `async_channel`. Errors are classified from the source chain, and the serde types are round-tripped through `serde_json`, a dev-dependency only. Separately, `ci.yml` runs fmt, clippy and tests on ubuntu and macOS, and `release.yml` builds four targets, with x86_64-apple-darwin native on `macos-26-intel`. Every macOS job installs and verifies the Metal Toolchain, actions are pinned to SHAs, and the Linux apt list comes from zed's `script/linux` at the gpui-pre zed-rev.
**Cost:** Task 04 needed three review rounds and a spec amendment. Round 1 found that `classify` keyword-searched the whole error chain including reqwest's URL text, so a request to `/tls` was reported as `Tls`; classify from the source chain, never the URL. It also found two tests that could not fail: the serde test only checked trait bounds at compile time, and the "no Completed after cancel" check passed because the 5 s mock delay outlasted a 100 ms wait. The serde finding led the user to amend the task to allow `serde_json` as a dev-dependency (one root `Cargo.toml` line, one `Cargo.lock` line). Round 2 found the cancel test still passing, because dropping the engine aborts the task and closes the channel, so keep the engine alive and use server-side delays with a sentinel request. It also found the pinned version written as `"1"` instead of `"1.0.154"`. Round 3 found an unreachable `panic!` arm that broke clippy. Do not use client sleeps to wait for state, and do not drop the engine to prove nothing arrives. Task 05's code comment claimed the Intel macOS runner was paid, which nobody had checked; `macos-26-intel` is a standard runner, so the x86_64-apple-darwin job runs natively rather than cross-compiling. Still unverified: the Linux and Windows jobs and a real green `release.yml` run, which cannot happen until the repo is pushed. `libstdc++-14-dev` matches the Ubuntu 24.04 branch of zed's script and needs re-checking when `ubuntu-latest` moves. Each task was squashed to one commit on main. Task 01's file was retired here; its work landed in 0e87d3a.

## 2026-10-29 — Cargo workspace scaffold and trimmed CLAUDE.md
The repository now has a Cargo workspace of wisp-core, wisp-storage and wisp-app (bin `wisp`), all `publish = false`, with the M0 dependency set locked in `Cargo.lock`. GPUI is pinned by exact crates.io version with no git sources: gpui-component =0.8.1, gpui-pre =0.3.10 (exposed as `gpui`) and gpui-pre-platform =0.3.10 (exposed as `gpui-platform`). It builds with `cargo build --workspace --locked`, passes fmt and clippy -D warnings, and has no openssl-sys or native-tls in the graph. Separately, `CLAUDE.md` is under its 50-line cap, with the role descriptions moved to `docs/workflow.md`.
**Cost:** The first attempt pinned gpui and gpui-component by git rev, which forced an old gpui-component (0.5.2): current gpui-component uses the versioned gpui-pre snapshots, so a rev pin of both gave two gpui sources, and a `[patch]` to the same URL is rejected by cargo. Do not go back to git revs. Attempt 1 also failed at verify because gpui-pre ships no platform backend (only `Application::with_platform`); gpui-pre-platform is a separate crate that has to be added, and the `wayland`/`x11` features on gpui-pre are no-ops, so they are enabled on the platform crate instead. macOS uses precompiled Metal shaders (no `runtime_shaders`), which needs Xcode's Metal Toolchain, a separate Xcode download; CI (task 05) installs it. The three task-02 commits were squashed into one so the stale git-rev rationale in the first commit does not survive. Task 03 needed one review round, for a commit-subject prefix.
