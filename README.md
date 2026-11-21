# Wisp

Wisp is a portable, lightweight HTTP client for developers working locally, in the spirit of Postman, Bruno and Insomnia. It ships as one self-contained binary per platform, with no installer, and it keeps its collections as TOML files in an ordinary folder, so they diff and review well in git. You open a collection folder, pick an environment, send a request and read the response. Its reason to exist is staying small and light on memory. The interface is built with GPUI, the framework behind the Zed editor.

This is version 0.5.0. It adds a compare view for two runs of a request with a per-collection ignore list, GraphQL request bodies, translation of simple Postman checks to `[[assert]]`, and trust for private certificate authorities in both the app and `wisp-cli run`. It also carries the 0.4.0 changes, since 0.4.0 was never published: `wisp-cli run` and `wisp-cli snapshot`, one shared `wisp-service` crate for sending, and a long list of hardening fixes. Release notes are in `docs/plan/release-notes-0.5.0.md`.

Besides sending requests and keeping collections as TOML files, it also can:

- **Import** existing work: a curl command, a Postman v2.1 collection, or an OpenAPI 3 document in JSON or YAML. OpenAPI servers become environments. Anything it cannot convert, such as Postman scripts, is listed in a warnings report instead of being dropped silently.
- **Authenticate** with Basic, Bearer and API key as before, and now with Digest, AWS Signature V4 and OAuth2 (client credentials, and authorization code with PKCE through your system browser).
- **Show where time went**: the response panel reports DNS and connect+TLS time alongside the first-byte and total times. A reused connection has no connect time to report.
- **Keep history**: each send is recorded in a per-user history panel with its response. Reopen an entry read-only, or Resend it. Credentials are not stored, and a resend uses your current environment. Clear history empties it. History keeps 500 entries per collection and cuts stored bodies at 1 MiB.
- **Keep cookies** in a per-collection jar, with a Cookies manager to list and delete them. Saving cookies to disk is off until you turn it on for that collection.
- **Open several collections** at once (up to 8), each with its own tree, environment and tabs, and switch between them in the sidebar.
- **Compare two runs** of a request from the History panel ("Compare with previous", or "Compare from" and "Compare to"): status, headers and a structural JSON body diff. A per-collection ignore list (`[compare] ignore` in `collection.toml`, entries such as `header:date` or `body:/data/*/id`) leaves volatile fields out. When a stored body is missing, cut short or was never recorded, the comparison says so as an evidence note instead of calling the sides equal.
- **Send GraphQL**: a request body can be a GraphQL query plus variables, saved as `[body] type = "graphql"`. There is no introspection or schema explorer. A 0.4.0 build reports such a file as invalid.
- **Import Postman checks**: a Postman request-level test script made wholly of `pm.test` units in a closed, simple form is translated to `[[assert]]` entries. Any other script is left untranslated and listed in the warnings report; there is still no scripting.
- **Trust a private certificate authority** for HTTPS, in the app and in `wisp-cli run`; see "TLS and private certificate authorities" below.
- **Keep secrets in the OS keychain**, if you choose, per environment. Secrets files remain the default. Where no keychain is available (portable mode, or Linux without a Secret Service), Wisp falls back to the secrets file and says so.

It also includes the fixes meant for a 0.1.1 that was never released: Wisp asks before discarding unsaved changes on quit, secrets are kept out of debug output, streamed bodies are resent on 307/308 redirects, Windows-unsafe file names are rejected, the Linux window and launcher have an icon, and the interface fits narrow windows. Redirect handling, history redaction, importers and the reading of collection files were also hardened against leaking credentials or following symlinks out of a collection.

Still missing: scripting, global variables, and a TCP/TLS split in the timings. Postman scripts are not translated. OAuth2 access tokens are never saved to disk; refresh tokens are saved only for keychain-backed environments. The Windows executable is not signed, and there is no Intel Mac build.

## Install

Download the archive for your platform and `SHA256SUMS` from the GitHub Release. Check the archive against the checksum file before you unpack it. Wisp needs no installation step: unpack it anywhere you like.

### macOS (Apple silicon)

The asset is `Wisp-0.5.0-macos-arm64.zip`. Apple silicon is required; Intel Macs are not supported.

```sh
shasum -a 256 -c --ignore-missing SHA256SUMS
unzip Wisp-0.5.0-macos-arm64.zip
```

The zip holds a `Wisp-0.5.0-macos-arm64` folder containing `Wisp.app`. Move `Wisp.app` to `/Applications` or anywhere else. The app is signed with a Developer ID certificate and notarised by Apple, so it opens without a Gatekeeper warning. A build you make from source is not signed, and macOS may block it.

The archive also holds the command-line tool as `Wisp.app/Contents/MacOS/wisp-cli`, signed and notarised with the app (see "Command line" below). The command palette opens with ⌘K.

### Windows (x64)

The asset is `Wisp-0.5.0-windows-x64.zip`.

```powershell
(Get-FileHash Wisp-0.5.0-windows-x64.zip -Algorithm SHA256).Hash
Select-String Wisp-0.5.0-windows-x64.zip SHA256SUMS
```

The two hashes must match, ignoring case. Unpack the zip (Extract All in Explorer, or `Expand-Archive`). It holds a `Wisp-0.5.0-windows-x64` folder with `wisp.exe`; put the folder wherever you like and run `wisp.exe`. `wisp-cli.exe`, the command-line tool, is in the same folder. The executable is not signed, so SmartScreen shows "Windows protected your PC" on first launch. Click "More info", then "Run anyway". The command-line tool `wisp-cli.exe` is not signed either, unless the release notes say it is, so SmartScreen may warn about it too. The command palette opens with Ctrl+K.

### Linux (x64)

The asset is `Wisp-0.5.0-linux-x64.tar.gz`.

```sh
sha256sum -c --ignore-missing SHA256SUMS
tar -xzf Wisp-0.5.0-linux-x64.tar.gz
```

The archive holds a `Wisp-0.5.0-linux-x64` folder. The folder holds the `wisp` binary, an `icons/` folder and a `share/` folder; it also holds the `wisp-cli` command-line tool. To install it for your user, from inside that folder run:

```sh
cp wisp wisp-cli ~/.local/bin/
cp -r share/* ~/.local/share/
```

`wisp` must be on your `PATH`, because the launcher entry runs `Exec=wisp`. The window icon shows on X11 only; on Wayland the icon comes from the installed launcher entry. The command palette opens with Ctrl+K.

The Linux binary is built on a pinned Ubuntu 24.04 runner and needs glibc 2.39 or newer (Ubuntu 24.04 or newer, or an equally recent distribution). Wisp opens an X11 or Wayland window and draws it with Vulkan, so it needs a working Vulkan driver. On Debian or Ubuntu, install:

```sh
sudo apt install libvulkan1 mesa-vulkan-drivers libxcb1 libx11-6 libx11-xcb1 \
    libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libgcc-s1 libc6
```

The Vulkan driver depends on your GPU: `mesa-vulkan-drivers` covers AMD, Intel and the software renderer, and for NVIDIA you install the vendor's driver instead. Of the other packages, the `wisp` binary links `libxcb`, `libxkbcommon`, `libxkbcommon-x11`, `libgcc_s`, `libm` and `libc` directly; `libx11-6`, `libx11-xcb1` and the Wayland client library are loaded by the graphics stack. Other distributions have equivalent packages under their own names.

## Command line

`wisp-cli` ships from Wisp 0.3.0 in every archive.

`wisp-cli` is a tool for a terminal, a script or an agent. `wisp-cli validate <collection>` reports every problem in a collection, `wisp-cli inspect <collection>` summarises it, `wisp-cli explain <collection> <request-path> --env <name>` shows where each variable, header and auth setting of a request comes from, and `wisp-cli snapshot <collection>` prints one bounded picture of a workspace. These four never send a request, write a file or print a secret value. From 0.4.0, `wisp-cli run` sends one saved request and reports the result with a distinct exit code for each outcome, so a CI job can tell an assertion failure from an unexpected status or a transport failure. It never retries, and it says whether a retry is safe:

```sh
wisp-cli run ./my-collection orders/create.toml --env staging --json
```

`--json` prints one versioned JSON document and is the interface meant for agents. Commands, exit codes and the JSON fields are in [docs/cli.md](docs/cli.md).

Where it is: on macOS inside the app, at `Wisp.app/Contents/MacOS/wisp-cli`; on Windows as `wisp-cli.exe` beside `wisp.exe`; on Linux as `wisp-cli` beside `wisp`. To use it from any macOS terminal, link it onto your `PATH`:

```sh
sudo mkdir -p /usr/local/bin
sudo ln -s /Applications/Wisp.app/Contents/MacOS/wisp-cli /usr/local/bin/wisp-cli
```

`/usr/local/bin` may not exist on an Apple-silicon Mac, so the `mkdir -p` comes first; it does nothing if the folder is there. To avoid `sudo`, link it into a folder in your home directory and put that folder on your `PATH`:

```sh
mkdir -p ~/.local/bin
ln -s /Applications/Wisp.app/Contents/MacOS/wisp-cli ~/.local/bin/wisp-cli
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zprofile
```

Open a new terminal window, then check that it works:

```sh
command -v wisp-cli
wisp-cli --help
```

`command -v` should print the path of the link, and `--help` the usage text.

## TLS and private certificate authorities

Wisp verifies HTTPS servers against the certificate authorities (CAs) it trusts. By default that is its bundled Mozilla root list plus your operating system's certificate store, so a server whose certificate was issued by a CA your system trusts works with no setup. Turning system trust off restores the bundled list alone; the bundled roots are always trusted.

For a server signed by a private CA, such as the root Caddy generates for local HTTPS, give Wisp the CA certificate instead of turning verification off. The CA file is a PEM file of at most 1 MiB that holds certificates only. A file with a private key, any other kind of PEM block or a damaged block is refused whole, and nothing is trusted from it.

In the app there are three command palette commands: "Toggle system certificate trust", "Choose CA certificate file…" and "Clear CA certificate file". They write `[tls]` in your `settings.toml` (`system_trust`, and `ca_bundle`, an absolute path). When a send fails because the server's issuer is unknown, the response panel points at these commands. Trust is per user: it is never read from a collection, so a collection you open from someone else cannot widen what Wisp trusts.

On macOS, to export a private root such as Caddy's from Keychain Access:

1. Open Keychain Access and select the "login" or "System" keychain and the "Certificates" category.
2. Select the root certificate (Caddy's is named like "Caddy Local Authority - 2026 ECC Root").
3. Choose File, then Export Items, set the format to "Privacy Enhanced Mail (.pem)" and save it, for example as `~/caddy-root.pem`.
4. In Wisp, run "Choose CA certificate file…" and pick that file.

`wisp-cli run` never reads `settings.toml`; it takes flags only:

```sh
wisp-cli run ./shop orders/create.toml --env local --ca-bundle ~/caddy-root.pem
wisp-cli run ./shop orders/create.toml --no-system-trust --ca-bundle ~/caddy-root.pem
```

`--ca-bundle <file>` adds the CA file to what is trusted, and `--no-system-trust` leaves out the operating system's store. A file that cannot be read or is refused stops the run before anything is sent, with exit 3.

As a last resort for one request, `verify_tls = false` in that request's settings turns verification off for that request alone. Prefer a CA file: it keeps verification on for everything else.

The Windows package for winget also installs the `wisp-cli` command; the Microsoft Store (MSIX) package does not include it.

## Confirm before send

You can ask Wisp to confirm before it sends to a chosen environment, to guard against sending to production by accident. Turn it on from the command palette ("Toggle confirm before send (this environment)"). While it is on, a "Confirm before send" badge shows in the URL bar, and clicking the badge turns the guard off. The setting is per user, kept in your `settings.toml`, and keyed by collection and environment name, so it is not shared through git and a renamed environment loses it. An environment file can commit `suggest_confirm = true`, and Wisp then offers once per run to turn the guard on. The guard covers sending a request; it does not cover OAuth2 Authorize. It is an accident guard, not a security control: while Wisp is still reading a collection's id, or failed to read it, the guard fails open and the send goes through without asking.

## Platform testing

The maintainer hand-tested 0.5.0 on macOS (Apple silicon) only, on 2026-11-21. Windows x64 and Linux x64 were not hand-tested for 0.5.0; Windows was last hand-tested at 0.1.0, and Linux has never been tested by hand. In CI it is built, its tests are run, and the binary is run under Xvfb with a software Vulkan driver, where its memory is measured and a screenshot of the window is taken and inspected. If something breaks on Linux with a real GPU, please report it.

## Unsaved changes

Wisp asks before it discards unsaved edits when you quit with ⌘Q (Ctrl+Q on Windows and Linux), choose Quit Wisp, close the window, close a collection, or close a tab. The prompt offers to save first: "Save and close" for a tab, "Save all and close" for a collection and "Save all and quit" when quitting, with "Discard" and "Cancel" beside it. It cannot ask when the system ends the app: Dock Quit, logout and shutdown on macOS, logoff and shutdown on Windows, and SIGTERM or session end on Linux all drop unsaved drafts without a prompt. Save with ⌘S before you do any of those.

## Where Wisp writes

Wisp writes to the collection folder, to `settings.toml`, to a per-user data directory (history, and cookies if you enable saving them), and to wherever you choose when you save a response. The data directory is in the operating system's data location, or in a `data` folder beside the executable in portable mode; on macOS and Linux its files are private to your user, while on Windows they inherit the folder's permissions (the per-user data directory under your profile is private by default; a portable folder is only as private as the place you put it). With the keychain option, secrets are written to the operating system's keychain. Wisp also writes in the operating system's temp directory in two cases: a response body larger than the in-memory limit is spilled to a temporary file, and on Unix startup briefly creates and deletes a small `.wisp-probe-*` file. An absolute `WISP_DATA_DIR` always wins and puts settings and data in that directory; a relative or empty value is ignored. On Windows and Linux, a file named `portable` next to the executable otherwise makes settings and data live beside it, and without one they live in the operating system's config and data directories. On macOS, use an absolute `WISP_DATA_DIR` for portable data; the `portable` marker file is the Windows and Linux mechanism, and without the override macOS uses `~/Library/Application Support/wisp`.

## Building from source

The toolchain is pinned in `rust-toolchain.toml`. Build with `cargo build --release -p wisp-app`.

## License

Wisp is licensed under the Apache License, Version 2.0; see [LICENSE](LICENSE) and [NOTICE](NOTICE). The licenses of the libraries it is built from are listed in [THIRD-PARTY-NOTICES](THIRD-PARTY-NOTICES). Each release archive carries all three files.
