#!/usr/bin/env bash
# quit-check.sh <Wisp.app>   (macOS only)
# Checks that Wisp quits cleanly and leaves no crash report:
#  (a) launched, then asked to quit over AppleScript: the process must go away;
#  (b) run with WISP_CLOSE_WINDOW_AFTER_MS: the process must exit 0.
# The bundle id is read from the app's Info.plist, so a test copy with a
# different id never touches a Wisp the user is running. Only pids this script
# started are ever signalled.
set -euo pipefail
unset CDPATH

[ "$(uname -s)" = Darwin ] || { echo "quit-check: macOS only" >&2; exit 2; }
app="${1:-}"
[ -d "$app/Contents/MacOS" ] || { echo "usage: quit-check.sh <Wisp.app>" >&2; exit 2; }
bin="$app/Contents/MacOS/wisp"
[ -x "$bin" ] || { echo "no executable at $bin" >&2; exit 2; }
id="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist")"

reports="$HOME/Library/Logs/DiagnosticReports"
before="$(mktemp)"; pid=""
cleanup() { [ -n "$pid" ] && kill "$pid" 2>/dev/null || true; rm -f "$before"; }
trap cleanup EXIT
ls "$reports" 2>/dev/null | grep -E '^wisp.*\.ips$' | sort >"$before" || true

no_new_reports() {
  sleep 2
  local now new
  now="$(mktemp)"
  ls "$reports" 2>/dev/null | grep -E '^wisp.*\.ips$' | sort >"$now" || true
  new="$(comm -13 "$before" "$now")"; rm -f "$now"
  [ -z "$new" ] || { echo "FAIL ($1): new crash report(s): $new" >&2; exit 1; }
}

wait_gone() { # pid, seconds
  local i
  for ((i = 0; i < $2 * 10; i++)); do
    kill -0 "$1" 2>/dev/null || return 0
    sleep 0.1
  done
  return 1
}

echo "(a) AppleScript quit of $id"
"$bin" >/dev/null 2>&1 & pid=$!
sleep 3
kill -0 "$pid" 2>/dev/null || { echo "FAIL (a): app did not stay running" >&2; exit 1; }
osascript -e "tell application id \"$id\" to quit"
wait_gone "$pid" 5 || { echo "FAIL (a): still running 5 s after quit" >&2; exit 1; }
wait "$pid" 2>/dev/null || true; pid=""
no_new_reports a

echo "(b) close hook"
rc=0
WISP_CLOSE_WINDOW_AFTER_MS=1500 "$bin" >/dev/null 2>&1 & pid=$!
wait_gone "$pid" 5 || { echo "FAIL (b): still running 5 s after close" >&2; exit 1; }
wait "$pid" || rc=$?
pid=""
[ "$rc" -eq 0 ] || { echo "FAIL (b): exit status $rc" >&2; exit 1; }
no_new_reports b
echo "quit-check: OK"
