# Wisp architecture

The shape of the system: what the pieces are, what talks to what, and where the
boundaries are. `explorer` and `architect` read this before inferring
structure from the filesystem, so keeping it honest saves every session a
search.

Detail and rationale live in the v1 design spec,
[`plan/specs/2026-10-29-http-client-v1.md`](plan/specs/2026-10-29-http-client-v1.md).
This file records only the stable shape.

## Purpose

A portable REST/HTTP client in Rust, in the spirit of Postman, Bruno and
Insomnia, for a developer working locally. Lightness is the reason the product
exists: existing clients are large and memory-heavy.

- **Portable.** One self-contained binary per platform. No installer, no
  registry, no system TLS library. It writes nothing outside the open collection
  folder, `settings.toml` and the per-user data dir (history, cookies).
- **Lightweight.** Low idle memory and a small binary. The numeric budgets are
  set from Milestone 0 measurements (see Budgets).
- **v1 platforms:** macOS Apple silicon, Windows x64, Linux x64
  (Wayland or X11; GPUI needs a Vulkan-capable driver).
- **License and version.** Apache-2.0 (`LICENSE`, `NOTICE`), version 0.2.0
  in `Cargo.toml` (0.3.0 is pending its tag, which must equal the workspace
  version), both set once in `[workspace.package]` and inherited by every crate.
  `THIRD-PARTY-NOTICES` is generated and committed (see
  `docs/conventions.md#dependencies`). The release targets are
  `aarch64-apple-darwin`, `x86_64-pc-windows-msvc` and `x86_64-unknown-linux-gnu`;
  Intel macOS is not released (dropped for 0.1.0).

## Components

A Cargo workspace of four crates under `crates/`. Port column omitted: nothing
here listens on a port.

| Component | Directory | Owns |
|---|---|---|
| `core` | `crates/core` | Request/response model, variable and path-param resolution, auth (Basic, Bearer, API key, Digest, AWS SigV4, OAuth2), the `Engine` (reqwest on tokio, rustls only), body store, in-memory cookie jars |
| `storage` | `crates/storage` | Reading and writing collections, environments and body files; file watcher; request history, cookie persistence and the per-user data dir; secret stores (OS keychain through `keyring`); curl, Postman and OpenAPI importers |
| `app` | `crates/app` | GPUI and `gpui-component` UI, including the workspace that holds up to 8 open collections, the per-user environment guard and the assertion editor; the binary |
| `cli` | `crates/cli` | `wisp-cli`, a read-only console tool (`validate`, `inspect`, `explain`); no GPUI, no engine, no secret store; see [`cli.md`](cli.md) |

The packages are `wisp-core`, `wisp-storage`, `wisp-app` (binary `wisp`) and
`wisp-cli` (binary `wisp-cli`); see `conventions.md#naming`.

## Repository topology

Single repo, this directory. Nothing is untracked or fetched separately, so
`git clone` obtains everything. `.claude/` is untracked on purpose: it holds
worktree scripts installed from the agent-kit repo, and `agents/` and
`commands/` are symlinked into `~/.claude`.

## How they talk

Dependencies point one way: `app` depends on `core` and `storage`; `storage`
may depend on `core` for the model types; `core` depends on neither. `cli` links
`core` and `storage` directly, as `app` does, and nothing links `cli`. These rules
and the CLI's read-only rule are checked by `scripts/check-boundaries.sh` (see
Boundaries); the 0.3.0 design is
[`plan/specs/2026-11-14-wisp-0-3-0.md`](plan/specs/2026-11-14-wisp-0-3-0.md).

### The Engine

`core` exposes a service-shaped `Engine`: commands in, events out.

```rust
let (engine, events) = Engine::start(EngineConfig::default());
let id = engine.submit(Command::Send(prepared));
engine.submit(Command::Cancel(id));
```

- Commands are `Send(PreparedRequest)`, `Cancel(RequestId)`, and, for OAuth2
  authorization-code login, `AuthorizeStart { .. }` and `AuthorizeCancel(RequestId)`.
  Events are `Started`, `Headers`, `Progress` (at most one per 100 ms),
  `Completed`, `Failed` (typed error) and `Cancelled`, plus `AuthUrl`,
  `AuthCompleted` and `AuthFailed` for an authorization. No token, code or
  verifier crosses the command or event boundary.
- Every command and event is plain data and `serde`-serialisable. No GPUI types
  and no locks appear in the API, so a socket transport (a daemon with clients)
  can be added later without changing them.
- The Engine owns a small tokio runtime of one or two worker threads. GPUI keeps
  its own executor; the split is hidden inside `core`, and the event channel is
  executor-agnostic.
- Response bodies are returned as a `BodyHandle` into a body store: in memory up
  to about 10 MB (configurable), spilled to a temp file above that. The UI reads
  pages and the body is freed when its tab closes.
- One `reqwest::Client` is kept per settings combination (TLS verify, proxy,
  redirect policy) for connection pooling. The cookie jar is per collection and
  in memory; saving it to the per-user data dir is an opt-in per collection.
- Redirects: Basic, Bearer and ApiKey requests use reqwest's redirect path,
  except that a streamed body on a 307/308 is followed by the engine too.
  For Digest and AWS SigV4 the engine follows redirects itself (up to 9),
  re-signs each same-origin hop, and drops all engine-added auth after the
  first cross-origin hop, so credentials and signatures never leave the origin
  they were made for.

### resolve() is pure

`resolve(&Request, &Scope) -> Result<PreparedRequest, ResolveError>` performs no
I/O. It applies variables, path params, inherited auth and headers, and
encoding. The Engine does only I/O on a `PreparedRequest`. The UI calls
`resolve` per keystroke for the URL preview and unresolved-variable marks, which
is why it must stay cheap and side-effect free.

### storage

A plain library (`wisp-storage`), tested without a window. It provides:

- `open_collection(path)`, which returns an ordered `CollectionTree` of folders
  and request metadata only (bodies load lazily; broken request files appear as
  `Node::Invalid`), and `scope_for`, which builds the folder chain for `resolve`.
- `load_request` and `save_request`, with sidecar bodies; `load`/`save` of
  `collection.toml` and `folder.toml` defaults; and environment list, load and
  save, with `*.secrets.toml` written 0600 and git-ignored.
- A `Watcher` emitting `Changed(path)` with 100 ms per-path coalescing. Consumers
  must re-check the disk.
- Request history in the per-user data dir under `history/<collection-id>/`
  (`index.jsonl` plus capped body files), with auth and secret-bearing header
  values redacted when an entry is made; and cookie persistence in
  `cookies/<collection-id>.json`, on only while that file exists.
- `[[assert]]` tables in request files (read and written with the request;
  an unrecognised kind is kept verbatim), a tolerant `validate` walk that
  reports every finding, and `environment_info`, which describes an environment
  (variable and secret names, store mode, the `suggest_confirm` hint) without
  opening a secret store or returning a value.
- `SecretStore` and an OS-keychain store (`keyring`), for environments that
  opt in with `secrets = "<kind>"`; `*.secrets.toml` stays the default.
- Importers for a curl command, a Postman v2.1 collection and an OpenAPI 3
  document (JSON or YAML), which report what they cannot convert as warnings.

Writes go through `toml_edit` and atomic file replacement, so comments and key
order survive a save. The fixture test in `crates/storage/tests/` proves the
whole path from disk through `resolve`.

## On-disk collection format

A collection is a folder of TOML files, similar to Bruno. Full examples are in
spec section 2.

```
my-api/
  collection.toml          format = 1, default vars, headers, auth
  environments/
    dev.toml               committed vars
    dev.secrets.toml       secret vars; *.secrets.toml is gitignored automatically
  users/
    folder.toml            optional: headers, auth, vars, order = [...]
    get-user.toml
    create-user.toml
    create-user.body.json  sidecar body
```

- **Identity is the file path** for requests and folders: no UUIDs in their
  files. From 0.2.0 `collection.toml` also carries a random collection `id`
  (a UUID, written once; user decision 2026-11-09, see
  `plan/specs/2026-11-09-wisp-0-2-0.md`), which keys the collection's data in
  the per-user data dir and survives moving or renaming the folder. Order comes
  from `order = [...]` in `folder.toml`; unlisted items sort alphabetically.
- **`{{var}}`** is a variable, resolved first. **`{name}`** is a path param,
  resolved second, and only in the path portion of the URL. Syntax follows
  Spring, OpenAPI and RFC 6570: several params per segment, `{id:\d+}` regex
  (warning only), and `{*path}` catch-all. Values are percent-encoded except
  for catch-all.
- **Variable precedence:** request, then nearest folder outward to the
  outermost folder, then environment (secrets over plain), then collection
  defaults. An unresolved `{{var}}` blocks sending and is never sent literally.
  Built-in dynamic vars in v1 are `{{$uuid}}` and `{{$timestamp}}` only.
- **Inheritance:** `auth.type = "inherit"` walks up folders to the collection.
  Headers merge down the chain and the nearer one wins.
- **Query params:** `[[params]]` is the single source of truth; the URL bar
  displays and edits them.
- **Versioning:** `collection.toml` carries `format = 1`.
- **Save model:** explicit save, with a dirty marker. No autosave.

## Settings location

`settings.toml` is located in this order: an absolute `WISP_DATA_DIR` override
(`settings.toml` directly in that directory); then beside the executable when a
file named `portable` is there; otherwise the OS config directory. A relative or
empty `WISP_DATA_DIR` is ignored. Apart from the per-user data dir
(history and cookies, from 0.2.0; see `plan/specs/2026-11-09-wisp-0-2-0.md`),
nothing else is persisted outside the collection folder.

The per-user data dir is chosen in `crates/storage/src/data_dir.rs`, in this
order: the `WISP_DATA_DIR` environment variable when it is an absolute path (a
relative value is ignored); `<exe-dir>/data` when the `portable` marker is
present; otherwise `wisp` under the OS data directory (macOS
`~/Library/Application Support`, Windows `%LOCALAPPDATA%`, other Unix
`$XDG_DATA_HOME` if absolute, else `~/.local/share`). Per collection it holds
`history/<id>` and `cookies/<id>.json`. Nothing is created until first use, and
directories are made 0700 on Unix.

## Guard and assertion data

Where each 0.3.0 piece of state lives:

- **Environment guard.** The choice to confirm before sending is per user, not
  per collection: `confirm_send` in the per-user `settings.toml`, a list of
  keys made of the collection id and the environment name. It is not written to
  the collection folder, so it is not shared through git. Because the key holds
  the environment name, a renamed environment loses its guard. The committed
  side is only a hint: `suggest_confirm = true` in `environments/<env>.toml`
  makes Wisp offer once per run to turn the guard on. The guard covers
  interactive Send; an OAuth2 Authorize is not guarded. It needs a collection
  id: while the id is still being read (or reading it failed) the guard fails
  open and a send goes through unasked (known residual 115-1, accepted by the
  user 2026-11-17; spec 2b).
- **Assertions.** `[[assert]]` tables in the request's own `.toml` file
  (`format = 1` is unchanged). They are declarative, evaluated after a send,
  report only, and never block a send. Expected values are literals.
- **Assertion results.** Only in history, as records of a label, a definition
  fingerprint and a state (pass, fail or skipped), never an expected or actual
  value; history also records the environment name. The request copy kept in a
  history entry has its assertions cleared.
- **No scripting.** The v1 rule stands: no scripting in v1. Assertions are data,
  not code.

## Boundaries that must not be crossed

- Only `crates/app` may depend on GPUI or `gpui-component`. `core`, `storage`
  and `cli` must build and test without them. `scripts/check-boundaries.sh`
  enforces this from `cargo tree`, and also fails when `crates/cli/src`
  mentions network, process, engine, keychain or file-write code, so the CLI
  stays read-only.
- `core`'s command and event types stay plain serialisable data: no GPUI types,
  no locks, no handles to runtime internals.
- `resolve` performs no I/O.
- No TLS other than `rustls`; no OpenSSL or native-tls anywhere in the
  dependency tree.
- Nothing writes outside the collection folder, `settings.toml` and the
  per-user data dir (history, cookies; `<exe-dir>/data` in portable mode)
  (body-store temp files are the one exception and must be removed on close).
- Secrets (`*.secrets.toml` and keychain values) never appear in logs or
  committed files. History stores no auth values (redacted; resend re-resolves
  against the current environment). Stored request and response bodies and a
  persisted cookie jar may hold session data, so the data dir is 0700/0600.

## Budgets

Measured by `scripts/measure/measure.sh` at the end of Milestone 0 and copied
from the committed records in `scripts/measure/results/`. RSS is in KiB as
recorded; the budget figures below are in MB (1 MB = 1,048,576 B, 1 MB RSS =
1,024 KiB).

| Target | Binary size | Idle RSS | RSS after one request | Renderer | Source commit |
|---|---|---|---|---|---|
| `aarch64-apple-darwin` | 16,660,480 B | 100,032 KiB | 103,296 KiB | hardware | `561807f` (`aarch64-apple-darwin.json`) |
| `aarch64-apple-darwin` (after task 16, M1 core) | 16,710,288 B | 99,232 KiB | 102,560 KiB | hardware | `2e8d74b` (`RENDERER=hardware scripts/measure/measure.sh`, not a committed record) |
| `aarch64-apple-darwin` (after task 21, M2 layout freeze) | 16,726,992 B | 69,216 KiB | 70,480 KiB | hardware | task 21 tester run (`RENDERER=hardware scripts/measure/measure.sh`, not a committed record) |
| `x86_64-apple-darwin` (dropped for 0.1.0) | 18,052,632 B (local cross-build on arm64) | not measured: RSS on `macos-26-intel` CI hung in the icon service (`ISIconManager`) | not measured: same | n/a | `561807f` (`x86_64-apple-darwin.json`) |

Before task 64: current-main CI baseline (task 63, `measure.yml` run 41607658851 on `0d450de`). Renderers are software on Linux and Windows, so these are not comparable with the hardware row and are not held to the budgets below. RSS and the memory fields are in KiB, idle / after one request.

| Target (CI) | Binary size | RSS | Other memory | Renderer |
|---|---|---|---|---|
| `aarch64-apple-darwin` | 19,181,056 B | 74,816 / 80,032 | n/a | not recorded |
| `x86_64-pc-windows-msvc` | 28,947,456 B | 42,784 / 52,180 | private bytes 35,108 / 41,628; private working set 9,024 / 13,112 | WARP software |
| `x86_64-unknown-linux-gnu` | 37,993,000 B | 175,192 / 172,864 | PSS 148,989 / 146,736; RssAnon 57,964 / 44,920 | llvmpipe (LLVM 21.1.8, 256 bits) under xvfb |
| `x86_64-apple-darwin` (dropped for 0.1.0) | 20,714,504 B | unmeasured: no `WISP_EVENT` completed within 30 s (long-standing runner hang) | n/a | n/a |

On Linux, RSS mostly counts shared llvmpipe driver pages; RssAnon is the closest figure to the app's own memory. On Windows, the private working set is the headline figure. These replace the task 09 skeleton figures (Windows 24,937,984 B, 39,600 / 45,940 KiB; Linux 35,134,344 B, 171,712 / 172,324 KiB; aarch64 16,660,480 B, 73,168 / 76,528 KiB).

After task 64 (release-profile shrink), `measure.yml` run 41631211960 on `10e3b3f`. These are the current figures. Memory fields are in KiB, idle / after one request. At that time the macOS x64 job failed in every run, baseline included, so its size is the only figure; that job was removed in task 71.

| Target (CI) | Binary size | RSS | Other memory | Change in size |
|---|---|---|---|---|
| `aarch64-apple-darwin` | 10,420,080 B | 73,152 / 77,728 | n/a | -45.7% |
| `x86_64-pc-windows-msvc` | 13,450,240 B | 41,560 / 50,016 | private working set 9,208 / 12,940 | -53.5% |
| `x86_64-unknown-linux-gnu` | 23,353,896 B | 167,640 / 163,652 | PSS 141,438 / 137,523; RssAnon 58,168 / 44,972 (llvmpipe) | -38.5% |
| `x86_64-apple-darwin` (dropped for 0.1.0) | 12,884,616 B | unmeasured (the job fails; no artefact) | n/a | -37.8% |

Task 77 made the Windows binary 13,630,464 B (static CRT, +1.34% over the 13,450,240 B above); it is within budget.

Release profile now (`Cargo.toml`, `.cargo/config.toml`): `panic = "abort"`; dependencies build at `opt-level = "z"` through `[profile.release.package."*"]`, while our `wisp-*` crates stay at 3; on Linux x64, `--gc-sections` and `--icf=all` are passed through rust-lld by `.cargo/config.toml`. Consequence: a panic anywhere, including in a spawned tokio task, ends the process; there is no unwinding and nothing recovers from a task panic. A `RUSTFLAGS` environment variable replaces the config's target rustflags, and GNU ld rejects `--icf=all`, so the Linux build needs lld.

### Set budgets

Set by the user on 2026-11-03 from the post-task-64 figures. Both Linux backends
(x11 and wayland) stay. The enforced copy is `scripts/measure/budgets.json`, keyed
by target triple; both measure scripts read it, and this table is its explanation.
1 MiB = 1,024 KiB.

| Target | Binary size | Idle memory | Memory after one request | Memory metric |
|---|---|---|---|---|
| `aarch64-apple-darwin` | at most 20 MiB (20,971,520 B) | at most 120 MiB | at most 128 MiB | RSS (`idle_rss_kib`) |
| `x86_64-pc-windows-msvc` | at most 16 MiB (16,777,216 B) | at most 120 MiB | at most 128 MiB | private bytes (`private_kib`) |
| `x86_64-unknown-linux-gnu` | at most 28 MiB (29,360,128 B) | at most 120 MiB | at most 128 MiB | RssAnon (`rss_anon_kib`) |

Windows uses private bytes because on WARP the working set leaves out GPU memory.
Linux uses RssAnon because llvmpipe dominates RSS and PSS in CI. CI renderers are
software, so figures are comparable only within one target.

Each result JSON carries a `budget` object (`binary_bytes`, `idle`, `after_request`,
each `{metric, limit, value, within}`) and `within_budget`. The scripts exit 3 when a
measured result is over budget and 1 when it is unmeasured. A run fails when arm64,
Windows or Linux is over budget or unmeasured. Intel macOS (`x86_64-apple-darwin`) was
dropped from 0.1.0 on 2026-11-03 (user decision): macOS 27 is Apple-silicon-only and the
x64 CI job hung in the runner's icon service in every run. `measure.yml` no longer builds it
and `budgets.json` has no entry for it; `results/x86_64-apple-darwin.json` stays as a
historical record. First enforced run: `measure.yml` run 41636483942 on `4ba28c1`; arm64,
Windows and Linux were measured and within budget (binary 10,420,080 / 13,450,240 /
23,353,896 B).

The Linux job also uploads three diagnostic artefacts, taken after the after-request sample
so they cannot change the measured figures. It runs under Xvfb at 1280x800x24.
`measure-<target>.png` is a screenshot of the X root window (ImageMagick `import`, wrapped in
`timeout 20`; set by `SCREENSHOT=<path>` in `measure.sh`, a failure prints one `screenshot: <reason>`
line and changes nothing else). `<logbase>.libs.txt` is the sorted, unique list of shared objects
in `/proc/<pid>/maps` of the running app. `measure-<target>.packages.txt` maps those to Debian
packages (`dpkg -S`, arch suffix stripped, `unowned: <path>` for files dpkg does not own) and
lists the binary's `readelf -d` NEEDED entries; it is the source of the README's Linux
runtime-library list. In CI the Mesa, LLVM and DRM packages come from the software Vulkan driver
and are not runtime requirements of a user's machine.

The JSON also carries `renderer_device_source`, which says where `renderer_device`
came from: `"app_log"`, `"vulkaninfo"` or null (null whenever `renderer_device` is
unset, and always on Windows and macOS). The `vulkaninfo` fallback may name a
different device than the one GPUI picked. A `budgets.json` that is malformed or
whose top level is not an object makes either script exit 2 with one stderr line,
before the app is launched. The `measure` job has `timeout-minutes: 45`.

### 0.3.0 limits: `wisp-cli` and the Windows GUI

The Windows GUI executable gate is 16,000,000 B and its soft line is 15,900,000 B
(user decision 2026-11-17, spec Amendment 2, which replaced the earlier 15.5 MB
gate and 15.4 MB soft line). The gate is enforced in `.github/workflows/release.yml`. The
soft line is a policy line, not a CI check: crossing it applies the spec's cut
order. `scripts/measure/budgets.json` keeps its 16 MiB (16,777,216 B) figure
unchanged.

`wisp-cli` has its own limit of 4,194,304 B per target, in
`scripts/measure/cli-budgets.json`; `release.yml` and `measure.yml` read it. The
limit is checked on the unsigned binary, so a signed `wisp-cli.exe` is a few KB
larger than the size checked.

Sizes measured at the wave 2 head (`measure.yml` run 43976706350 on `4769107`,
record `scripts/measure/results/wave-2-size.json`):

| Target | GUI binary | `wisp-cli` | CLI limit |
|---|---|---|---|
| `aarch64-apple-darwin` | 11,817,952 B | 1,415,600 B | 4,194,304 B |
| `x86_64-pc-windows-msvc` | 15,494,656 B | 1,631,232 B | 4,194,304 B |
| `x86_64-unknown-linux-gnu` | 26,181,000 B | 1,966,744 B | 4,194,304 B |

The CLI roughly tripled from the wave 1 figures (macOS 552,528 B); the cause has
not been investigated. The Windows GUI figure above (15,494,656 B) is over the
soft line and under the gate.

No change may be justified or rejected by reference to a number that is not in
this section.

## Release

`.github/workflows/release.yml` builds `aarch64-apple-darwin`, `x86_64-pc-windows-msvc` and `x86_64-unknown-linux-gnu` with the same steps and `Build` command as `measure.yml`, packages each with `scripts/release/package.sh`, and a final job writes `SHA256SUMS` and creates the release. `<v>` below is the `wisp-app` version from `cargo metadata`.

| Asset | Contents (inside `Wisp-<v>-<platform>/`) |
|---|---|
| `Wisp-<v>-macos-arm64.zip` | `Wisp.app/` (with `wisp-cli` at `Contents/MacOS/wisp-cli`, signed with the app and covered by its notarisation), `LICENSE`, `NOTICE`, `THIRD-PARTY-NOTICES` |
| `Wisp-<v>-windows-x64.zip` | `wisp.exe`, `wisp-cli.exe` and the three notice files |
| `Wisp-<v>-linux-x64.tar.gz` | `wisp` and `wisp-cli` (both mode 0755, owner 0/0), `icons/`, `share/` (launcher and icons) and the three notice files |
| `SHA256SUMS` | one `sha256sum -c` line per archive |

The release workflow builds `wisp-app` and `wisp-cli` together and `package.sh` takes the CLI binary as an optional fourth argument. `wisp-cli.exe` is a console-subsystem executable, which is why the CLI is a separate binary. It is Authenticode-signed only when the repository variable `SIGNPATH_SIGNS_CLI` is `true`; otherwise it is unsigned and the draft release notes say so. The winget manifest template adds a `wisp-cli` portable command alias; the MSIX stays GUI-only (`packaging/msix/STORE-SUBMISSION.md`). `check-glibc.sh` also runs on the Linux `wisp-cli`.

The Windows release `wisp.exe` is a GUI-subsystem executable (`windows_subsystem = "windows"` in release builds, so no console window from Explorer, and stderr goes nowhere there) with a static CRT (`+crt-static` in `.cargo/config.toml`, so no VC++ runtime is needed); measured size was 13,630,464 B at task 77 (see Budgets for the current figure).

The bundle id is `io.github.aindriub.wisp`. `Wisp.app` is signed with a Developer ID Application certificate and notarised (see below). It carries the app icon (see below). The Linux binary is built on a pinned `ubuntu-24.04` runner (release, measure and the ci `check` job), so its measured glibc floor is 2.39: Ubuntu 24.04 or newer. `ubuntu-latest` is not used because it moved to 26.04, whose binary needed GLIBC_2.43. `scripts/release/check-glibc.sh` runs after Build in release and measure and fails if any symbol needs a glibc version above 2.39.

To cut a release: make sure the workspace version is the one you want, tag `v<version>` (it must equal the workspace version or the final job fails before creating anything), push the tag, check the draft release and its four assets, then publish with `gh release edit v<version> --draft=false`. A tag push never publishes by itself. If a tag-push run is re-run after failing, it creates a second draft for the same tag; delete the stale one first.

To test without a tag: `gh workflow run release.yml --ref <branch> -f draft_release=true` builds the same assets and creates a draft `dryrun-<run_id>`. Check it, then remove it with `gh release delete dryrun-<run_id> --yes` (`--cleanup-tag` prints a harmless HTTP 422 because a draft has no tag). With `draft_release` false a dispatch run creates no release.

Windows distribution channels have their own guides: `packaging/winget/README.md` (winget manifests), `scripts/release/sign-windows/README.md` (SignPath Authenticode signing) and `packaging/msix/STORE-SUBMISSION.md` (the MSIX and Microsoft Store submission).

### macOS signing and notarisation

The macOS build job signs `Wisp.app` with the Developer ID Application certificate (team `UB89GDN343`) and the hardened runtime, submits the zip to Apple with `notarytool`, and staples the ticket. `scripts/release/macos-keychain.sh` creates and removes a temporary keychain on the runner, and `scripts/release/sign-macos.sh` signs, notarises and staples. Five repository secrets drive it: `APPLE_DEVELOPER_ID_P12`, `APPLE_DEVELOPER_ID_P12_PASSWORD`, `APPLE_API_KEY_P8`, `APPLE_API_KEY_ID` and `APPLE_API_ISSUER_ID` (an App Store Connect API key authenticates `notarytool`). These are names only; the values live in the repository's secret store. `APPLE_TEAM_ID` is a plain `env` value in the workflow, not a secret. `notarytool` waits at most 90 minutes; the first submission for the account took about 19 minutes.

Fallback rules: a `workflow_dispatch` run without the secrets signs ad-hoc and prints a notice, so forks and dry runs still build; a `v*` tag push without the secrets fails; a run with only some of the secrets fails. Linux is unaffected. The Windows executable is Authenticode-signed through SignPath when all five SignPath settings are present, left unsigned with a notice when none are, and the build fails when only some are (see `scripts/release/sign-windows/README.md`).

### App icon

The sources are in `assets/icon/`: `wisp.svg` (large sizes, with sparkles), `wisp-small.svg` (16 to 48 px, simplified), `wisp-layer.svg` and the layered Icon Composer document `AppIcon.icon/`, and `wisp-mono.svg`, which is kept as a reference and used by nothing. `scripts/icon/generate.sh` renders every output with resvg 0.49.0; `generate.sh --check` regenerates into a scratch directory and fails if the committed outputs differ, and it does not flag stale extra files. The committed outputs are the PNGs in `assets/icon/png/` (16 to 1024), `wisp.ico` and `wisp.icns`.

macOS: `package.sh` compiles `AppIcon.icon` with `actool` into `Assets.car`, so the system derives the dark, tinted and clear appearances, and puts `wisp.icns` in the bundle as the fallback; `Info.plist` carries `CFBundleIconFile` and `CFBundleIconName`. The macOS runner must have a full Xcode with Icon Composer support, and the Xcode version is not pinned. Xcode 27.2 `actool` ignores explicit tinted images, which is why the icon is layered rather than supplied as three variants. Windows: `crates/app/build.rs` embeds `wisp.ico` through the `winresource` build-dependency (MIT), and GPUI uses it for the window class. Linux: the 48 to 512 px PNGs go in the tar. The Linux tar also carries `share/applications/io.github.aindriub.wisp.desktop` and hicolor icons (copy `share/` into `~/.local/share`). The macOS binary size is unchanged.

### App lifecycle

Wisp has one window, and closing it quits the app: every window close is routed through `cx.quit()`, so the process never runs windowless. Quit is the `Quit` action (cmd-q on macOS, ctrl-q elsewhere) and, on macOS, the Quit item in the "Wisp" app menu. This also avoids a Touch Bar Mac abort in Dock Quit, caused upstream by `accesskit_macos` restoring the content view's class on window drop; `scripts/measure/quit-check.sh` checks a clean exit.

Quit and close ask before they discard unsaved drafts, and offer to save first (task 143, `crates/app/src/save_close.rs`). The `Quit` action and the window close button go through one function that prompts when any tab is dirty, with the buttons `Save all and quit`, `Discard` and `Cancel`; closing a dirty tab (the × button, cmd-w, the palette and the banner) offers `Save and close`, `Discard` and `Cancel`; closing a collection with unsaved tabs offers `Save all and close`, `Discard` and `Cancel`. A tab or quit that has nothing saveable (scratch drafts) offers only `Discard` and `Cancel`. A save that fails keeps the tab open and says why. A `prompt_up` flag stops a second prompt opening while one is showing. The `Quit` handler defers its work with `cx.defer`, because global action listeners run inside the active window's update, where a nested `handle.update` on that window fails with "window not found". On macOS the Wisp menu also has Hide Wisp (cmd-h), Hide Others (alt-cmd-h) and Show All.

Some paths cannot ask, because GPUI 0.3.10 gives no way to veto them, so a dirty draft is dropped without a prompt:

| Platform | Path | Why it cannot be intercepted |
|---|---|---|
| macOS | Dock Quit, AppleScript `quit`, logout, restart, shutdown | The app delegate implements only `applicationWillTerminate:` (gpui-pre-macos-0.3.10/src/platform.rs:91, :1380), with no `applicationShouldTerminate:`. The internal `on_quit` (gpui-pre-0.3.10/src/app.rs:1152) is not public. |
| Windows | Logoff, restart, shutdown (`WM_QUERYENDSESSION`) | Answered with 1 unconditionally (gpui-pre-windows-0.3.10/src/platform.rs:1203). |
| Linux | SIGTERM, session end | GPUI installs no signal handler, so the default action ends the process. |

This is a documented gap. Fixing it would need an objc swizzle of `terminate:`, a new dependency or a GPUI patch, and the task that added the prompts ruled all three out.

## Decisions worth knowing

All dated 2026-10-29; rationale in the spec's Decisions table.

- GPUI with `gpui-component` for the UI. Rejected: egui, Iced, Slint, ratatui,
  Tauri (system webview breaks portability and memory).
- TOML folder storage. Rejected: YAML (unmaintained `serde_yaml`, awkward
  multiline bodies) and a single SQLite file (opaque in git).
- Cargo workspace with a UI-free service-shaped core. Rejected: a single crate,
  and a daemon with clients, though the serialisable API keeps the latter open.
- `{name}` path params, not `:name`. The Postman importer converts `:name`.
- No scripting in v1.
