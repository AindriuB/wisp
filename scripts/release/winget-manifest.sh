#!/usr/bin/env bash
# winget-manifest.sh <version> <SHA256SUMS> <outdir>
# Writes the winget manifest for the Windows zip of a published release under
#   <outdir>/manifests/<letter>/<Publisher>/<Name>/<version>/
# Version, URL and SHA256 come from the release's own assets.
set -euo pipefail
unset CDPATH

# Defined once; the user confirms both (see packaging/winget/README.md).
PACKAGE_IDENTIFIER="${PACKAGE_IDENTIFIER:-AindriuB.Wisp}"
PUBLISHER="${PUBLISHER:-AindriuB}"

version="${1:-}"; sums="${2:-}"; out="${3:-}"
[ -n "$version" ] && [ -n "$sums" ] && [ -n "$out" ] || {
  echo "usage: winget-manifest.sh <version> <SHA256SUMS> <outdir>" >&2; exit 2; }
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
  echo "malformed version: $version" >&2; exit 2; }
[ -f "$sums" ] || { echo "no such file: $sums" >&2; exit 2; }

zip="Wisp-${version}-windows-x64.zip"
sha="$(awk -v z="$zip" '$2 == z || $2 == "*" z { print toupper($1); exit }' "$sums")"
[[ "$sha" =~ ^[0-9A-F]{64}$ ]] || {
  echo "no valid SHA256 line for $zip in $sums" >&2; exit 1; }
url="https://github.com/AindriuB/wisp/releases/download/v${version}/${zip}"

here="$(cd "$(dirname "$0")" && pwd)"
tpl="$here/../../packaging/winget/templates"
name="${PACKAGE_IDENTIFIER#*.}"
pubdir="${PACKAGE_IDENTIFIER%%.*}"
letter="$(printf '%s' "$PACKAGE_IDENTIFIER" | cut -c1 | tr '[:upper:]' '[:lower:]')"
dir="$out/manifests/$letter/$pubdir/$name/$version"
mkdir -p "$dir"

# Escape a value for the replacement side of a sed s||| command.
esc() { printf '%s' "$1" | sed -e 's/[\\&|]/\\&/g'; }
render() {
  sed -e "s|@PACKAGE_IDENTIFIER@|$(esc "$PACKAGE_IDENTIFIER")|g" \
      -e "s|@PUBLISHER@|$(esc "$PUBLISHER")|g" \
      -e "s|@VERSION@|$(esc "$version")|g" \
      -e "s|@URL@|$(esc "$url")|g" \
      -e "s|@SHA256@|$(esc "$sha")|g" "$1" > "$2"
}
render "$tpl/version.yaml.template"      "$dir/$PACKAGE_IDENTIFIER.yaml"
render "$tpl/installer.yaml.template"    "$dir/$PACKAGE_IDENTIFIER.installer.yaml"
render "$tpl/locale.en-US.yaml.template" "$dir/$PACKAGE_IDENTIFIER.locale.en-US.yaml"
echo "$dir"
