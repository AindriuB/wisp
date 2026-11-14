# Wisp 0.2.0

Wisp is a portable, lightweight HTTP client with collections stored as TOML files. 0.2.0 closes most of the gaps with Postman, Bruno and Insomnia while keeping the single small binary.

## What's new

- **Importers.** Bring in a curl command, a Postman v2.1 collection or an OpenAPI 3 document (JSON or YAML). OpenAPI servers become environments. Constructs that cannot be converted, such as Postman scripts, are listed in a warnings report.
- **More authentication.** Digest (RFC 7616), AWS Signature V4, and OAuth2 with client credentials or authorization code plus PKCE through your system browser.
- **Phase timings.** The response panel shows DNS and connect+TLS time with first byte and total. Reused connections show no connect time.
- **History.** A history panel records each send per collection (500 entries, bodies cut at 1 MiB). Open an entry read-only, Resend it against the current environment, or Clear history.
- **Cookie jar.** A per-collection jar with a Cookies manager. Saving cookies to disk is off by default and is a per-collection toggle.
- **Several collections.** Up to 8 open at once, each with its own tree, environment and tabs, switched from the sidebar.
- **Keychain secrets.** Optionally keep an environment's secrets in the OS keychain instead of a secrets file. Without a keychain, Wisp falls back to the file and shows a banner.

## Fixed since 0.1.0

0.1.1 was never released. Its fixes (tasks 83 to 89) are included here: Wisp asks before discarding unsaved changes on quit or close; secrets no longer appear in debug output; streamed bodies are resent on 307/308 redirects; Windows-unsafe and aliasing file names are rejected; the Linux window has an icon and a launcher entry; and the sidebar and panes fit narrow windows.

## Security hardening

- Credentials are not sent to another site on a redirect: only a short list of safe headers crosses origins, and the redirect method rules match curl and reqwest.
- Request errors no longer include the request URL, which can carry secrets.
- History never stores auth values, credential-bearing headers or Set-Cookie values, and Resend refuses entries that would need them.
- Importers do not carry passwords from URLs or secret references into committed files, and they limit input size and nesting against hostile files.
- Wisp refuses to follow symlinks out of a collection for secrets, environment and collection files, and a failed reload clears secrets that were loaded.
- History and cookie files are private to your user. Keychain failures fail closed: the variables stay unresolved and Send is blocked.

## Known limitations

- The Windows executable is not signed, so SmartScreen warns on first launch ("More info", then "Run anyway").
- Keychain behaviour with the signed, notarised macOS build is untested until the release hand test.
- Linux has not been tested by hand on a real GPU.
- Not included: scripting, GraphQL, global variables, a command-line runner, Postman script translation, and a TCP/TLS timing split. OAuth2 access tokens are never saved to disk.

## Install and checksum

Assets are `Wisp-0.2.0-macos-arm64.zip`, `Wisp-0.2.0-windows-x64.zip` and `Wisp-0.2.0-linux-x64.tar.gz`, with `SHA256SUMS`. Check each archive against `SHA256SUMS` before unpacking. Per-platform steps, Linux library requirements and where Wisp writes are in the [README](https://github.com/AindriuB/wisp/blob/main/README.md).
