#!/usr/bin/env bash
# measure.sh <binary> <output.json>   (macOS, Linux)
# Env: TARGET (default: rustc host triple), RENDERER (hardware|software|unknown,
#      default unknown), SETTLE_SECONDS (default 5).
#
# Method: serve a fixed 64 KiB file from `python3 -m http.server` on 127.0.0.1.
# Launch the binary once without the hook, wait settle_seconds, sample RSS with
# `ps -o rss=`, kill. Launch it again with WISP_AUTOSEND_URL, wait for the
# `WISP_EVENT completed` stdout line (30 s max), wait settle_seconds, sample
# RSS, kill. Fixed waits are fine: this is a measurement, not a test.
# If the window fails to start or no WISP_EVENT line arrives, write
# status "unmeasured" with a reason, exit non-zero, and record no numbers.
set -u

bin="${1:-}"; out="${2:-}"
[ -n "$bin" ] && [ -n "$out" ] || { echo "usage: measure.sh <binary> <output.json>" >&2; exit 2; }
[ -f "$bin" ] || { echo "no such binary: $bin" >&2; exit 2; }

settle="${SETTLE_SECONDS:-5}"
renderer="${RENDERER:-unknown}"
target="${TARGET:-$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')}"
commit="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
os_version="$(uname -sr)"
if [ "$(uname -s)" = Darwin ]; then os_version="macOS $(sw_vers -productVersion) $(uname -m)"; fi
bytes="$(wc -c <"$bin" | tr -d ' ')"

work="$(mktemp -d)"
server_pid=""; app_pid=""
cleanup() {
  [ -n "$app_pid" ] && kill "$app_pid" 2>/dev/null
  [ -n "$server_pid" ] && kill "$server_pid" 2>/dev/null
  rm -rf "$work"
}
trap cleanup EXIT

# write_json <status> <reason|""> <idle> <after>
write_json() {
  mkdir -p "$(dirname "$out")"
  python3 - "$target" "$commit" "$bytes" "$3" "$4" "$settle" "$renderer" "$1" "$2" "$os_version" >"$out" <<'PY'
import json, sys
a = sys.argv[1:]
num = lambda s: int(s) if s else None
print(json.dumps({
    "target": a[0], "commit": a[1], "binary_bytes": num(a[2]),
    "idle_rss_kib": num(a[3]), "after_request_rss_kib": num(a[4]),
    "settle_seconds": int(a[5]), "renderer": a[6], "status": a[7],
    "reason": a[8] or None, "os_version": a[9],
}, indent=2))
PY
}
fail() { echo "unmeasured: $1" >&2; write_json unmeasured "$1" "" ""; exit 1; }

head -c 65536 /dev/zero >"$work/file.bin"
port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')"
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$work" >/dev/null 2>&1 &
server_pid=$!
for _ in $(seq 1 50); do
  curl -sf -o /dev/null "http://127.0.0.1:$port/file.bin" && break
  sleep 0.1
done
curl -sf -o /dev/null "http://127.0.0.1:$port/file.bin" || fail "local http server did not start"

rss_of() { ps -o rss= -p "$1" | tr -d ' '; }

# Run 1: idle.
"$bin" >"$work/idle.out" 2>&1 &
app_pid=$!
sleep "$settle"
kill -0 "$app_pid" 2>/dev/null || fail "window failed to start (process exited during idle run)"
idle="$(rss_of "$app_pid")"
kill "$app_pid" 2>/dev/null; wait "$app_pid" 2>/dev/null; app_pid=""
[ -n "$idle" ] || fail "could not sample idle RSS"

# Run 2: after one request.
WISP_AUTOSEND_URL="http://127.0.0.1:$port/file.bin" "$bin" >"$work/req.out" 2>&1 &
app_pid=$!
got=""
for _ in $(seq 1 300); do
  if grep -q '^WISP_EVENT completed' "$work/req.out"; then got=1; break; fi
  kill -0 "$app_pid" 2>/dev/null || break
  sleep 0.1
done
[ -n "$got" ] || fail "no 'WISP_EVENT completed' line within 30 s"
sleep "$settle"
kill -0 "$app_pid" 2>/dev/null || fail "app exited after the request"
after="$(rss_of "$app_pid")"
kill "$app_pid" 2>/dev/null; wait "$app_pid" 2>/dev/null; app_pid=""
[ -n "$after" ] || fail "could not sample RSS after request"

write_json measured "" "$idle" "$after"
cat "$out"
