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

## 2026-10-29 — Cargo workspace scaffold and trimmed CLAUDE.md
The repository now has a Cargo workspace of wisp-core, wisp-storage and wisp-app (bin `wisp`), all `publish = false`, with the M0 dependency set locked in `Cargo.lock`. GPUI is pinned by exact crates.io version with no git sources: gpui-component =0.8.1, gpui-pre =0.3.10 (exposed as `gpui`) and gpui-pre-platform =0.3.10 (exposed as `gpui-platform`). It builds with `cargo build --workspace --locked`, passes fmt and clippy -D warnings, and has no openssl-sys or native-tls in the graph. Separately, `CLAUDE.md` is under its 50-line cap, with the role descriptions moved to `docs/workflow.md`.
**Cost:** The first attempt pinned gpui and gpui-component by git rev, which forced an old gpui-component (0.5.2): current gpui-component uses the versioned gpui-pre snapshots, so a rev pin of both gave two gpui sources, and a `[patch]` to the same URL is rejected by cargo. Do not go back to git revs. Attempt 1 also failed at verify because gpui-pre ships no platform backend (only `Application::with_platform`); gpui-pre-platform is a separate crate that has to be added, and the `wayland`/`x11` features on gpui-pre are no-ops, so they are enabled on the platform crate instead. macOS uses precompiled Metal shaders (no `runtime_shaders`), which needs Xcode's Metal Toolchain, a separate Xcode download; CI (task 05) installs it. The three task-02 commits were squashed into one so the stale git-rev rationale in the first commit does not survive. Task 03 needed one review round, for a commit-subject prefix.
