# 109 — Spike: keychain backend size and Linux headless behaviour

**Repo:** `.`
**Depends on:** none
**Size impact:** this task is the measurement; no code merges
**Owns:** (throwaway branch `task/109-keychain-spike`; the PR is opened as a draft and closed unmerged, so these paths never reach main and do not collide with other tasks)
- crates/app/src/main.rs
- crates/app/Cargo.toml
- crates/storage/Cargo.toml
- crates/storage/src/lib.rs
- crates/storage/src/keychain_spike.rs (new)
- Cargo.lock
- .github/workflows/spike-109.yml (new, only if a Linux CI run is used instead of docker)

## Goal
Answer, with measurements, whether an OS keychain backend fits the 0.2.0 size budget on Windows and Linux, whether it works or fails cleanly on headless Linux without a Secret Service, and which crate or API each platform should use. Task 114 is written against the recommendation and is confirmed or amended from this report before it is fanned out. Time box: one day.

## Context
- docs/plan/specs/2026-11-09-wisp-0-2-0.md:21, :67-76, :113 — the keychain design and the "zbus over 1 MB means Linux ships unsupported" rule
- Main at 9a3e318: Windows 15,136,256 B (task 108, measure.yml run 42890358687); headroom 363,744 B to 15,500,000. Tasks 112 and 115 still need part of that headroom.
- Planner recon, to confirm or refute: these are already in `Cargo.lock` through GPUI, so the marginal cost may be near zero:
  - Linux: `oo7` 0.6.0 (features `async-std`, `native_crypto`) and `zbus` 5.19 via `gpui-pre-linux`; `zbus` also via `accesskit_unix` and `ashpd`.
  - Windows: `windows` 0.62 with `Win32_Security_Credentials` via `gpui-pre-windows` (CredReadW, CredWriteW, CredDeleteW).
  - macOS: `security-framework` 3.7 (has `passwords`) via `rustls-platform-verifier`.
  - GPUI itself exposes `cx.read_credentials`, `cx.write_credentials` and `cx.delete_credentials` on all three platforms (`gpui-pre-0.3.10/src/app.rs:1890-1925`). They need an `App` context, return GPUI `Task`s, and label entries for Zed: Linux label `zed-github-account`, Windows target `zed:url=<url>`.
- `keyring` is not in the lock. Its Linux backends are `dbus-secret-service` (libdbus, rejected by the spec) or a zbus-based one; check whether it duplicates zbus at another major version.

## Acceptance
- [ ] AC1 For each candidate, the PR gives the Windows release `binary_bytes` from a `measure.yml` run on this branch and the delta against 15,136,256 B. Candidates: (a) GPUI credentials API; (b) direct `windows` 0.62 Cred* calls from `wisp-storage`; (c) `keyring` with only its Windows-native feature. Each candidate is kept alive by a code path in `main.rs` behind `WISP_SPIKE_KEYCHAIN=<a|b|c>`, so LTO cannot drop it. One run may build several candidates in turn.
- [ ] AC2 The PR gives the same delta for the Linux x64 release binary for direct `oo7` use (matching GPUI's features exactly, never `openssl_crypto`) and for the cheapest `keyring` Linux option that does not use libdbus. It states whether either exceeds 1 MB.
- [ ] AC3 Headless Linux, run in a container or on a CI runner with no desktop session. For the recommended Linux path, the PR records the result, the error kind and the time to fail in three setups: `DBUS_SESSION_BUS_ADDRESS` unset; a session bus with no Secret Service provider; and a locked gnome-keyring, if one can be set up within the time box. It records whether any setup hangs for more than 5 s or prompts.
- [ ] AC4 macOS: the PR states whether `security-framework` generic passwords (service and account) round-trip under a test service name. It states whether a rebuilt, unsigned or ad-hoc-signed binary prompts when it reads an item that an earlier build wrote.
- [ ] AC5 The PR states the Windows credential blob limit (2,560 bytes) and whether any planned value can exceed it: secret vars, and OAuth2 refresh tokens such as long JWTs.
- [ ] AC6 Recommendation. For each platform: the crate or API; where it lives (`storage` behind a trait, or `app` through GPUI); its call shape (blocking or async, and on which executor); how to probe availability without a prompt; and whether Linux ships supported in 0.2.0.
- [ ] AC7 Every spike entry uses service `wisp-spike-109`. The PR lists the entries it created and confirms that each was deleted. No entry under service `wisp` is read, written or deleted.

## Checks
- `measure` command — AC1, AC2
  run: `gh workflow run measure.yml --ref task/109-keychain-spike` then `gh run download <id> -n measure-<target> -D <scratch>` per target; the PR cites the run URLs.
- `report` read — AC1-AC7
  how: each question has a measured answer with its run URL or command. The recommendation follows from the figures. No secret value, real account name or token appears in the PR or a log.

## Out of scope
- Production code: the branch is never merged. The real backend is task 114.
- Multiple collections, the core refresh-token seam (110), and any UI.
- Changing GPUI or forking it to rename its credential labels.
