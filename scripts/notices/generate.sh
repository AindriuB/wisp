#!/usr/bin/env bash
# Regenerates THIRD-PARTY-NOTICES from Cargo.lock with cargo-about.
# CI (the `notices` job) runs this and fails if the file changes.
set -euo pipefail

CARGO_ABOUT_VERSION=0.9.3

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

have="$(cargo about --version 2>/dev/null | awk '{print $2}' || true)"
if [ "$have" != "$CARGO_ABOUT_VERSION" ]; then
  echo "cargo-about $CARGO_ABOUT_VERSION required (found '${have:-none}'):" >&2
  echo "  cargo install --locked cargo-about --version $CARGO_ABOUT_VERSION --features cli" >&2
  exit 1
fi

out="$root/THIRD-PARTY-NOTICES"
tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

# Make sure every crate source is in the local registry; cargo-about then runs offline.
cargo fetch --locked

cargo about generate --locked --offline \
  --config about.toml \
  --manifest-path crates/app/Cargo.toml \
  scripts/notices/template.hbs > "$tmp"

{
  cat "$tmp"
  cat <<'HDR'
================================================================================
Bundled assets
================================================================================

The 19 SVG icons in crates/app/assets/icons are embedded in the Wisp binary.
They are copied unmodified from gpui-kit-assets 0.8.1 (Apache-2.0). They are
not part of any Rust crate's license entry above, so their notices follow.

gpui-component 0.8.1 also embeds two Lucide SVG icons of its own; they fall
under the same Lucide ISC license text reproduced below.

--------------------------------------------------------------------------------

HDR
  cat crates/app/assets/icons/NOTICE
} > "$out"
