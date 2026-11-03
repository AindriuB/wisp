#!/usr/bin/env bash
# generate.sh [--check]
# Regenerates every derived icon file under assets/icon/ from the SVG sources.
# Runs on a developer Mac (iconutil is macOS-only); CI uses the committed output.
#   --check  render to a temp dir and fail if it differs from the committed files.
# Needs: resvg (cargo install resvg --version 0.49.0 --locked), python3, iconutil.
# Sources are the project's own artwork (Apache-2.0), no third-party notice needed.
# Size cut: <= 48 px from wisp-small.svg, >= 64 px from wisp.svg.
# AppIcon.icon (macOS Icon Composer bundle, compiled by actool at package time)
# layers: a full-bleed dark background and wisp-layer.svg (W and sparkles).
set -euo pipefail
unset CDPATH

RESVG_VERSION=0.49.0
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
src="$(cd "$here/../../assets/icon" && pwd)"

mode=write
case "${1:-}" in
  "") ;;
  --check) mode=check ;;
  *) echo "usage: generate.sh [--check]" >&2; exit 2 ;;
esac

have="$(resvg --version 2>/dev/null || true)"
[ "$have" = "$RESVG_VERSION" ] || {
  echo "resvg $RESVG_VERSION required (found '${have:-none}'):" >&2
  echo "  cargo install resvg --version $RESVG_VERSION --locked" >&2; exit 1; }
command -v iconutil >/dev/null || { echo "iconutil not found (macOS only)" >&2; exit 1; }

tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
gen="$tmp/icon"; mkdir -p "$gen/png"

render() { # <size> <svg> <out>
  resvg -w "$1" -h "$1" "$2" "$3"
}
for z in 16 24 32 48; do render "$z" "$src/wisp-small.svg" "$gen/png/wisp-$z.png"; done
for z in 64 128 256 512 1024; do render "$z" "$src/wisp.svg" "$gen/png/wisp-$z.png"; done

# .icns
set_="$tmp/wisp.iconset"; mkdir -p "$set_"
p="$gen/png/wisp"
cp "$p-16.png"   "$set_/icon_16x16.png"
cp "$p-32.png"   "$set_/icon_16x16@2x.png"
cp "$p-32.png"   "$set_/icon_32x32.png"
cp "$p-64.png"   "$set_/icon_32x32@2x.png"
cp "$p-128.png"  "$set_/icon_128x128.png"
cp "$p-256.png"  "$set_/icon_128x128@2x.png"
cp "$p-256.png"  "$set_/icon_256x256.png"
cp "$p-512.png"  "$set_/icon_256x256@2x.png"
cp "$p-512.png"  "$set_/icon_512x512.png"
cp "$p-1024.png" "$set_/icon_512x512@2x.png"
iconutil -c icns "$set_" -o "$gen/wisp.icns"

# .ico
python3 -I "$here/make_ico.py" "$gen/wisp.ico" \
  "$p-16.png" "$p-24.png" "$p-32.png" "$p-48.png" "$p-64.png" "$p-128.png" "$p-256.png"

# AppIcon.icon layers
ic="$gen/AppIcon.icon"; mkdir -p "$ic/Assets"
cat > "$tmp/bg.svg" <<'S'
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512"><defs><radialGradient id="bg" cx="50%" cy="45%" r="65%"><stop offset="0%" stop-color="#161922"/><stop offset="100%" stop-color="#0b0c10"/></radialGradient></defs><rect width="512" height="512" fill="url(#bg)"/></svg>
S
render 1024 "$tmp/bg.svg" "$ic/Assets/background.png"
render 1024 "$src/wisp-layer.svg" "$ic/Assets/wisp.png"
cp "$here/icon.json" "$ic/icon.json"

if [ "$mode" = check ]; then
  fail=0
  while IFS= read -r f; do
    cmp -s "$gen/$f" "$src/$f" || { echo "stale or missing: assets/icon/$f" >&2; fail=1; }
  done < <(cd "$gen" && find . -type f | sed 's|^\./||' | sort)
  [ "$fail" = 0 ] && echo "icons up to date"
  exit "$fail"
fi

rm -rf "$src/png" "$src/AppIcon.icon" "$src/wisp.icns" "$src/wisp.ico"
cp -R "$gen/." "$src/"
echo "regenerated assets/icon"
