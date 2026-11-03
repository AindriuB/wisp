#!/usr/bin/env bash
# check-glibc.sh <binary> [max-glibc-version, default 2.39]
# Prints the highest GLIBC_x.y[.z] version the ELF binary requires. Exits 0 when
# it is <= max, 1 (naming the version and the symbols that need it) when above,
# 2 on bad usage.
set -euo pipefail
unset CDPATH

bin="${1:-}"; max="${2:-2.39}"
[ -n "$bin" ] || { echo "usage: check-glibc.sh <binary> [max-glibc-version]" >&2; exit 2; }
[ -f "$bin" ] || { echo "no such binary: $bin" >&2; exit 2; }
[[ "$max" =~ ^[0-9]+(\.[0-9]+)+$ ]] || { echo "bad max version: $max" >&2; exit 2; }

# One line per dynamic symbol: "... GLIBC_2.43  asinf" (defined and required alike).
dyn="$(objdump -T "$bin" 2>&1)" || { echo "objdump failed on $bin (not an ELF binary?)" >&2; exit 2; }
versions="$(printf '%s\n' "$dyn" | grep -oE 'GLIBC_[0-9]+(\.[0-9]+)+' | sed 's/^GLIBC_//' | sort -uV || true)"
[ -n "$versions" ] || { echo "no GLIBC_ version requirements found in $bin" >&2; exit 2; }

highest="$(printf '%s\n' "$versions" | tail -n 1)"
echo "highest GLIBC required: $highest (max allowed: $max)"

# sort -V orders 2.4 < 2.39 < 2.43 numerically per component.
if [ "$(printf '%s\n%s\n' "$highest" "$max" | sort -V | tail -n 1)" = "$max" ]; then
  exit 0
fi

echo "FAIL: $bin needs GLIBC_$highest, above the allowed GLIBC_$max. Offending symbols:" >&2
printf '%s\n' "$dyn" | awk 'match($0, /GLIBC_[0-9]+(\.[0-9]+)+/) { print substr($0, RSTART, RLENGTH), $NF }' \
  | while read -r v sym; do
      ver="${v#GLIBC_}"
      if [ "$(printf '%s\n%s\n' "$ver" "$max" | sort -V | tail -n 1)" != "$max" ]; then
        echo "  $v $sym"
      fi
    done | sort -u >&2
exit 1
