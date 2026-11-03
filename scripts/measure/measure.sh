#!/usr/bin/env bash
# measure.sh <binary> <output.json>   (macOS, Linux)
# Env: TARGET (default: rustc host triple), RENDERER (hardware|software|unknown,
#      default unknown), SETTLE_SECONDS (default 5), SCREENSHOT=<path.png>
#      (Linux with DISPLAY set: save the X root window after the request run's
#      sample; a failure prints one `screenshot: <reason>` line and changes
#      nothing else). On Linux <output stem>.libs.txt always lists the shared
#      objects the running app had mapped.
#
# Method: serve a fixed 64 KiB file from `python3 -m http.server` on 127.0.0.1.
# Launch the binary once without the hook, wait settle_seconds, sample RSS with
# `ps -o rss=`, kill. Launch it again with WISP_AUTOSEND_URL, wait for the
# `WISP_EVENT completed` stdout line (30 s max), wait settle_seconds, sample
# RSS, kill. Fixed waits are fine: this is a measurement, not a test.
# If the window fails to start or no WISP_EVENT line arrives, write
# status "unmeasured" with a reason, exit 1, and record no numbers.
# The waiting stops early when the app prints `WISP_EVENT failed` or exits.
# Each launch's stdout/stderr are kept next to <output.json> as
# <output stem>.{idle,req}.{out,err}.log; on "unmeasured" the last 20 stderr
# lines go into the JSON key stderr_tail and the job log. The app runs with
# RUST_BACKTRACE=1 and RUST_LOG=info. On macOS a request-run timeout also
# saves `sample` output as <output stem>.req.sample.log.
# On Linux the headline memory figures are pss_kib and rss_anon_kib (Pss and
# Anonymous from /proc/<pid>/smaps_rollup, each {idle, after_request}); plain
# RSS also counts shared driver pages. renderer_device is the GPU/adapter name
# from the app log or `vulkaninfo --summary`; null with renderer_device_reason
# when neither gives one. renderer_device_source says which: "app_log" or
# "vulkaninfo" when renderer_device is set, otherwise null. Those keys, and
# private_working_set_kib, are null where not applicable (macOS; Windows has
# its own script).
# Budgets: scripts/measure/budgets.json (or $BUDGETS) gives the target's limits;
# the result JSON gets a `budget` object and `within_budget`. Exit codes: 2 usage,
# 1 unmeasured, 3 measured but over budget, 0 otherwise.
set -u
# A set CDPATH makes `cd` print the directory, which would corrupt $(cd ... && pwd).
unset CDPATH

bin="${1:-}"; out="${2:-}"
[ -n "$bin" ] && [ -n "$out" ] || { echo "usage: measure.sh <binary> <output.json>" >&2; exit 2; }
[ -f "$bin" ] || { echo "no such binary: $bin" >&2; exit 2; }
# Absolute path, so a bare name is never looked up on PATH.
bin="$(cd "$(dirname "$bin")" && pwd)/$(basename "$bin")"

budgets="${BUDGETS:-$(cd "$(dirname "$0")" && pwd)/budgets.json}"
[ -f "$budgets" ] || { echo "no such budgets file: $budgets" >&2; exit 2; }
python3 -c 'import json, sys
with open(sys.argv[1]) as f:
    sys.exit(0 if isinstance(json.load(f), dict) else 1)' "$budgets" 2>/dev/null \
  || { echo "budgets file is not valid JSON: $budgets" >&2; exit 2; }

settle="${SETTLE_SECONDS:-5}"
renderer="${RENDERER:-unknown}"
target="${TARGET:-$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')}"
commit="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
os_version="$(uname -sr)"
if [ "$(uname -s)" = Darwin ]; then os_version="macOS $(sw_vers -productVersion) $(uname -m)"; fi
bytes="$(wc -c <"$bin" | tr -d ' ')"

mkdir -p "$(dirname "$out")"
logbase="${out%.json}"
export RUST_BACKTRACE=1 RUST_LOG=info

work="$(mktemp -d)"
server_pid=""; app_pid=""
# shellcheck disable=SC2329  # invoked via trap, not by name
cleanup() {
  [ -n "$app_pid" ] && kill "$app_pid" 2>/dev/null
  [ -n "$server_pid" ] && kill "$server_pid" 2>/dev/null
  rm -rf "$work"
}
trap cleanup EXIT

# Linux-only extras, filled in as they are sampled.
pss_idle=""; pss_after=""; anon_idle=""; anon_after=""
device=""; device_source=""
if [ "$(uname -s)" = Linux ]; then
  device_reason="not sampled: run failed before the renderer was probed"
else
  device_reason="not applicable on this OS"
fi

# write_json <status> <reason|""> <idle> <after> [stderr-file]
write_json() {
  mkdir -p "$(dirname "$out")"
  python3 - "$target" "$commit" "$bytes" "$3" "$4" "$settle" "$renderer" "$1" "$2" "$os_version" "${5:-}" \
    "$pss_idle" "$pss_after" "$anon_idle" "$anon_after" "$device" "$device_reason" "$budgets" "$device_source" >"$out" <<'PY'
import json, sys
a = sys.argv[1:]
num = lambda s: int(s) if s else None
pair = lambda i, j: {"idle": num(a[i]), "after_request": num(a[j])} if (a[i] or a[j]) else None
tail = None
if a[10]:
    try:
        with open(a[10], errors="replace") as f:
            tail = f.read().splitlines()[-20:]
    except OSError:
        tail = []
res = {
    "target": a[0], "commit": a[1], "binary_bytes": num(a[2]),
    "idle_rss_kib": num(a[3]), "after_request_rss_kib": num(a[4]),
    "settle_seconds": int(a[5]), "renderer": a[6], "status": a[7],
    "reason": a[8] or None, "os_version": a[9],
    "stderr_tail": tail,
    "pss_kib": pair(11, 12), "rss_anon_kib": pair(13, 14),
    "renderer_device": a[15] or None,
    "renderer_device_source": (a[18] or None) if a[15] else None,
    "renderer_device_reason": None if a[15] else (a[16] or None),
    "private_working_set_kib": None,
}
def lookup(path):
    v = res
    for part in path.split("."):
        v = v.get(part) if isinstance(v, dict) else None
    return v
with open(a[17]) as f:
    entry = json.load(f).get(a[0])
if entry is None:
    res["budget"] = None
    res["within_budget"] = None
else:
    budget = {}
    for key in ("binary_bytes", "idle", "after_request"):
        m, lim = entry[key]["metric"], entry[key]["limit"]
        val = lookup(m)
        budget[key] = {"metric": m, "limit": lim, "value": val,
                       "within": None if val is None else val <= lim}
    w = [b["within"] for b in budget.values()]
    res["budget"] = budget
    res["within_budget"] = False if False in w else (True if all(x is True for x in w) else None)
print(json.dumps(res, indent=2))
PY
}
# err_file is the stderr log of the launch in progress (empty before any launch).
err_file=""
fail() {
  local reason="$1"
  if [ -n "$err_file" ]; then
    if [ -s "$err_file" ]; then
      reason="$reason (stderr: $(wc -l <"$err_file" | tr -d ' ') lines, tail in stderr_tail)"
      echo "--- last 20 lines of app stderr ---" >&2
      tail -n 20 "$err_file" >&2
      echo "--- end of app stderr ---" >&2
    else
      reason="$reason (app stderr was empty)"
    fi
  fi
  echo "unmeasured: $reason" >&2
  write_json unmeasured "$reason" "" "" "$err_file"
  exit 1
}

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
# smaps_field <pid> <Pss|Anonymous>: kB from smaps_rollup (Linux), else empty.
smaps_field() { awk -v k="$2:" '$1 == k { print $2 }' "/proc/$1/smaps_rollup" 2>/dev/null; }

# find_device <err-log>...: adapter name from GPUI's "Selected GPU" log line,
# else the first Vulkan device from vulkaninfo (not necessarily GPUI's choice).
find_device() {
  local line=""
  line="$(sed -n 's/.*Selected GPU (passed configuration test):[[:space:]]*//p' "$@" 2>/dev/null | head -n 1)"
  line="$(printf '%s' "$line" | sed 's/[[:space:]]*([^()]*)[[:space:]]*$//' | cut -c1-200)"
  if [ -n "$line" ]; then device="$line"; device_source=app_log; return; fi
  if command -v vulkaninfo >/dev/null 2>&1; then
    line="$(vulkaninfo --summary 2>/dev/null | sed -n 's/^[[:space:]]*deviceName[[:space:]]*=[[:space:]]*//p' | head -n 1)"
    if [ -n "$line" ]; then device="$line"; device_source=vulkaninfo; return; fi
    device_reason="vulkaninfo gave no deviceName and the app log named no adapter"
  else
    device_reason="vulkaninfo not installed and the app log named no adapter"
  fi
}

# Run 1: idle.
err_file="$logbase.idle.err.log"
"$bin" >"$logbase.idle.out.log" 2>"$err_file" &
app_pid=$!
sleep "$settle"
kill -0 "$app_pid" 2>/dev/null || fail "window failed to start (process exited during idle run)"
idle="$(rss_of "$app_pid")"
if [ "$(uname -s)" = Linux ]; then
  pss_idle="$(smaps_field "$app_pid" Pss)"; anon_idle="$(smaps_field "$app_pid" Anonymous)"
fi
kill "$app_pid" 2>/dev/null; wait "$app_pid" 2>/dev/null; app_pid=""
[ -n "$idle" ] || fail "could not sample idle RSS"

# Run 2: after one request.
err_file="$logbase.req.err.log"
req_out="$logbase.req.out.log"
WISP_AUTOSEND_URL="http://127.0.0.1:$port/file.bin" "$bin" >"$req_out" 2>"$err_file" &
app_pid=$!
got=""; alive=1
for _ in $(seq 1 300); do
  grep -q '^WISP_EVENT completed' "$req_out" && { got=1; break; }
  failed="$(grep -m1 '^WISP_EVENT failed' "$req_out")"
  [ -z "$failed" ] || fail "$failed"
  if ! kill -0 "$app_pid" 2>/dev/null; then alive=0; break; fi
  sleep 0.1
done
if [ -z "$got" ] && [ "$alive" = 0 ]; then
  # Exited: re-check the log once for a line written just before exit.
  grep -q '^WISP_EVENT completed' "$req_out" && got=1
  failed="$(grep -m1 '^WISP_EVENT failed' "$req_out")"
  [ -n "$got" ] || [ -z "$failed" ] || fail "$failed"
  if [ -z "$got" ]; then
    code=0; wait "$app_pid" 2>/dev/null || code=$?
    app_pid=""
    fail "app exited (code $code) before WISP_EVENT"
  fi
fi
if [ -z "$got" ]; then
  # Timed out with the app still running: on macOS grab a stack sample.
  if [ "$(uname -s)" = Darwin ] && kill -0 "$app_pid" 2>/dev/null; then
    sample "$app_pid" 3 -file "$logbase.req.sample.log" >/dev/null 2>&1 || true
  fi
  fail "no 'WISP_EVENT completed' line within 30 s"
fi
sleep "$settle"
kill -0 "$app_pid" 2>/dev/null || fail "app exited after the request"
after="$(rss_of "$app_pid")"
if [ "$(uname -s)" = Linux ]; then
  pss_after="$(smaps_field "$app_pid" Pss)"; anon_after="$(smaps_field "$app_pid" Anonymous)"
  # After the samples, before kill: these cannot change the measured figures.
  { awk '$6 ~ /^\// && $6 ~ /\.so(\.|$)/ { print $6 }' "/proc/$app_pid/maps" | sort -u >"$logbase.libs.txt"; } 2>/dev/null || true
  if [ -n "${SCREENSHOT:-}" ] && [ -n "${DISPLAY:-}" ]; then
    if ! command -v import >/dev/null 2>&1; then
      echo "screenshot: 'import' (ImageMagick) not installed" >&2
    elif ! shot_err="$(import -window root "$SCREENSHOT" 2>&1)"; then
      echo "screenshot: $(printf '%s' "$shot_err" | head -n 1 | cut -c1-200)" >&2
    fi
  fi
fi
kill "$app_pid" 2>/dev/null; wait "$app_pid" 2>/dev/null; app_pid=""
[ -n "$after" ] || fail "could not sample RSS after request"

if [ "$(uname -s)" = Linux ]; then
  find_device "$logbase.idle.err.log" "$logbase.req.err.log" "$logbase.idle.out.log"
fi
write_json measured "" "$idle" "$after"
cat "$out"
python3 - "$out" <<'PY'
import json, sys
r = json.load(open(sys.argv[1]))
if r.get("within_budget") is False:
    for k, b in r["budget"].items():
        if b["within"] is False:
            print(f"over budget: {k} {b['value']} > {b['limit']}", file=sys.stderr)
    sys.exit(3)
PY
exit $?
