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

# SignPath timestamps with RFC 3161. TimeStamperCertificate is also set by a
# legacy Authenticode countersignature, so read the PKCS#7 blob from the PE
# certificate table and require the RFC 3161 timestamp token attribute
# (1.3.6.1.4.1.311.3.3.1) as an unsigned attribute on the signer.
Add-Type -AssemblyName System.Security
$rfc3161Oid = '1.3.6.1.4.1.311.3.3.1'
$hasRfc3161 = $false
try {
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    $peOff = [BitConverter]::ToInt32($bytes, 0x3C)
    $opt = $peOff + 24
    $magic = [BitConverter]::ToUInt16($bytes, $opt)
    $dirs = if ($magic -eq 0x20b) { $opt + 112 } else { $opt + 96 }
    $certOff = [BitConverter]::ToUInt32($bytes, $dirs + 4 * 8)
    $certSize = [BitConverter]::ToUInt32($bytes, $dirs + 4 * 8 + 4)
    if ($certOff -eq 0 -or $certSize -lt 8) { throw 'no certificate table' }
    # WIN_CERTIFICATE: dwLength(4) wRevision(2) wCertificateType(2) bCertificate
    $len = [BitConverter]::ToUInt32($bytes, [int]$certOff)
    $blob = New-Object byte[] ($len - 8)
    [Array]::Copy($bytes, [int]$certOff + 8, $blob, 0, $len - 8)
    $cms = New-Object System.Security.Cryptography.Pkcs.SignedCms
    $cms.Decode($blob)
    foreach ($si in $cms.SignerInfos) {
        foreach ($attr in $si.UnsignedAttributes) {
            if ($attr.Oid.Value -eq $rfc3161Oid) { $hasRfc3161 = $true }
        }
    }
} catch {
    Write-Host '::error::could not read the signature timestamp'
    exit 1
}
if (-not $hasRfc3161) {
    Write-Host '::error::signature has no RFC 3161 timestamp'
    exit 1
}

Write-Host "Signer subject: $($sig.SignerCertificate.Subject)"
