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
    # Icon: wisp.icns is the CFBundleIconFile fallback; the layered Icon Composer
    # bundle is compiled into Assets.car (CFBundleIconName=AppIcon), which carries
    # the dark/tinted/clear appearances on macOS 26+.
    mkdir -p "$app/Resources"
    cp "$root/assets/icon/wisp.icns" "$app/Resources/wisp.icns"
    actool_out="$stage/actool"; mkdir -p "$actool_out"
    if ! xcrun actool "$root/assets/icon/AppIcon.icon" --compile "$actool_out" \
      --platform macosx --minimum-deployment-target "$minos" --app-icon AppIcon \
      --output-partial-info-plist "$actool_out/partial.plist" >"$actool_out/actool.log" 2>&1; then
      cat "$actool_out/actool.log" >&2; echo "actool failed" >&2; exit 1
    fi
    [ -f "$actool_out/Assets.car" ] || { cat "$actool_out/actool.log" >&2; echo "actool produced no Assets.car" >&2; exit 1; }
    # To a file, not a pipe: grep -q exiting early would SIGPIPE assetutil under pipefail.
    xcrun assetutil --info "$actool_out/Assets.car" > "$actool_out/assets.json"
    grep -q '"Name" : "AppIcon"' "$actool_out/assets.json" \
      || { cat "$actool_out/actool.log" >&2; echo "AppIcon missing from Assets.car" >&2; exit 1; }
    cp "$actool_out/Assets.car" "$app/Resources/Assets.car"
    # WISP_MACOS_SIGN=developer-id signs, notarises and staples; unset or adhoc
    # keeps the ad-hoc signature.
    "$here/sign-macos.sh" "$dir/Wisp.app"
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
    mkdir -p "$dir/icons"
    for z in 48 64 128 256 512; do cp "$root/assets/icon/png/wisp-$z.png" "$dir/icons/wisp-$z.png"; done
    asset="$name.tar.gz"; rm -f "$out/$asset"
    tar --owner=0 --group=0 --numeric-owner -C "$stage" -czf "$out/$asset" "$name"
    ;;
  *) echo "unknown platform: $platform" >&2; exit 2 ;;
esac
echo "$asset"
