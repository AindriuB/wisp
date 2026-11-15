#!/bin/sh
# Crate boundary check: no GPUI or Windows-resource crate in core, storage or
# cli; no engine, network, keychain or write code in the CLI sources.
set -u
cd "$(dirname "$0")/.." || exit 2
fail=0

for c in wisp-core wisp-storage wisp-cli; do
    tree=$(cargo tree -p "$c" -e normal,build --locked --prefix none 2>&1) || {
        echo "check-boundaries: cargo tree failed for $c" >&2
        echo "$tree" >&2
        fail=1
        continue
    }
    for bad in gpui gpui-component gpui-platform gpui-pre gpui-pre-platform winresource; do
        if printf '%s\n' "$tree" | grep -q "^$bad "; then
            echo "check-boundaries: $c depends on $bad" >&2
            fail=1
        fi
    done
done

for pat in 'Engine' 'Command::' 'reqwest' 'tokio' 'std::net' 'keychain' \
    'ensure_collection_id' 'atomic_write' 'fs::write' 'create_dir'; do
    if grep -rnF -e "$pat" crates/cli/src; then
        echo "check-boundaries: crates/cli/src contains $pat" >&2
        fail=1
    fi
done

[ "$fail" -eq 0 ] && echo "check-boundaries: ok"
exit "$fail"
