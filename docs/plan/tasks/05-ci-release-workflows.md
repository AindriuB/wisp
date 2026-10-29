# 05 — Add GitHub Actions CI and release builds for four targets

**Repo:** `.`
**Depends on:** 02
**Owns:**
- .github/workflows/ci.yml
- .github/workflows/release.yml

## Goal
Every push and pull request runs format, lint and tests; a tag or manual
dispatch builds release binaries of `wisp` for macOS arm64, macOS x64,
Windows x64 and Linux x64 and uploads them as artifacts. This is the only
evidence M0 will have that the Windows and Linux builds work.

## Context
- docs/plan/specs/2026-10-29-http-client-v1.md:16-23 — platforms; one binary, no installer
- docs/conventions.md:30-37 — fmt and clippy gates
- docs/plan/tasks/02-scaffold-cargo-workspace.md — toolchain file, release profile, binary name
- zed-industries/zed `script/linux` at the pinned rev — the apt packages GPUI needs to build on Ubuntu

## Acceptance
- [ ] `ci.yml` triggers on `push` and `pull_request`; runs `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` on `ubuntu-latest` and `macos-latest`.
- [ ] `release.yml` triggers on `push` of tags `v*` and on `workflow_dispatch`, with a matrix of exactly four entries: `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`.
- [ ] Every build step uses `cargo build --release --locked -p wisp-app --target <target>`.
- [ ] Each matrix entry uploads one artifact named `wisp-<target>` containing the binary (`wisp.exe` on Windows) and nothing else.
- [ ] The toolchain comes from `rust-toolchain.toml` (no second pinned version in YAML); targets are added with `rustup target add`.
- [ ] Linux jobs install the GPUI build packages from the pinned zed rev's `script/linux` list; the YAML comment names that rev.
- [ ] `x86_64-apple-darwin` is cross-compiled on an Apple Silicon runner unless an Intel macOS runner image is verified current on the day of implementation; the choice is stated in the commit body.
- [ ] All third-party actions are pinned to a full commit SHA with the version in a trailing comment.
- [ ] `actionlint .github/workflows/*.yml` exits 0 (installed at `/opt/homebrew/bin/actionlint`).
- [ ] Locally verifiable subset passes: `rustup target add x86_64-apple-darwin && cargo build --release --locked -p wisp-app --target x86_64-apple-darwin` succeeds on this Mac.
- [ ] **Not verifiable locally:** the Windows and Linux jobs, and any job actually running. The repo has no remote. The implementer reports this criterion as `UNVERIFIED — needs a push`; it is closed by the first green `release.yml` run once a remote exists, and that run's URL goes in the close-out. Do not mark it passed on the strength of `actionlint`.

## Out of scope
- Creating a GitHub Release, signing, notarization, installers, checksums.
- Measurement jobs (task 07 owns `.github/workflows/measure.yml`).
- Adding a remote or pushing.
- Any change to `Cargo.toml`, `Cargo.lock` or `rust-toolchain.toml`; if CI needs one, stop and report.
