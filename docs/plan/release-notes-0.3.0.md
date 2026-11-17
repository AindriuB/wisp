# Wisp 0.3.0

Wisp is a portable, lightweight HTTP client with collections stored as TOML files. 0.3.0 makes a request explainable and safer to send, and adds a read-only command-line tool, without adding scripting.

## What's new

- **Explain Request.** An Explain sub-tab in the response pane shows where each variable, parameter, header, auth setting and request setting came from: the request, a folder, the collection defaults, the environment, a secret, or a value generated at send. A strip under the URL row shows the active environment, auth source and history scope. Secrets are shown by name and source, never by value.
- **Save and close.** Closing a tab or a collection, or quitting, offers "Save and close", "Save all and close" or "Save all and quit" beside Discard and Cancel. A save only writes if the file on disk is still what Wisp read, so a change made outside Wisp is never overwritten. A failed save keeps the tab open.
- **Confirm before send.** Turn on "Toggle confirm before send (this environment)" from the command palette and Wisp asks "Send to {env}?" before sending, from the Send button, the keyboard or the palette. A badge shows while it is on. The setting is per user, stored in `settings.toml` and keyed by collection and environment name, so it is not shared through git and is lost if the environment is renamed. An environment file can suggest it with `suggest_confirm = true`.
- **Assertions.** Add pass or fail checks to a request in the Assertions sub-tab: on the status, a header or a value at a simple JSON path, with equals, contains, matches and exists. Results show in the Checks sub-tab after each send. A failed check never blocks a send, and expected values are literals, not variables.
- **History provenance.** Each history entry records the environment it was sent with and the result of each assertion, as names and states only, never values. The history list shows an "N/M checks" chip. Resend uses your current environment and does not resend assertions.
- **`wisp-cli`.** A separate, read-only command-line tool in every archive: `validate`, `inspect` and `explain`, as text or versioned `--json`. It never sends a request, writes a file or prints a secret. On macOS it is inside the app at `Wisp.app/Contents/MacOS/wisp-cli`; on Windows it is `wisp-cli.exe` beside `wisp.exe`; on Linux it is `wisp-cli` beside `wisp`. See `docs/cli.md`.
- **Also.** Wisp Light and Dark themes; the SigV4 access key is masked in the auth editor; "New collection…" and "New environment…" are in the command palette; history hardening (eviction, a bounded lock, redaction of `Location` and `Refresh`).

## Changed

The sidebar "New collection…" button and the "New environment…" entry in the environment select were built and then cut to keep the Windows executable within its size gate; use the command palette instead. The Windows gate is now 16,000,000 B (it was 15,500,000 B).

## Known limitations

- Tested by hand on macOS (Apple silicon) only. Windows and Ubuntu were not hand-tested for 0.3.0; CI on both is green.
- The confirm-before-send guard is an accident guard, not a security control. It does not cover OAuth2 Authorize, it shows the host as typed before variables resolve, and it fails open: while Wisp is still reading a collection's id, or failed to read it, a send goes through without asking.
- The Windows `wisp.exe` and `wisp-cli.exe` are not signed, so SmartScreen warns on first launch ("More info", then "Run anyway"). The macOS app and its `wisp-cli` are signed and notarised. The Microsoft Store package does not include `wisp-cli`.
- A save compares the file just before writing; it is not a lock, so a write from outside Wisp in that instant can still win.
- Not included: scripting, GraphQL, global variables, a CLI `run` command, sending from the CLI, and Postman script translation. OAuth2 access tokens are never saved to disk.

## Install and checksum

Assets are `Wisp-0.3.0-macos-arm64.zip`, `Wisp-0.3.0-windows-x64.zip` and `Wisp-0.3.0-linux-x64.tar.gz`, with `SHA256SUMS`. Check each archive against `SHA256SUMS` before unpacking. Per-platform steps, Linux library requirements and where Wisp writes are in the [README](https://github.com/AindriuB/wisp/blob/main/README.md).
