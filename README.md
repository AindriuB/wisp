# Wisp

Wisp is a portable, lightweight HTTP client for developers working locally, in the spirit of Postman, Bruno and Insomnia. It ships as one self-contained binary per platform, with no installer, and it keeps its collections as TOML files in an ordinary folder, so they diff and review well in git. You open a collection folder, pick an environment, send a request and read the response. Its reason to exist is staying small and light on memory. The interface is built with GPUI, the framework behind the Zed editor.

This is version 0.1.0, the first release. Scripting, importers (Postman, OpenAPI, curl), GraphQL, OAuth2, a history view, and code signing are not in it.

## Install

Download the archive for your platform and `SHA256SUMS` from the GitHub Release. Check the archive against the checksum file before you unpack it. Wisp needs no installation step: unpack it anywhere you like.

### macOS (Apple silicon)

The asset is `Wisp-0.1.0-macos-arm64.zip`. Apple silicon is required; Intel Macs are not supported.

```sh
shasum -a 256 -c --ignore-missing SHA256SUMS
unzip Wisp-0.1.0-macos-arm64.zip
```

The zip holds a `Wisp-0.1.0-macos-arm64` folder containing `Wisp.app`. Move `Wisp.app` to `/Applications` or anywhere else. The app is ad-hoc signed, not signed with a Developer ID and not notarised, so macOS blocks the first launch. To open it, try to launch it once, then open System Settings, go to Privacy & Security, and click "Open Anyway" next to the message about Wisp. On macOS 15 and later, Control-click and Open no longer gets past Gatekeeper. Alternatively, remove the quarantine flag before the first launch:

```sh
xattr -d com.apple.quarantine Wisp.app
```

The app has no icon yet, so it shows the generic one. The command palette opens with ⌘K.

### Windows (x64)

The asset is `Wisp-0.1.0-windows-x64.zip`.

```powershell
(Get-FileHash Wisp-0.1.0-windows-x64.zip -Algorithm SHA256).Hash
Select-String Wisp-0.1.0-windows-x64.zip SHA256SUMS
```

The two hashes must match, ignoring case. Unpack the zip (Extract All in Explorer, or `Expand-Archive`). It holds a `Wisp-0.1.0-windows-x64` folder with `wisp.exe`; put the folder wherever you like and run `wisp.exe`. The executable is not signed, so SmartScreen shows "Windows protected your PC" on first launch. Click "More info", then "Run anyway". The command palette opens with Ctrl+K.

### Linux (x64)

The asset is `Wisp-0.1.0-linux-x64.tar.gz`.

```sh
sha256sum -c --ignore-missing SHA256SUMS
tar -xzf Wisp-0.1.0-linux-x64.tar.gz
```

The archive holds a `Wisp-0.1.0-linux-x64` folder with the `wisp` binary. Put it anywhere on your path, for example `~/.local/bin`. The command palette opens with Ctrl+K.

The binary was built on Ubuntu's current CI image and needs glibc 2.43 or newer. That is the highest symbol version in the binary's dynamic symbol table (`objdump -T wisp`), so it runs on Ubuntu 26.04 or a distribution with an equally new glibc, and not on Ubuntu 24.04. Wisp opens an X11 or Wayland window and draws it with Vulkan, so it needs a working Vulkan driver. On Debian or Ubuntu, install:

```sh
sudo apt install libvulkan1 mesa-vulkan-drivers libxcb1 libx11-6 libx11-xcb1 \
    libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libgcc-s1 libc6
```

`mesa-vulkan-drivers` covers Intel, AMD and the software renderer; for NVIDIA, install the vendor's driver instead. A driver is something the list of libraries the running binary loads can only show indirectly, so choose the one that matches your GPU. The other packages are the libraries that list shows the binary loading, which includes the X11/xcb and xkbcommon libraries it links directly, plus the Wayland client library that the graphics stack loads. Other distributions have equivalent packages under their own names.

## Platform testing

The maintainer hand-tested macOS on Apple silicon and Windows x64 before release. Linux x64 has not been tested by hand. In CI it is built, its tests are run, and the binary is run under Xvfb with a software Vulkan driver, where its memory is measured and a screenshot of the window is taken and inspected. If something breaks on Linux with a real GPU, please report it.

## Where Wisp writes

Wisp writes nothing outside the collection folder and `settings.toml`. Settings live beside the executable when a file named `portable` is next to it, and in the operating system's config directory otherwise.

## Building from source

The toolchain is pinned in `rust-toolchain.toml`. Build with `cargo build --release -p wisp-app`.

## License

Wisp is licensed under the Apache License, Version 2.0; see [LICENSE](LICENSE) and [NOTICE](NOTICE). The licenses of the libraries it is built from are listed in [THIRD-PARTY-NOTICES](THIRD-PARTY-NOTICES). Each release archive carries all three files.
