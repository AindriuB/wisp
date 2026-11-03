#!/usr/bin/env bash
# package.sh <platform> <binary> <outdir>
#   platform: macos-arm64 | windows-x64 | linux-x64
# Writes the release archive for one platform into <outdir> and prints its name.
# Run from the repository root (LICENSE, NOTICE, THIRD-PARTY-NOTICES live there).
# The version is the wisp-app version from `cargo metadata`.
set -euo pipefail
unset CDPATH

platform="${1:-}"; bin="${2:-}"; out="${3:-}"
[ -n "$platform" ] && [ -n "$bin" ] && [ -n "$out" ] || {
  echo "usage: package.sh <macos-arm64|windows-x64|linux-x64> <binary> <outdir>" >&2; exit 2; }
[ -f "$bin" ] || { echo "no such binary: $bin" >&2; exit 2; }

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
bin="$(cd "$(dirname "$bin")" && pwd)/$(basename "$bin")"
mkdir -p "$out"; out="$(cd "$out" && pwd)"

version="$(cd "$root" && cargo metadata --format-version 1 --no-deps --locked \
  | jq -r '.packages[] | select(.name=="wisp-app") | .version')"
[ -n "$version" ] && [ "$version" != null ] || { echo "cannot read wisp-app version" >&2; exit 1; }

name="Wisp-$version-$platform"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
dir="$stage/$name"
mkdir -p "$dir"
for f in LICENSE NOTICE THIRD-PARTY-NOTICES; do cp "$root/$f" "$dir/$f"; done

case "$platform" in
  macos-arm64)
    app="$dir/Wisp.app/Contents"
    mkdir -p "$app/MacOS"
    cp "$bin" "$app/MacOS/wisp"; chmod 0755 "$app/MacOS/wisp"
    # The binary's own minimum OS, from its load commands.
    minos="$(otool -l "$bin" | awk '
      $1=="cmd" && $2=="LC_BUILD_VERSION" {b=1; next}
      $1=="cmd" && $2=="LC_VERSION_MIN_MACOSX" {m=1; next}
      b && $1=="minos" {print $2; exit}
      m && $1=="version" {print $2; exit}')"
    [ -n "$minos" ] || { echo "cannot read minimum OS from $bin" >&2; exit 1; }
    sed -e "s/@VERSION@/$version/g" -e "s/@MINOS@/$minos/g" \
      "$here/Info.plist.template" > "$app/Info.plist"
    plutil -lint "$app/Info.plist"
    codesign --force --sign - "$dir/Wisp.app"
    asset="$name.zip"; rm -f "$out/$asset"
    (cd "$stage" && ditto -c -k --keepParent --norsrc --noextattr --noqtn --noacl "$name" "$out/$asset")
    ;;
  windows-x64)
    cp "$bin" "$dir/wisp.exe"
    asset="$name.zip"; rm -f "$out/$asset"
    (cd "$stage" && 7z a -tzip -bd "$out/$asset" "$name" >/dev/null)
    ;;
  linux-x64)
    cp "$bin" "$dir/wisp"; chmod 0755 "$dir/wisp"
    asset="$name.tar.gz"; rm -f "$out/$asset"
    tar -C "$stage" -czf "$out/$asset" "$name"
    ;;
  *) echo "unknown platform: $platform" >&2; exit 2 ;;
esac
echo "$asset"
