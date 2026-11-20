#!/bin/sh
# Crate boundary check: no GPUI or Windows-resource crate in core, storage or
# cli; no engine, network, keychain or write code in the CLI sources.
set -u
cd "$(dirname "$0")/.." || exit 2
fail=0

for c in wisp-core wisp-storage wisp-service wisp-cli; do
    tree=$(cargo tree -p "$c" -e normal,build --target all --all-features --locked --prefix none 2>&1) || {
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

for pat in 'net::' 'TcpStream' 'UdpSocket' 'process::Command' 'Command::new' \
    'File::create' 'OpenOptions' 'fs::write' 'fs::remove' '#[path' 'include!' \
    'include_str!' 'include_bytes!' 'reqwest' 'tokio' 'Engine' 'keychain::' \
    'SecretStore' 'ensure_collection_id' 'atomic_write' 'create_dir'; do
    grep -rnF -e "$pat" crates/cli/src
    rc=$?
    if [ "$rc" -eq 0 ]; then
        echo "check-boundaries: crates/cli/src contains $pat" >&2
        fail=1
    elif [ "$rc" -ne 1 ]; then
        echo "check-boundaries: grep failed (status $rc) for $pat" >&2
        fail=1
    fi
done

# Exactly one Command::Send( outside core, in the service crate's send.rs.
sends=$(grep -rn 'Command::Send(' crates --include='*.rs' | grep -v '^crates/core/')
case "$sends" in
    crates/service/src/send.rs:*) ;;
    *) echo "check-boundaries: Command::Send( must occur only in crates/service/src/send.rs" >&2
       printf '%s\n' "$sends" >&2
       fail=1 ;;
esac
if [ "$(printf '%s\n' "$sends" | grep -c .)" -ne 1 ]; then
    echo "check-boundaries: Command::Send( must occur exactly once outside crates/core" >&2
    fail=1
fi

# The GUI plans through the service: no free-function resolve( above the tests, whether
# called bare, path-qualified (wisp_core::resolve::resolve( or resolve::resolve( ) or
# imported under another name (use wisp_core::resolve::resolve as r;).
if sed '/^#\[cfg(test)\]/,$d' crates/app/src/workspace.rs |
    grep -nE '(^|[^:.[:alnum:]_])resolve\(|resolve::resolve\(|(^|[^[:alnum:]_])resolve[[:space:]]+as[[:space:]]'; then
    echo "check-boundaries: crates/app/src/workspace.rs calls or aliases resolve" >&2
    fail=1
fi

# History entries are built by the service: no NewEntry { literal above the tests.
if sed '/^#\[cfg(test)\]/,$d' crates/app/src/workspace.rs | grep -nF 'NewEntry {'; then
    echo "check-boundaries: crates/app/src/workspace.rs builds NewEntry {" >&2
    fail=1
fi

[ "$fail" -eq 0 ] && echo "check-boundaries: ok"
exit "$fail"
