# measure.ps1 -Binary <wisp.exe> -Output <out.json>   (Windows)
# Env: TARGET (default x86_64-pc-windows-msvc), RENDERER (hardware|software|unknown,
#      default unknown), SETTLE_SECONDS (default 5), BUDGETS (path to the budgets
#      file, default budgets.json next to this script).
#
# Headline memory figure: private_working_set_kib {idle, after_request}, the
# process's private working set (Win32_PerfFormattedData WorkingSetPrivate).
# idle_rss_kib / after_request_rss_kib (WorkingSet64) and private_kib
# (PrivateMemorySize64, committed private bytes) are kept for continuity.
# pss_kib, rss_anon_kib, renderer_device and renderer_device_source are null
# here (Linux-only keys). renderer_device_source ("app_log"|"vulkaninfo") says
# where a Linux renderer_device came from.
#
# Method: serve a fixed 64 KiB file from `python3 -m http.server` on 127.0.0.1.
# Launch the binary once without the hook, wait settle_seconds, sample
# WorkingSet64 (idle_rss_kib) and PrivateMemorySize64 (private_kib), kill.
# Launch it again with WISP_AUTOSEND_URL, wait for the `WISP_EVENT completed`
# stdout line (30 s max), wait settle_seconds, sample again, kill. Fixed waits
# are fine: this is a measurement, not a test.
# If the window fails to start or no WISP_EVENT line arrives, write status
# "unmeasured" with a reason, exit 1, and record no numbers. The wait
# stops early when the app prints `WISP_EVENT failed` or exits, and any thrown
# exception also ends as "unmeasured" with its message as the reason.
# Each launch's stdout/stderr are kept next to -Output as
# <output stem>.{idle,req}.{out,err}.log; on "unmeasured" the last 20 stderr
# lines go into the JSON key stderr_tail and the job log. The app runs with
# RUST_BACKTRACE=1 and RUST_LOG=info.
# Budgets: scripts/measure/budgets.json (or $env:BUDGETS) gives the target's limits;
# the result JSON gets a `budget` object and `within_budget`. Exit codes: 2 usage
# (missing budgets file), 1 unmeasured, 3 measured but over budget, 0 otherwise.
param(
  [Parameter(Mandatory = $true)][string]$Binary,
  [Parameter(Mandatory = $true)][string]$Output
)
$ErrorActionPreference = 'Stop'
$Output = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Output)
$logBase = Join-Path (Split-Path -Parent $Output) ([System.IO.Path]::GetFileNameWithoutExtension($Output))
$env:RUST_BACKTRACE = '1'
$env:RUST_LOG = 'info'

$settle = if ($env:SETTLE_SECONDS) { [int]$env:SETTLE_SECONDS } else { 5 }
$renderer = if ($env:RENDERER) { $env:RENDERER } else { 'unknown' }
$target = if ($env:TARGET) { $env:TARGET } else { 'x86_64-pc-windows-msvc' }
$commit = (git rev-parse HEAD 2>$null); if (-not $commit) { $commit = 'unknown' }
$osVersion = [System.Environment]::OSVersion.VersionString
$bytes = $null
$errFile = $null
$budgetsFile = if ($env:BUDGETS) { $env:BUDGETS } else { Join-Path $PSScriptRoot 'budgets.json' }
if (-not (Test-Path -PathType Leaf $budgetsFile)) { [Console]::Error.WriteLine("no such budgets file: $budgetsFile"); exit 2 }
try { $budgetsData = Get-Content -Raw $budgetsFile | ConvertFrom-Json -NoEnumerate -ErrorAction Stop } catch { $budgetsData = $null }
if ($budgetsData -isnot [System.Management.Automation.PSCustomObject]) { [Console]::Error.WriteLine("budgets file is not valid JSON: $budgetsFile"); exit 2 }
$script:overBudget = @()

function Get-StderrTail {
  if (-not $errFile) { return $null }
  try { return , @(Get-Content -Path $errFile -Tail 20 -ErrorAction Stop | ForEach-Object { [string]$_ }) } catch { return , @() }
}

function Get-PrivateWs($procId) {
  try {
    $p = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Filter "IDProcess=$procId" -ErrorAction Stop | Select-Object -First 1
  } catch { $p = $null }
  if (-not $p) {
    [Console]::Error.WriteLine("warning: no private working set perf counter row for pid $procId")
    return $null
  }
  return [int64]($p.WorkingSetPrivate / 1KB)
}

function Write-Result($status, $reason, $idle, $idlePriv, $after, $afterPriv, $stderrTail, $idlePws = $null, $afterPws = $null) {
  $dir = Split-Path -Parent $Output
  if ($dir) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
  $res = [ordered]@{
    target = $target; commit = $commit; binary_bytes = $bytes
    idle_rss_kib = $idle; after_request_rss_kib = $after
    private_kib = [ordered]@{ idle = $idlePriv; after_request = $afterPriv }
    private_working_set_kib = [ordered]@{ idle = $idlePws; after_request = $afterPws }
    pss_kib = $null; rss_anon_kib = $null
    renderer_device = $null; renderer_device_source = $null; renderer_device_reason = 'not applicable on this OS'
    settle_seconds = $settle; renderer = $renderer; status = $status
    reason = $reason; os_version = $osVersion
    stderr_tail = $stderrTail
  }
  $entry = $budgetsData.$target
  $budget = $null; $within = $null
  if ($entry) {
    $budget = [ordered]@{}
    $flags = @()
    foreach ($key in 'binary_bytes', 'idle', 'after_request') {
      $m = $entry.$key.metric; $lim = [int64]$entry.$key.limit
      $val = $res
      foreach ($part in $m.Split('.')) { if ($null -ne $val) { $val = $val[$part] } }
      $ok = if ($null -eq $val) { $null } else { [int64]$val -le $lim }
      $budget[$key] = [ordered]@{ metric = $m; limit = $lim; value = $val; within = $ok }
      $flags += , $ok
      if ($ok -eq $false) { $script:overBudget += "over budget: $key $val > $lim" }
    }
    $within = if ($flags -contains $false) { $false } elseif (($flags | Where-Object { $_ -eq $true }).Count -eq 3) { $true } else { $null }
  }
  $res['budget'] = $budget
  $res['within_budget'] = $within
  $res | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 $Output
}

$work = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $work | Out-Null
$server = $null; $app = $null; $exitCode = 0
function Stop-All {
  if ($app -and -not $app.HasExited) { $app.Kill() }
  if ($server -and -not $server.HasExited) { $server.Kill() }
}
function Fail($reason) {
  Stop-All
  $tail = Get-StderrTail
  if ($errFile) {
    if ($tail -and $tail.Count -gt 0) {
      $reason = "$reason (stderr tail in stderr_tail)"
      [Console]::Error.WriteLine('--- last 20 lines of app stderr ---')
      $tail | ForEach-Object { [Console]::Error.WriteLine($_) }
      [Console]::Error.WriteLine('--- end of app stderr ---')
    } else {
      $reason = "$reason (app stderr was empty)"
    }
  }
  [Console]::Error.WriteLine("unmeasured: $reason")
  Write-Result 'unmeasured' $reason $null $null $null $null $tail
  exit 1
}

try {
  $Binary = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Binary)
  $bytes = (Get-Item $Binary).Length
  $dir = Split-Path -Parent $Output
  if ($dir) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
  [System.IO.File]::WriteAllBytes((Join-Path $work 'file.bin'), (New-Object byte[] 65536))
  $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
  $listener.Start(); $port = $listener.LocalEndpoint.Port; $listener.Stop()
  $server = Start-Process python3 -ArgumentList "-m http.server $port --bind 127.0.0.1 --directory `"$work`"" -PassThru -WindowStyle Hidden
  $url = "http://127.0.0.1:$port/file.bin"
  $up = $false
  for ($i = 0; $i -lt 50 -and -not $up; $i++) {
    try { Invoke-WebRequest -UseBasicParsing -Uri $url | Out-Null; $up = $true } catch { Start-Sleep -Milliseconds 100 }
  }
  if (-not $up) { Fail 'local http server did not start' }

  # Run 1: idle.
  $errFile = "$logBase.idle.err.log"
  $app = Start-Process $Binary -PassThru -RedirectStandardOutput "$logBase.idle.out.log" -RedirectStandardError $errFile
  $null = $app.Handle  # keep the handle so ExitCode stays readable
  Start-Sleep -Seconds $settle
  if ($app.HasExited) { Fail 'window failed to start (process exited during idle run)' }
  $app.Refresh()
  $idle = [int64]($app.WorkingSet64 / 1KB); $idlePriv = [int64]($app.PrivateMemorySize64 / 1KB)
  $idlePws = Get-PrivateWs $app.Id
  $app.Kill(); $app.WaitForExit(); $app = $null

  # Run 2: after one request.
  $env:WISP_AUTOSEND_URL = $url
  $reqOut = "$logBase.req.out.log"
  $errFile = "$logBase.req.err.log"
  $app = Start-Process $Binary -PassThru -RedirectStandardOutput $reqOut -RedirectStandardError $errFile
  $null = $app.Handle
  Remove-Item Env:WISP_AUTOSEND_URL
  $got = $false
  # Each pass: completed line, then failed line, then process exit. A final
  # pass after exit catches a line written just before the process ended.
  for ($i = 0; $i -lt 300 -and -not $got; $i++) {
    $exited = $app.HasExited
    if ($exited) { $app.WaitForExit() }
    if (Test-Path $reqOut) {
      if (Select-String -Path $reqOut -Pattern '^WISP_EVENT completed' -Quiet) { $got = $true; break }
      $failedLine = Select-String -Path $reqOut -Pattern '^WISP_EVENT failed' | Select-Object -First 1
      if ($failedLine) { Fail $failedLine.Line.Trim() }
    }
    if ($exited) { Fail "app exited (code $($app.ExitCode)) before WISP_EVENT" }
    Start-Sleep -Milliseconds 100
  }
  if (-not $got) { Fail "no 'WISP_EVENT completed' line within 30 s" }
  Start-Sleep -Seconds $settle
  if ($app.HasExited) { Fail 'app exited after the request' }
  $app.Refresh()
  $after = [int64]($app.WorkingSet64 / 1KB); $afterPriv = [int64]($app.PrivateMemorySize64 / 1KB)
  $afterPws = Get-PrivateWs $app.Id
  $app.Kill(); $app.WaitForExit(); $app = $null

  Write-Result 'measured' $null $idle $idlePriv $after $afterPriv $null $idlePws $afterPws
  Get-Content $Output
  if ($script:overBudget.Count -gt 0) {
    $script:overBudget | ForEach-Object { [Console]::Error.WriteLine($_) }
    $exitCode = 3
  }
}
catch {
  # Fail's own `exit 1` does not land here; this is for unexpected throws.
  $msg = $_.Exception.Message
  $tail = Get-StderrTail
  [Console]::Error.WriteLine("unmeasured: $msg")
  try { Stop-All } catch { }
  try { Write-Result 'unmeasured' $msg $null $null $null $null $tail } catch { [Console]::Error.WriteLine("could not write result: $($_.Exception.Message)") }
  exit 1
}
finally {
  Stop-All
  Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
exit $exitCode
