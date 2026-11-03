#!/usr/bin/env bash
# Temporary signing keychain for the macOS release job.
#   macos-keychain.sh setup    needs APPLE_DEVELOPER_ID_P12 (base64) and
#                              APPLE_DEVELOPER_ID_P12_PASSWORD in the env
#   macos-keychain.sh cleanup  safe when nothing was created
# State lives in $RUNNER_TEMP/wisp-sign; the identity hash and keychain path
# are exported to later steps through $GITHUB_ENV (neither is secret).
# Never run with -x: secrets pass through this script.
set -euo pipefail

tmp="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
state="$tmp/wisp-sign"
keychain="$tmp/wisp-signing.keychain-db"
team="${APPLE_TEAM_ID:-UB89GDN343}"

setup() {
  umask 077
  mkdir -p "$state"
  # Remember the user's search list so cleanup can restore it.
  security list-keychains -d user | sed -e 's/^[[:space:]]*"//' -e 's/"$//' > "$state/search-list"

  local pw p12="$state/developer-id.p12"
  pw="$(openssl rand -hex 24)"
  echo "::add-mask::$pw"
  printf '%s' "$APPLE_DEVELOPER_ID_P12" | base64 --decode > "$p12"

  security create-keychain -p "$pw" "$keychain"
  security set-keychain-settings -lut 21600 "$keychain"
  security unlock-keychain -p "$pw" "$keychain"
  security import "$p12" -k "$keychain" -P "$APPLE_DEVELOPER_ID_P12_PASSWORD" -T /usr/bin/codesign >/dev/null
  rm -f "$p12"
  security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$pw" "$keychain" >/dev/null
  local -a existing=()
  while IFS= read -r l; do [ -n "$l" ] && existing+=("$l"); done < "$state/search-list"
  security list-keychains -d user -s "$keychain" ${existing[@]+"${existing[@]}"}

  local ids n
  ids="$(security find-identity -v -p codesigning "$keychain" \
    | grep 'Developer ID Application' | grep "($team)" || true)"
  n="$(printf '%s' "$ids" | grep -c . || true)"
  if [ "$n" != 1 ]; then
    echo "expected exactly one Developer ID Application identity for team $team, found $n" >&2
    exit 1
  fi
  local hash
  hash="$(printf '%s' "$ids" | awk '{print $2}')"
  echo "signing identity SHA-1: $hash"
  if [ -n "${GITHUB_ENV:-}" ]; then
    {
      echo "WISP_KEYCHAIN=$keychain"
      echo "WISP_SIGN_IDENTITY=$hash"
    } >> "$GITHUB_ENV"
  fi
}

cleanup() {
  if [ -f "$state/search-list" ]; then
    local -a existing=()
    while IFS= read -r l; do [ -n "$l" ] && existing+=("$l"); done < "$state/search-list"
    if [ "${#existing[@]}" -gt 0 ]; then
      security list-keychains -d user -s "${existing[@]}" || true
    fi
  fi
  [ -e "$keychain" ] && { security delete-keychain "$keychain" || true; }
  rm -rf "$state" "$tmp/wisp-notary"
  return 0
}

case "${1:-}" in
  setup) setup ;;
  cleanup) cleanup ;;
  *) echo "usage: $0 setup|cleanup" >&2; exit 2 ;;
esac
