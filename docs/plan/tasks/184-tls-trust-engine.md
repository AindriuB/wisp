# 184 — Trust private CAs in the engine (OS store and CA file)

**Repo:** .
**Round:** 0
**Depends on:** none
**Owns:**
- crates/core/src/client.rs
- crates/core/src/engine.rs (`EngineConfig`, `Engine::set_tls_trust` and the `ClientCache` hand-off only)
- crates/core/src/trust.rs (new)
- crates/core/src/lib.rs (`mod trust;` and re-exports only)
- crates/core/tests/tls.rs
- scripts/measure/results/v0.5.0-trust-size.json (new)

## Goal
HTTPS servers whose chain ends at a private CA can be verified. There are two
ways: through the OS trust store (rustls-platform-verifier, already linked by
reqwest), and through a user-supplied PEM CA file. Both are configured
engine-wide and can be changed at run time. This task is core only; the GUI
and CLI wiring are tasks 190 and 191.

## Context
- docs/plan/specs/2026-11-19-wisp-0-5-0.md "Feature 5": the mode table, the loader rules, and the default, which is open question 2. This brief assumes the recommendation, `system: true`; the orchestrator amends AC1 if the user answers otherwise.
- crates/core/src/client.rs:46-55: today's bundled-only build.
- reqwest 0.13.5 at `~/.cargo/registry/src/*/reqwest-0.13.5/src/async_impl/client.rs`: `tls_certs_only` :1902, `tls_certs_merge` :1877, the verifier choice :750-783.
- rustls-platform-verifier 0.7.1 `src/verification/others.rs:104-108`: Linux errors when the OS store is empty, so the bundled roots stay as extra anchors.
- crates/core/src/engine.rs:30-60 `EngineConfig`, :145-180 `start_with_clock`. crates/core/src/auth/oauth2/manager.rs:467 shows OAuth2 token requests share `ClientCache`.
- crates/core/tests/tls.rs:42-80 `serve_tls`, which uses rcgen and is already a dev-dependency.
- `measure.yml` needs no approval. Wait with one foreground `gh run watch <id> --exit-status`.
- Worktree: `./.claude/scripts/wt-new.sh . 184 tls-trust-engine`; `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-184`. Foreground commands only; never `pkill` or `killall`.

## Acceptance
- [ ] AC1 `wisp_core::TlsTrust` is public, `Clone + Debug`, and holds `system: bool` plus the extra CA certificates (DER). Its `Debug` prints counts, never bytes. `Default` is `system: true` with no extra certificates.
- [ ] AC2 `TlsTrust::load_pem_file(path)` returns the certificates or a `TrustError`, and refuses:
  - a missing path;
  - a non-regular file (directory or FIFO);
  - a file over 1 MiB, checked on metadata and while reading;
  - a file with no `CERTIFICATE` block;
  - any block that does not parse as a certificate;
  - any private-key block.

  Error text names the path (escaped) and the reason, never file content. No new crate appears in `Cargo.lock`.
- [ ] AC3 `EngineConfig` gains `tls: TlsTrust`. `Engine::set_tls_trust(TlsTrust)` drops every cached client, so the next send builds with the new trust. Requests in flight are unaffected and cookie jars are kept.
- [ ] AC4 Clients are built per the spec table: `system` off uses `tls_certs_only(bundled ∪ extra)`; `system` on uses `tls_certs_merge(bundled ∪ extra)`. `verify_tls = false` still accepts any certificate. OAuth2 token requests use the same trust.
- [ ] AC5 Tests in `crates/core/tests/tls.rs`, prefix `trust_`, against a private CA. rcgen generates the CA; the leaf has SAN `127.0.0.1` and `localhost`, the serverAuth EKU, and is valid from now−1h to now+30d. Cases:
  - (a) system off with no file fails with `ErrorKind::Tls`;
  - (b) system off with the CA file completes with 200;
  - (c) system on with the CA file completes;
  - (d) system on with no file fails with `Tls`;
  - (e) `set_tls_trust` turns (a) into (b) on one engine;
  - (f) `verify_tls = false` still completes against a self-signed server in both modes.
- [ ] AC6 Loader tests, prefix `trust_load_`, one per AC2 refusal plus a two-certificate bundle that loads both certificates.
- [ ] AC7 `measure.yml` is dispatched on the pushed task branch and completes green. The results file records the run id, the head, the per-target GUI bytes, and the Windows GUI delta against 15,616,000 B. If the Windows GUI exceeds 15,900,000 B, stop and report.
- [ ] AC8 No file outside Owns changes. fmt, clippy and the workspace suite pass on all three CI OSes.

## Checks
- `trust-tests` command — AC5, AC6
  run: `cargo test -p wisp-core --locked --test tls trust_ 2>&1 | tee /dev/stderr | grep -E 'test result: ok\. (1[3-9]|[2-9][0-9]) passed; 0 failed; 0 ignored'`
- `no-new-crates` command — AC2
  run: `git diff --exit-code main...HEAD -- Cargo.lock`
- `measure-run` command — AC7
  run: `gh run view "$(jq -r .run_id scripts/measure/results/v0.5.0-trust-size.json)" --json conclusion -q .conclusion | grep -x success`
- `scope` command — AC8
  run: `test "$(git diff --name-only main...HEAD | grep -v -e '^crates/core/src/client.rs$' -e '^crates/core/src/engine.rs$' -e '^crates/core/src/trust.rs$' -e '^crates/core/src/lib.rs$' -e '^crates/core/tests/tls.rs$' -e '^scripts/measure/results/v0.5.0-trust-size.json$' | wc -l | tr -d ' ')" = 0`
- `suite-and-lint` command — AC8
  run: `cargo test --workspace --locked && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `trust-shape` read — AC1, AC3, AC4
  how: the reviewer checks that bundled roots are present in both modes, that a load error can never widen trust, that `Debug` prints no certificate bytes, and that the cache is cleared on `set_tls_trust`.

## Out of scope
- `settings.toml`, palette commands and the TLS hint (190); `wisp-cli` flags (191).
- Client certificates (mTLS), CRLs, pinning, and per-request trust.
- Changing `ClientSettings` or `PreparedRequest`.
