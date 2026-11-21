# Wisp 0.5.0

Wisp is a portable, lightweight HTTP client with collections stored as TOML files. 0.5.0 lets you compare two runs of a request, send GraphQL bodies, import simple Postman checks as assertions, and trust a private certificate authority. It adds no scripting.

0.4.0 was never published, so anyone upgrading from 0.3.0 gets the 0.4.0 changes as well. They are listed in their own section below.

## What's new in 0.5.0

- **Compare two runs.** In the History panel, "Compare with previous" compares an entry with the earlier one for the same request, and "Compare from" then "Compare to" compares any two. The view shows status, headers and a structural JSON body diff. Non-JSON bodies are compared for byte equality only.
- **Ignore list.** `[compare] ignore` in `collection.toml` leaves volatile fields out, with entries such as `header:date` or `body:/data/*/id`. Ignore on a row in the compare view adds the exact pattern for you.
- **Evidence notes.** New history entries record whether the body and head were stored in full, cut short, switched off, unreadable or never received. When a side is missing or incomplete, the comparison says so instead of calling the sides equal. Entries written before 0.5.0 are compared on what they recorded and say so.
- **GraphQL bodies.** A request body can be a GraphQL query plus variables, saved as `[body] type = "graphql"`, with a Query editor and a Variables editor (with Format) in the request pane. It is sent as a JSON POST body. There is no introspection, schema explorer or `{{var}}` highlighting. A 0.4.0 build reports such a file as invalid.
- **Postman checks.** A request-level Postman test script made wholly of `pm.test` units in a closed, simple form (status, header, JSON path equality and the like) imports as `[[assert]]` entries. Any other script is left untranslated and listed in the warnings report; scripts are not run. GraphQL bodies from Postman import too.
- **Private certificate authorities.** Wisp trusts its bundled roots plus the operating system's store by default. Three command palette commands, "Toggle system certificate trust", "Choose CA certificate file…" and "Clear CA certificate file", write `[tls]` in `settings.toml`. `wisp-cli run` takes `--ca-bundle <file>` and `--no-system-trust`. A CA file must hold certificates only, and a file that does not is refused whole. Trust is per user and is never read from a collection. When a send fails on an unknown issuer, the response panel points at the commands. The README explains how to export a private root such as Caddy's on macOS.
- **Assertion numbers.** A JSON assertion treats `1` and `1.0` as equal, compared by value. A string never equals a number.
- **Hardening.** Collection binding is stricter: a moved collection is refused while another root holds the same id, markers are size-capped and fail closed, and linked worktrees are checked for ownership. The keychain worker zeroes secrets it no longer needs. The OpenAPI importer charges example text to its budget as it serialises, picks unique names in linear time, and strips every chained credential from a URL. Windows owner-only checks now require the owner's ACE to grant full access, and a failed restrict removes the directories it created. Secret-file reads refuse non-regular files, so a swapped-in FIFO cannot hang a read, and the git-tracked check is bounded at 5 s and fails closed.
- **Smaller fixes.** A snapshot names an unreadable environment in `truncated`. The README says how to put `wisp-cli` on your `PATH` on macOS. The three known test flakes are fixed.

## What was new in 0.4.0 (never published)

- **One send path.** A new `wisp-service` crate holds planning, sending, the delivery outcome and the history record. The app and the command line use the same code, so a request behaves the same from either. The engine reports whether anything was sent (`not_sent`, `responded` or `unknown`), follows every redirect itself, and no longer retries behind your back. History entries record that delivery state, and the history chip shows "outcome unknown" when a request may have reached the server.
- **`wisp-cli run`.** `wisp-cli run <collection> <request-path>` sends one saved request, with `--env`, `--json`, `--history` (append a history entry as the app would) and `--keychain` (load keychain-backed secrets; `run` never prompts for them itself). Exit codes tell the outcome: 0 passed, 1 an assertion failed, 2 bad usage, 3 the collection, request or environment cannot be read, 4 not prepared, 5 transport failed, 6 outcome unknown, 7 unexpected status, 8 output failed. See `docs/cli.md`.
- **`wisp-cli snapshot`.** A bounded, secret-free picture of a workspace as text or `--json`, capped at 1 MiB, for handing to a person or a tool.
- **Explain metadata.** Requests can carry a `description` and an `expected_status`, stored in the request file and shown in the Explain pane. A Vars sub-tab edits per-request variables, flags duplicate names and refuses to save them.
- **Importers.** The OpenAPI importer warns when it dropped credentials from a server URL, and leaves the host out of the warning when it might itself be a secret. Environment names, warnings and URLs from Postman and OpenAPI no longer carry credentials, and descriptions are imported.
- **Windows hardening.** The data directory is owner-only, file identity is checked by handle, and a real 8.3 short-name test fails the build. A volume without persistent ACLs, such as a portable FAT or exFAT drive, now works with one warning.
- **Closed hardening items.** Secret import and move conflicts are typed and confirmed, file bodies are hashed and sent from one handle under one timeout budget, history masking is linear and caps request bodies, and a cut history request body is refused until you edit it.

## Sizes

The Windows `wisp.exe` is 15,823,872 B, 176,128 B under the 16,000,000 B gate and 207,872 B larger than 0.4.0. `wisp-cli` is 3,667,424 B on macOS, 4,346,880 B on Windows and 6,483,064 B on Linux, each under a 7,340,032 B (7 MiB) limit. Figures are in `scripts/measure/results/v0.5.0-wave-3-size.json`.

## Known limitations

- Tested by hand on macOS (Apple silicon) only, on 2026-11-21. Windows and Linux were not hand-tested for 0.5.0; CI on both is green.
- Compare does not diff non-JSON bodies line by line, compares arrays by position, and cannot be exported or copied. There is no `wisp-cli compare`.
- GraphQL has no introspection, schema explorer or variable highlighting. Postman scripts that are not in the closed form are not translated, and there is no scripting.
- Private CA trust is per user: it cannot be set per collection or environment, and there are no client certificates.
- `wisp-cli run` is for saved requests. OAuth2 requests that need interactive authorization exit 4 and send nothing. OAuth2 access tokens are never saved to disk.
- The confirm-before-send guard is an accident guard, not a security control, and fails open while a collection's id is unread.
- The Windows `wisp.exe` and `wisp-cli.exe` are not signed, so SmartScreen warns on first launch ("More info", then "Run anyway"). The macOS app and its `wisp-cli` are signed and notarised. The Microsoft Store package does not include `wisp-cli`.
- Not included: scripting, global variables, Intel Mac builds.

## Install and checksum

Assets are `Wisp-0.5.0-macos-arm64.zip`, `Wisp-0.5.0-windows-x64.zip` and `Wisp-0.5.0-linux-x64.tar.gz`, with `SHA256SUMS`. Check each archive against `SHA256SUMS` before unpacking. Per-platform steps, Linux library requirements and where Wisp writes are in the [README](https://github.com/AindriuB/wisp/blob/main/README.md).
