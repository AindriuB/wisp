# pack.ps1 -Exe <wisp.exe> -OutDir <dir>
# Builds Wisp-<version>-windows-x64.msix from the Windows exe, the notice files and
# the MSIX logos. Run from the repository root on a Windows host with the Windows SDK.
# Store identity comes from MSIX_IDENTITY_NAME, MSIX_PUBLISHER and
# MSIX_PUBLISHER_DISPLAY_NAME; an unset one uses a marked placeholder
# (see STORE-SUBMISSION.md). The package is unsigned.
param(
  [Parameter(Mandatory)][string]$Exe,
  [Parameter(Mandatory)][string]$OutDir
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$root = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
if (-not (Test-Path -LiteralPath $Exe -PathType Leaf)) { throw "no such exe: $Exe" }
$Exe = (Resolve-Path -LiteralPath $Exe).Path
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$OutDir = (Resolve-Path -LiteralPath $OutDir).Path

Push-Location $root
try {
  $meta = cargo metadata --format-version 1 --no-deps --locked | ConvertFrom-Json
  if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed' }
} finally { Pop-Location }
$version = ($meta.packages | Where-Object name -eq 'wisp-app').version
if (-not $version) { throw 'cannot read wisp-app version' }
if ($version -notmatch '^\d+\.\d+\.\d+$') { throw "version $version is not x.y.z" }
$msixVersion = "$version.0"   # MSIX versions have four parts; the Store reserves the last as 0

$fields = @(
  @{ Var = 'MSIX_IDENTITY_NAME';           Token = '@IDENTITY_NAME@';          Default = 'Wisp.Placeholder';              Label = 'Identity Name' },
  @{ Var = 'MSIX_PUBLISHER';               Token = '@PUBLISHER@';              Default = 'CN=WispPlaceholder';            Label = 'Publisher' },
  @{ Var = 'MSIX_PUBLISHER_DISPLAY_NAME';  Token = '@PUBLISHER_DISPLAY_NAME@'; Default = 'Wisp placeholder publisher';    Label = 'PublisherDisplayName' }
)

$manifest = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'AppxManifest.xml.template') -Raw
$manifest = $manifest.Replace('@VERSION@', $msixVersion)
foreach ($f in $fields) {
  $v = [Environment]::GetEnvironmentVariable($f.Var)
  if ([string]::IsNullOrWhiteSpace($v)) {
    Write-Host "::notice::MSIX uses placeholder $($f.Label); not Store-uploadable"
    $v = $f.Default
  }
  $manifest = $manifest.Replace($f.Token, [System.Security.SecurityElement]::Escape($v))
}
if ($manifest -match '@[A-Z_]+@') { throw "unfilled token in manifest: $($Matches[0])" }

$stage = Join-Path ([IO.Path]::GetTempPath()) ("wisp-msix-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path "$stage/Assets" | Out-Null
try {
  Copy-Item -LiteralPath $Exe -Destination "$stage/wisp.exe"
  foreach ($n in 'LICENSE', 'NOTICE', 'THIRD-PARTY-NOTICES') { Copy-Item (Join-Path $root $n) "$stage/$n" }
  Copy-Item (Join-Path $root 'assets/icon/msix/*.png') "$stage/Assets/"
  [IO.File]::WriteAllText("$stage/AppxManifest.xml", $manifest, (New-Object Text.UTF8Encoding $false))

  $sdk = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/bin'
  $makeappx = Get-ChildItem -Path $sdk -Recurse -Filter makeappx.exe -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match '[\\/]x64[\\/]' } |
    Sort-Object { try { [version]$_.FullName.Split([char[]]'\/')[-3] } catch { [version]'0.0' } } -Descending |
    Select-Object -First 1
  if (-not $makeappx) { throw "makeappx.exe not found under $sdk" }

  $msix = Join-Path $OutDir "Wisp-$version-windows-x64.msix"
  if (Test-Path -LiteralPath $msix) { Remove-Item -LiteralPath $msix }
  $log = & $makeappx.FullName pack /d $stage /p $msix /o 2>&1
  $code = $LASTEXITCODE
  $log | ForEach-Object { Write-Host $_ }
  if ($code -ne 0 -or ($log -match 'error')) { throw "makeappx reported errors (exit $code)" }
  if (-not (Test-Path -LiteralPath $msix)) { throw "makeappx produced no package" }
  Write-Host "packed $msix"
} finally {
  Remove-Item -Recurse -Force -LiteralPath $stage -ErrorAction SilentlyContinue
}
