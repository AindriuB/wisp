# Wisp

Wisp is a portable, lightweight HTTP client for developers working locally, in the spirit of Postman, Bruno and Insomnia. It ships as one self-contained binary per platform, with no installer, and it keeps its collections as TOML files in an ordinary folder, so they diff and review well in git. You open a collection folder, pick an environment, send a request and read the response. Its reason to exist is staying small and light on memory. The interface is built with GPUI, the framework behind the Zed editor.

This is version 0.2.0. Besides sending requests and keeping collections as TOML files, it can now:

- **Import** existing work: a curl command, a Postman v2.1 collection, or an OpenAPI 3 document in JSON or YAML. OpenAPI servers become environments. Anything it cannot convert, such as Postman scripts, is listed in a warnings report instead of being dropped silently.
- **Authenticate** with Basic, Bearer and API key as before, and now with Digest, AWS Signature V4 and OAuth2 (client credentials, and authorization code with PKCE through your system browser).
- **Show where time went**: the response panel reports DNS and connect+TLS time alongside the first-byte and total times. A reused connection has no connect time to report.
- **Keep history**: each send is recorded in a per-user history panel with its response. Reopen an entry read-only, or Resend it. Credentials are not stored, and a resend uses your current environment. Clear history empties it. History keeps 500 entries per collection and cuts stored bodies at 1 MiB.
- **Keep cookies** in a per-collection jar, with a Cookies manager to list and delete them. Saving cookies to disk is off until you turn it on for that collection.
- **Open several collections** at once (up to 8), each with its own tree, environment and tabs, and switch between them in the sidebar.
- **Keep secrets in the OS keychain**, if you choose, per environment. Secrets files remain the default. Where no keychain is available (portable mode, or Linux without a Secret Service), Wisp falls back to the secrets file and says so.

It also includes the fixes meant for a 0.1.1 that was never released: Wisp asks before discarding unsaved changes on quit, secrets are kept out of debug output, streamed bodies are resent on 307/308 redirects, Windows-unsafe file names are rejected, the Linux window and launcher have an icon, and the interface fits narrow windows. Redirect handling, history redaction, importers and the reading of collection files were also hardened against leaking credentials or following symlinks out of a collection.

Still missing: scripting, GraphQL, global variables, a command-line runner, and a TCP/TLS split in the timings. Postman scripts are not translated. OAuth2 access tokens are never saved to disk; refresh tokens are saved only for keychain-backed environments. The Windows executable is not signed, and there is no Intel Mac build.

## Install

Download the archive for your platform and `SHA256SUMS` from the GitHub Release. Check the archive against the checksum file before you unpack it. Wisp needs no installation step: unpack it anywhere you like.

### macOS (Apple silicon)

The asset is `Wisp-0.2.0-macos-arm64.zip`. Apple silicon is required; Intel Macs are not supported.

```sh
shasum -a 256 -c --ignore-missing SHA256SUMS
unzip Wisp-0.2.0-macos-arm64.zip
```

The zip holds a `Wisp-0.2.0-macos-arm64` folder containing `Wisp.app`. Move `Wisp.app` to `/Applications` or anywhere else. The app is signed with a Developer ID certificate and notarised by Apple, so it opens without a Gatekeeper warning. A build you make from source is not signed, and macOS may block it.

The command palette opens with ⌘K.

### Windows (x64)

The asset is `Wisp-0.2.0-windows-x64.zip`.

```powershell
(Get-FileHash Wisp-0.2.0-windows-x64.zip -Algorithm SHA256).Hash
Select-String Wisp-0.2.0-windows-x64.zip SHA256SUMS
```

The two hashes must match, ignoring case. Unpack the zip (Extract All in Explorer, or `Expand-Archive`). It holds a `Wisp-0.2.0-windows-x64` folder with `wisp.exe`; put the folder wherever you like and run `wisp.exe`. The executable is not signed, so SmartScreen shows "Windows protected your PC" on first launch. Click "More info", then "Run anyway". The command palette opens with Ctrl+K.

### Linux (x64)

The asset is `Wisp-0.2.0-linux-x64.tar.gz`.

```sh
sha256sum -c --ignore-missing SHA256SUMS
tar -xzf Wisp-0.2.0-linux-x64.tar.gz
```

The archive holds a `Wisp-0.2.0-linux-x64` folder. The folder holds the `wisp` binary, an `icons/` folder and a `share/` folder. To install it for your user, from inside that folder run:

```sh
cp wisp ~/.local/bin/
cp -r share/* ~/.local/share/
```

`wisp` must be on your `PATH`, because the launcher entry runs `Exec=wisp`. The window icon shows on X11 only; on Wayland the icon comes from the installed launcher entry. The command palette opens with Ctrl+K.

The Linux binary is built on a pinned Ubuntu 24.04 runner and needs glibc 2.39 or newer (Ubuntu 24.04 or newer, or an equally recent distribution). Wisp opens an X11 or Wayland window and draws it with Vulkan, so it needs a working Vulkan driver. On Debian or Ubuntu, install:

```sh
sudo apt install libvulkan1 mesa-vulkan-drivers libxcb1 libx11-6 libx11-xcb1 \
    libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libgcc-s1 libc6
```

The Vulkan driver depends on your GPU: `mesa-vulkan-drivers` covers AMD, Intel and the software renderer, and for NVIDIA you install the vendor's driver instead. Of the other packages, the `wisp` binary links `libxcb`, `libxkbcommon`, `libxkbcommon-x11`, `libgcc_s`, `libm` and `libc` directly; `libx11-6`, `libx11-xcb1` and the Wayland client library are loaded by the graphics stack. Other distributions have equivalent packages under their own names.

## Platform testing

The maintainer hand-tested 0.1.0 on macOS (Apple silicon) and Windows x64, and hand-checked the new 0.2.0 features on macOS during development. Keychain behaviour with the signed macOS build is checked in the release hand test. Linux x64 has not been tested by hand. In CI it is built, its tests are run, and the binary is run under Xvfb with a software Vulkan driver, where its memory is measured and a screenshot of the window is taken and inspected. If something breaks on Linux with a real GPU, please report it.

## Unsaved changes

Wisp asks before it discards unsaved edits when you quit with ⌘Q (Ctrl+Q on Windows and Linux), choose Quit Wisp, close the window, or close a tab. It cannot ask when the system ends the app: Dock Quit, logout and shutdown on macOS, logoff and shutdown on Windows, and SIGTERM or session end on Linux all drop unsaved drafts without a prompt. Save with ⌘S before you do any of those.

## Where Wisp writes

Wisp writes to the collection folder, to `settings.toml`, to a per-user data directory (history, and cookies if you enable saving them), and to wherever you choose when you save a response. The data directory is in the operating system's data location, or in a `data` folder beside the executable in portable mode; its files are readable only by you. With the keychain option, secrets are written to the operating system's keychain. Wisp also writes in the operating system's temp directory in two cases: a response body larger than the in-memory limit is spilled to a temporary file, and on Unix startup briefly creates and deletes a small `.wisp-probe-*` file. On Windows and Linux, settings and data live beside the executable when a file named `portable` is next to it, and in the operating system's config and data directories otherwise. macOS has no portable mode; settings and data are always under `~/Library/Application Support/wisp`.

## Building from source

The toolchain is pinned in `rust-toolchain.toml`. Build with `cargo build --release -p wisp-app`.

## License

Wisp is licensed under the Apache License, Version 2.0; see [LICENSE](LICENSE) and [NOTICE](NOTICE). The licenses of the libraries it is built from are listed in [THIRD-PARTY-NOTICES](THIRD-PARTY-NOTICES). Each release archive carries all three files.
