param(
    [Parameter(Mandatory = $true)]
    [string] $ManifestPath,

    [string] $SignaturePath
)

$ErrorActionPreference = 'Stop'

$manifestPath = (Resolve-Path -LiteralPath $ManifestPath).Path
if ([string]::IsNullOrWhiteSpace($SignaturePath)) { $SignaturePath = "$manifestPath.p7s" }
$signaturePath = (Resolve-Path -LiteralPath $SignaturePath).Path
$manifestName = [IO.Path]::GetFileName($manifestPath)
if ($manifestName -notmatch '^MusicPlayer-(?<version>\d+\.\d+\.\d+(?:-preview\.\d+)?)-SHA256SUMS\.txt$') {
    throw 'Manifest filename does not contain a supported Music Player version.'
}
$version = $Matches.version
$manifestDirectory = Split-Path -Parent $manifestPath

Add-Type -AssemblyName System.Security.Cryptography.Pkcs
$content = [System.Security.Cryptography.Pkcs.ContentInfo]::new([IO.File]::ReadAllBytes($manifestPath))
$signedManifest = [System.Security.Cryptography.Pkcs.SignedCms]::new($content, $true)
$signedManifest.Decode([IO.File]::ReadAllBytes($signaturePath))
if ($signedManifest.SignerInfos.Count -ne 1) { throw 'Release manifest must have exactly one signer.' }
$signedManifest.CheckSignature($true)

$signerCertificate = $signedManifest.SignerInfos[0].Certificate
if ($null -eq $signerCertificate) { throw 'The manifest signature does not include its signing certificate.' }
if ($signerCertificate.Subject -ne 'CN=Kalabhaftu') { throw 'Manifest signer does not match the Music Player release publisher.' }
$now = [DateTime]::UtcNow
if ($signerCertificate.NotBefore.ToUniversalTime() -gt $now -or $signerCertificate.NotAfter.ToUniversalTime() -le $now) {
    throw 'Manifest signing certificate is outside its validity period.'
}
$hasCodeSigningEku = $signerCertificate.Extensions | Where-Object {
    $_ -is [System.Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension] -and
    ($_.EnhancedKeyUsages | Where-Object { $_.Value -eq '1.3.6.1.5.5.7.3.3' })
} | Select-Object -First 1
if (-not $hasCodeSigningEku) { throw 'Manifest signer lacks the Code Signing purpose.' }

# Authenticate the persistent project key independently of Windows public trust.
# Obtain this verifier/fingerprint from the project's GitHub repository over HTTPS.
$expectedFingerprint = '91BB3E5022E52803329F928B8CCAD73E3211AE710B764B373480A2FB1C5EDEE4'
$sha256 = [System.Security.Cryptography.SHA256]::Create()
try { $actualFingerprint = ([BitConverter]::ToString($sha256.ComputeHash($signerCertificate.RawData))).Replace('-', '') }
finally { $sha256.Dispose() }
if ($actualFingerprint -ne $expectedFingerprint) { throw 'Manifest signer does not match the pinned Music Player self-signed certificate.' }

$entries = [System.Collections.Generic.List[object]]::new()
$seenNames = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
foreach ($line in [IO.File]::ReadAllLines($manifestPath, [Text.Encoding]::UTF8)) {
    if ($line -notmatch '^([0-9a-fA-F]{64})  ([^/\\]+)$') { throw 'Manifest contains an invalid SHA-256 entry.' }
    $hash = $Matches[1].ToLowerInvariant()
    $name = $Matches[2]
    if ($name -in @('.', '..') -or [IO.Path]::GetFileName($name) -ne $name) { throw 'Manifest contains an unsafe asset path.' }
    if (-not $seenNames.Add($name)) { throw "Manifest contains a duplicate entry: $name" }
    $entries.Add([pscustomobject]@{ Hash = $hash; Name = $name })
}

$requiredNames = @(
    "MusicPlayer-$version-x64-portable.zip",
    "MusicPlayer-$version-arm64-portable.zip",
    'MusicPlayer-Setup-x64.exe',
    'MusicPlayer-Setup-arm64.exe',
    "MusicPlayer-$version-x64-arm64.msixbundle",
    'ThirdPartyNotices.md',
    'MusicPlayer-Signing.cer',
    'SIGNING.md',
    'Verify-MusicPlayer-ReleaseManifest.ps1',
    'Uninstall-MusicPlayer-MSIX.ps1'
)
foreach ($requiredName in $requiredNames) {
    if (-not $seenNames.Contains($requiredName)) { throw "Manifest is missing the required release asset: $requiredName" }
}
if ($entries.Count -ne $requiredNames.Count) { throw 'Manifest contains an unexpected release asset entry.' }

foreach ($entry in $entries) {
    $assetPath = Join-Path $manifestDirectory $entry.Name
    if (-not (Test-Path -LiteralPath $assetPath -PathType Leaf)) { throw "Release asset is missing: $($entry.Name)" }
    $actualHash = (Get-FileHash -LiteralPath $assetPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $entry.Hash) { throw "SHA-256 mismatch: $($entry.Name)" }
}

Write-Host "Project self-signed signature and SHA-256 hashes verified for $($entries.Count) release files. This does not establish publicly trusted Windows publisher identity."
