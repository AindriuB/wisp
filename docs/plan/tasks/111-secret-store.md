# 111 — SecretStore trait, store-mode environments and migration in storage

Amendment: module renamed to vault (deny rule) — `wisp_storage::vault`, files `crates/storage/src/vault.rs` and `crates/storage/tests/vault.rs`; type names unchanged.

**Repo:** `.`
**Depends on:** 109 (the size answer comes before any keychain implementation is fanned out; this task adds no dependency)
**Size impact:** none expected (no OS keychain code, no new dependencies; `no-deps` check)
**Owns:**
- crates/storage/src/vault.rs (new)
- crates/storage/src/environment.rs
- crates/storage/src/error.rs
- crates/storage/src/lib.rs (one `pub mod vault;` line)
- crates/storage/tests/vault.rs (new)

## Goal
Add the storage half of optional per-environment secret stores, without any OS backend. That half is a blocking `SecretStore` trait that does not assume an OS keychain, an in-memory store, an extensible `secrets = "<kind>"` environment mode, a loader that follows the fail-closed table below, and explicit migration between the file and a store. Task 114 adds the OS keychain store and task 115 wires the app. Wave 3 ships only `file` and `keychain`; a later version may add other stores (e.g. KeePassXC), and this seam must take them as additive variants without rework.

## Context
- docs/plan/specs/2026-11-09-wisp-0-2-0.md:67-76 — the keychain design (service `wisp`, account `<collection-id>/<env>/<var>`)
- crates/storage/src/environment.rs:95-230 — load and save today; `*.secrets.toml` is written 0600
- crates/storage/src/fs.rs:166 — `atomic_write_private`
- crates/storage/src/collection.rs:199-257 — `CollectionId` (`[A-Za-z0-9_-]{8,64}`, so never contains `.`)
- crates/core/src/request.rs:372-390 — `Environment` and its redacting Debug. Unchanged: storage returns secrets in `Environment.secrets`.
- docs/conventions.md#toml, #logging — writes go through `toml_edit`; secret types get a redacting Debug and a `debug_redacts_*` test
- KeePassXC on Linux can already act as the Secret Service provider, so task 114's Linux backend must speak Secret Service generically and not assume GNOME Keyring.

## Design (decided here)
- **Store kind.** `<env>.toml` gains `secrets = "<kind>"` and `secret_names = ["a", "b"]`. `StoreKind` is a `#[non_exhaustive]` enum, `File | Keychain` in this wave, parsed by one `StoreKind::parse(&str) -> Option<StoreKind>` with `as_str()` as its inverse; absent means `File`. A future `"keepassxc"` is one new variant plus one parse arm. An unrecognised value is not an `Err`: it loads with status `Failed { UnknownStoreKind(String) }`, every name unresolved and the secrets file not read. It never falls back to plaintext. Both keys are committed and neither is secret.
- **No enumeration.** Names live in `secret_names` because stores offer no common way to enumerate; no code path lists store entries.
- **Trait.** `SecretStore: Send + Sync` has `probe() -> Result<(), SecretStoreError>`, `get`, `set` and `delete` on an account string. Any method may block for seconds or go over IPC (D-Bus, a socket, a browser-protocol pipe); callers already run them off the UI thread. Nothing in the trait or its docs assumes an OS keychain.
- **Errors.** `SecretStoreError` is a `#[non_exhaustive]` enum with at least `Unavailable { reason }` (store absent or not running), `Locked` (needs the user to unlock), `Denied` (user refused, or pairing revoked), `TooLarge` (value over the store's size cap; nothing stored) and `Other { code: Option<i64> }`. `UnavailableReason` is a `#[non_exhaustive]` enum with at least `Portable`, `NotConfigured`, `NoService` and `TimedOut`; task 114 returns the last two. Messages are fixed strings plus non-secret codes, never a value.
- **D1 mapping, for every store.** Only `Unavailable` at probe with reason `Portable`, `NoService` or `NotConfigured` means "absent" and may fall back to the file. `Unavailable { reason: TimedOut }` at probe is treated like `Locked` (user decision 2026-11-12: the store may exist but did not answer). `Locked`, `Denied`, `Other` (and any error from `get`, including `Unavailable` after a good probe) fail closed: names unresolved, file not read, nothing copied.
- **Store lookup.** The loader takes `stores: &dyn SecretStores`, a trait with `fn get(&self, kind: StoreKind) -> Option<&dyn SecretStore>`. A known kind with no store (not built or not configured) is treated as `Unavailable { reason: NotConfigured }`. The app constructs stores; storage never does.
- **Per-store state (seam only, not implemented).** A store that needs one-time pairing (e.g. a KeePassXC association key) gets that state at construction, from the app, not through the trait. Such state is itself a secret: it may be kept in another `SecretStore` under the reserved account prefix `.store/<kind>/<name>`, which no `CollectionId` can produce. This task adds only `store_state_account(kind, name)` and a doc comment; no pairing code.
- `SecretValue` wraps the string. It has a hand-written Debug that prints `<redacted>` and no Display.
- `MemoryStore` (public, for app and storage tests) can be made to return any `SecretStoreError` from its probe, or from its next call.
- **Addressing.** `secret_account(id, env, var)` returns `<id>/<env>/<var>`. `refresh_token_account(id, env, grant, token_url, client_id, scope)` returns `<id>/<env>/oauth2/<grant>/<token_url>/<client_id>/<scope>`. In each component `%` becomes `%25` and `/` becomes `%2F`, so two different keys never map to one account. The account string is the store-neutral address: a URL-keyed store maps it as `wisp://<account>` without changing it.

## Load table (`load_environment_with(root, name, id: Option<&CollectionId>, stores: &dyn SecretStores)`)
| mode | probe | read | secrets come from | status |
|---|---|---|---|---|
| file | not called | — | `<env>.secrets.toml` | `File` |
| unknown kind | not called | — | none: names are unresolved; file not read | `Failed { UnknownStoreKind }` |
| store | `Unavailable` with reason `Portable` / `NoService` / `NotConfigured`, or no store for kind | — | `<env>.secrets.toml` if present | `FellBackToFile { reason }` |
| store | `Unavailable { reason: TimedOut }` | — | none; file not read | `Failed { reason: TimedOut }` |
| store | `Locked` / `Denied` / `Other` | — | none; file not read | `Failed { reason }` |
| store | Ok, id is None | — | none: names are unresolved | `Failed { NoCollectionId }` |
| store | Ok | every name ok or missing | store; a missing name is unresolved | `Store { kind }` (plus `file_ignored: bool` if a secrets file exists) |
| store | Ok | any read errors | none for any name; file not read | `Failed { reason }` |
The load never returns `Err` for a store failure or an unknown kind; only TOML and IO errors are `Err`. Values are never copied from a store into a file except by an explicit `migrate_to_file`.

## Acceptance
- [ ] AC1 Every row of the load table has a test that uses `MemoryStore`.
- [ ] AC2 `migrate_to_store(root, name, id, kind, store)` works in order: it writes each file secret to the store and reads it back to compare. It then writes `secrets = kind.as_str()` and `secret_names` with `toml_edit`, keeping comments, and only then deletes `<env>.secrets.toml`. If anything fails before the delete, the file and mode are unchanged. Entries created in this call are deleted again, and entries that existed before are left alone. Tests cover success, and a failure at each step.
- [ ] AC3 `migrate_to_file` works in order: it reads every name, and any read error aborts with nothing changed. It then writes `<env>.secrets.toml` with `atomic_write_private` (0600), writes mode `file` and removes `secret_names`, and only then deletes the store entries. A delete failure after that is returned as a warning, and the file stays. Tests cover success and a read failure.
- [ ] AC4 `set_secret` and `delete_secret` for a store-mode environment write the store first and then update `secret_names`. A store failure leaves `secret_names` unchanged. Neither ever writes a secrets file.
- [ ] AC5 Two keys that differ only by `/` or `%` in a component produce different accounts (test). `store_state_account` output starts with `.store/` and no `secret_account` output does (test).
- [ ] AC6 There are `debug_redacts_secret_value` and `debug_redacts_memory_store` tests over `{:?}` and `{:#?}`. A sentinel value appears in no `SecretStoreError` message, and in no `tracing` field added by this task (read).
- [ ] AC7 `load_environment` and `save_environment` behave exactly as before for file-mode environments, and their existing tests pass unchanged. `save_environment` on a store-mode or unknown-kind environment never writes a secrets file. It writes the plain vars and keeps `secrets` and `secret_names` verbatim.
- [ ] AC8 Extensible kind and error mapping, unit tests: `StoreKind::parse` round-trips `"file"` and `"keychain"` through `as_str`; `secrets = "keepassxc"` (or any unknown value) with a secrets file present yields `Failed { UnknownStoreKind }` and no secret from the file; a fake store whose probe returns each of `Unavailable` (`Portable`, `NoService`, `NotConfigured`), `Unavailable { TimedOut }`, `Locked`, `Denied` and `Other` yields `FellBackToFile` for the first three and `Failed` with no file read, secrets file present, for the rest; a fake whose `get` returns each error yields `Failed`.

## Checks
cargo writes no JUnit XML here, so tests are `command` checks.
- `vault` command — AC1-AC6, AC8
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-111; test "$(cargo test -p wisp-storage --locked --test vault -- --list 2>/dev/null | rg -c ': test$')" -ge 22 && cargo test -p wisp-storage --locked`
- `env-unchanged` command — AC7
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-111; cargo test -p wisp-storage --locked --lib environment::`
- `no-deps` command — size
  run: `git diff --exit-code main...HEAD -- Cargo.lock crates/storage/Cargo.toml`
- `lint` command — all
  run: `export CARGO_TARGET_DIR=/Users/Andrew/workspace/wisp/target/wt-111; cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
- `no-leak` read — AC6
  how: no value reaches a log, an error string or a derived Debug. The only path from store to file is `migrate_to_file`.
- `store-neutral` read — AC8
  how: the trait, its docs and `SecretStoreError` name no OS keychain; `StoreKind` and `SecretStoreError` are `#[non_exhaustive]`; no code enumerates store entries; no unknown-kind, `TimedOut` or non-`Unavailable` path reads the secrets file.

## Out of scope
- Any OS keychain backend or probe (task 114). App wiring, banners and UI (task 115).
- Any KeePassXC store, pairing flow, protocol code or dependency; only the seams above.
- The core `RefreshTokenStore` adapter. Only the account helper lives here.
- Changing core `Environment`, `Scope` or `resolve()`.
