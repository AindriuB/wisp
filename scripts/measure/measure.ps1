# measure.ps1 -Binary <wisp.exe> -Output <out.json>   (Windows)
# Env: TARGET (default x86_64-pc-windows-msvc), RENDERER (hardware|software|unknown,
#      default unknown), SETTLE_SECONDS (default 5).
#
# Method: serve a fixed 64 KiB file from `python3 -m http.server` on 127.0.0.1.
# Launch the binary once without the hook, wait settle_seconds, sample
# WorkingSet64 (idle_rss_kib) and PrivateMemorySize64 (private_kib), kill.
# Launch it again with WISP_AUTOSEND_URL, wait for the `WISP_EVENT completed`
# stdout line (30 s max), wait settle_seconds, sample again, kill. Fixed waits
# are fine: this is a measurement, not a test.
# If the window fails to start or no WISP_EVENT line arrives, write status
# "unmeasured" with a reason, exit non-zero, and record no numbers.
param(
  [Parameter(Mandatory = $true)][string]$Binary,
  [Parameter(Mandatory = $true)][string]$Output
)
$ErrorActionPreference = 'Stop'

$settle = if ($env:SETTLE_SECONDS) { [int]$env:SETTLE_SECONDS } else { 5 }
$renderer = if ($env:RENDERER) { $env:RENDERER } else { 'unknown' }
$target = if ($env:TARGET) { $env:TARGET } else { 'x86_64-pc-windows-msvc' }
$commit = (git rev-parse HEAD 2>$null); if (-not $commit) { $commit = 'unknown' }
$osVersion = [System.Environment]::OSVersion.VersionString
$bytes = (Get-Item $Binary).Length

function Write-Result($status, $reason, $idle, $idlePriv, $after, $afterPriv) {
  $dir = Split-Path -Parent $Output
  if ($dir) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
  [ordered]@{
    target = $target; commit = $commit; binary_bytes = $bytes
    idle_rss_kib = $idle; after_request_rss_kib = $after
    private_kib = [ordered]@{ idle = $idlePriv; after_request = $afterPriv }
    settle_seconds = $settle; renderer = $renderer; status = $status
    reason = $reason; os_version = $osVersion
  } | ConvertTo-Json | Set-Content -Encoding utf8 $Output
}

$work = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $work | Out-Null
$server = $null; $app = $null
function Stop-All {
  if ($app -and -not $app.HasExited) { $app.Kill() }
  if ($server -and -not $server.HasExited) { $server.Kill() }
}
function Fail($reason) {
  [Console]::Error.WriteLine("unmeasured: $reason")
  Write-Result 'unmeasured' $reason $null $null $null $null
  Stop-All
  exit 1
}

try {
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
  $app = Start-Process $Binary -PassThru -RedirectStandardOutput (Join-Path $work 'idle.out') -RedirectStandardError (Join-Path $work 'idle.err')
  Start-Sleep -Seconds $settle
  if ($app.HasExited) { Fail 'window failed to start (process exited during idle run)' }
  $app.Refresh()
  $idle = [int64]($app.WorkingSet64 / 1KB); $idlePriv = [int64]($app.PrivateMemorySize64 / 1KB)
  $app.Kill(); $app.WaitForExit(); $app = $null

  # Run 2: after one request.
  $env:WISP_AUTOSEND_URL = $url
  $reqOut = Join-Path $work 'req.out'
  $app = Start-Process $Binary -PassThru -RedirectStandardOutput $reqOut -RedirectStandardError (Join-Path $work 'req.err')
  Remove-Item Env:WISP_AUTOSEND_URL
  $got = $false
  for ($i = 0; $i -lt 300 -and -not $got; $i++) {
    if ((Test-Path $reqOut) -and (Select-String -Path $reqOut -Pattern '^WISP_EVENT completed' -Quiet)) { $got = $true; break }
    if ($app.HasExited) { break }
    Start-Sleep -Milliseconds 100
  }
  if (-not $got) { Fail "no 'WISP_EVENT completed' line within 30 s" }
  Start-Sleep -Seconds $settle
  if ($app.HasExited) { Fail 'app exited after the request' }
  $app.Refresh()
  $after = [int64]($app.WorkingSet64 / 1KB); $afterPriv = [int64]($app.PrivateMemorySize64 / 1KB)
  $app.Kill(); $app.WaitForExit(); $app = $null

  Write-Result 'measured' $null $idle $idlePriv $after $afterPriv
  Get-Content $Output
}
finally {
  Stop-All
  Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
