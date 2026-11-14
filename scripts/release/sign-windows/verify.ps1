# Verify an Authenticode-signed exe: valid signature, signtool /pa, RFC 3161
# timestamp. Prints the signer subject only. Exits non-zero on any failure.
param([Parameter(Mandatory = $true)][string]$Path)
$ErrorActionPreference = 'Stop'

$sig = Get-AuthenticodeSignature -FilePath $Path
if ($sig.Status -ne 'Valid') {
    Write-Host "::error::Authenticode status is $($sig.Status), expected Valid"
    exit 1
}

$signtool = Get-ChildItem -Path "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Filter signtool.exe -Recurse -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match '\\x64\\' } |
    Sort-Object FullName -Descending | Select-Object -First 1
if (-not $signtool) {
    Write-Host '::error::signtool.exe not found'
    exit 1
}
& $signtool.FullName verify /pa $Path | Out-Null
if ($LASTEXITCODE -ne 0) {
    Write-Host '::error::signtool verify /pa failed'
    exit 1
}

# SignPath timestamps with RFC 3161; require a timestamp countersignature.
if (-not $sig.TimeStamperCertificate) {
    Write-Host '::error::signature has no timestamp'
    exit 1
}
$stamped = & $signtool.FullName verify /pa /v $Path 2>&1 | Select-String -Pattern 'timestamp' -Quiet
if (-not $stamped) {
    Write-Host '::error::signtool reports no timestamp'
    exit 1
}

Write-Host "Signer subject: $($sig.SignerCertificate.Subject)"
