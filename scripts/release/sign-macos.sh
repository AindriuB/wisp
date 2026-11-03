#!/usr/bin/env bash
# Sign, notarise, staple and verify a Wisp.app.  usage: sign-macos.sh <Wisp.app>
# Mode comes from WISP_MACOS_SIGN: developer-id | adhoc (default adhoc).
# developer-id needs WISP_SIGN_IDENTITY, WISP_KEYCHAIN (see macos-keychain.sh),
# APPLE_API_KEY_P8 (base64), APPLE_API_KEY_ID and APPLE_API_ISSUER_ID.
# Never run with -x: secrets pass through this script.
set -euo pipefail

app="${1:?usage: sign-macos.sh <Wisp.app>}"
mode="${WISP_MACOS_SIGN:-adhoc}"
here="$(cd "$(dirname "$0")" && pwd)"
bin="$app/Contents/MacOS/wisp"

case "$mode" in
  adhoc)
    codesign --force --sign - "$app"
    codesign --verify --strict --deep --verbose=2 "$app"
    exit 0
    ;;
  developer-id) ;;
  *) echo "unknown WISP_MACOS_SIGN: $mode" >&2; exit 2 ;;
esac

umask 077
tmp="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
work="$tmp/wisp-notary"
mkdir -p "$work"
trap 'rm -rf "$work"' EXIT
zip="$work/Wisp-notarize.zip"
p8="$work/AuthKey.p8"

ent=()
[ -f "$here/wisp.entitlements" ] && ent=(--entitlements "$here/wisp.entitlements")

# Inside out, no --deep.
for target in "$bin" "$app"; do
  codesign --force --options runtime --timestamp --keychain "$WISP_KEYCHAIN" \
    --sign "$WISP_SIGN_IDENTITY" ${ent[@]+"${ent[@]}"} "$target"
done

printf '%s' "$APPLE_API_KEY_P8" | base64 --decode > "$p8"
ditto -c -k --keepParent "$app" "$zip"
result="$work/result.json"
# notarytool exits non-zero on some failures; the status check below decides.
xcrun notarytool submit "$zip" --key "$p8" --key-id "$APPLE_API_KEY_ID" \
  --issuer "$APPLE_API_ISSUER_ID" --wait --timeout 90m --output-format json > "$result" || true
id="$(plutil -extract id raw -o - "$result" 2>/dev/null || true)"
status="$(plutil -extract status raw -o - "$result" 2>/dev/null || true)"
echo "notarisation submission: ${id:-unknown} status: ${status:-unknown}"
if [ "$status" != "Accepted" ]; then
  if [ -n "$id" ]; then
    xcrun notarytool log "$id" --key "$p8" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER_ID" >&2 || true
  fi
  if [ -z "$status" ] || [ "$status" = "In Progress" ]; then
    echo "notarisation did not finish within 90m (or notarytool failed); submission id: ${id:-unknown}." >&2
    echo "Check later with: xcrun notarytool info|log ${id:-<id>} --key <p8> --key-id <id> --issuer <issuer>" >&2
  fi
  echo "notarisation not accepted" >&2
  exit 1
fi
xcrun stapler staple "$app"
xcrun stapler validate "$app"

codesign --verify --strict --deep --verbose=2 "$app"
assess="$(spctl --assess --type execute -vv "$app" 2>&1)" || { echo "$assess" >&2; exit 1; }
echo "$assess"
if ! { grep -q 'accepted' <<<"$assess" && grep -q 'source=Notarized Developer ID' <<<"$assess"; }; then
  echo "spctl did not report a notarised Developer ID app" >&2
  exit 1
fi
