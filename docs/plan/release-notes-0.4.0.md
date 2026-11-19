# Wisp 0.4.0

Wisp is a portable, lightweight HTTP client with collections stored as TOML files. 0.4.0 lets the command line send a saved request, moves request sending into one shared crate, and closes a long list of hardening gaps. It adds no scripting.

## What's new

- **One send path.** A new `wisp-service` crate holds planning, sending, the delivery outcome and the history record. The app and the command line now use the same code, so a request behaves the same from either. The engine reports whether anything was sent (`not_sent`, `responded` or `unknown`), follows every redirect itself, and no longer retries behind your back. History entries record that delivery state, and the history chip shows "outcome unknown" when a request may have reached the server.
- **`wisp-cli run`.** `wisp-cli run <collection> <request-path>` sends one saved request, with `--env`, `--json`, `--history` (append a history entry as the app would) and `--keychain` (load keychain-backed secrets; `run` never prompts for them itself). Exit codes tell the outcome: 0 passed, 1 an assertion failed, 2 bad usage, 3 the collection, request or environment cannot be read, 4 not prepared, 5 transport failed, 6 outcome unknown, 7 unexpected status, 8 output failed. See `docs/cli.md`.
- **`wisp-cli snapshot`.** A bounded, secret-free picture of a workspace as text or `--json`, capped at 1 MiB, for handing to a person or a tool.
- **Explain metadata.** Requests can carry a `description` and an `expected_status`, stored in the request file and shown in the Explain pane. A Vars sub-tab edits per-request variables, flags duplicate names and refuses to save them.
- **Importers.** The OpenAPI importer warns when it dropped credentials from a server URL, and leaves the host out of the warning when it might itself be a secret. Environment names, warnings and URLs from Postman and OpenAPI no longer carry credentials, and descriptions are imported.
- **Windows hardening.** The data directory is owner-only, file identity is checked by handle, and a real 8.3 short-name test fails the build. A volume without persistent ACLs, such as a portable FAT or exFAT drive, now works with one warning.
- **Closed hardening items.** 31 acceptance criteria from earlier reviews are done: secret import and move conflicts are typed and confirmed, file bodies are hashed and sent from one handle under one timeout budget, history masking is linear and caps request bodies, and a cut history request body is refused until you edit it.

## Sizes

The Windows `wisp.exe` is 15,616,000 B, 384,000 B under the 16,000,000 B gate and 82,432 B larger than 0.3.0. The macOS app binary is 11,901,136 B and the Linux binary 26,303,048 B. `wisp-cli` is 3,535,072 B on macOS, 4,263,424 B on Windows and 6,391,544 B on Linux, each under a 7,340,032 B (7 MiB) limit. Run 44593845270 measured them.

## Known limitations

- Tested by hand on macOS (Apple silicon) only. Windows and Ubuntu were not hand-tested for 0.4.0; CI on both is green.
- `wisp-cli run` is for saved requests. OAuth2 requests that need interactive authorization exit 4 and send nothing. OAuth2 access tokens are never saved to disk.
- The confirm-before-send guard is an accident guard, not a security control, and fails open while a collection's id is unread.
- The Windows `wisp.exe` and `wisp-cli.exe` are not signed, so SmartScreen warns on first launch ("More info", then "Run anyway"). The macOS app and its `wisp-cli` are signed and notarised. The Microsoft Store package does not include `wisp-cli`.
- Not included: scripting, GraphQL, global variables and Postman script translation.

## Install and checksum

Assets are `Wisp-0.4.0-macos-arm64.zip`, `Wisp-0.4.0-windows-x64.zip` and `Wisp-0.4.0-linux-x64.tar.gz`, with `SHA256SUMS`. Check each archive against `SHA256SUMS` before unpacking. Per-platform steps, Linux library requirements and where Wisp writes are in the [README](https://github.com/AindriuB/wisp/blob/main/README.md).
