param(
    [Parameter(Mandatory = $true)]
    [string] $ArtifactDirectory,

    [Parameter(Mandatory = $true)]
    [string] $Version,

    [Parameter(Mandatory = $true)]
    [string] $PfxPath,

    [Parameter(Mandatory = $true)]
    [string] $Password
)

$ErrorActionPreference = 'Stop'

if ($Version -notmatch '^\d+\.\d+\.\d+(-preview\.\d+)?$') {
    throw 'Version must use MAJOR.MINOR.PATCH or MAJOR.MINOR.PATCH-preview.N.'
}

$artifactRoot = (Resolve-Path -LiteralPath $ArtifactDirectory).Path
$manifestName = "MusicPlayer-$Version-SHA256SUMS.txt"
$manifestPath = Join-Path $artifactRoot $manifestName
$signaturePath = "$manifestPath.p7s"
$verifierName = 'Verify-MusicPlayer-ReleaseManifest.ps1'
$verifierSource = Join-Path $PSScriptRoot $verifierName
$verifierDestination = Join-Path $artifactRoot $verifierName

if (-not (Test-Path -LiteralPath $verifierSource -PathType Leaf)) {
    throw "Manifest verifier was not found: $verifierSource"
}
Copy-Item -LiteralPath $verifierSource -Destination $verifierDestination -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'Uninstall-MusicPlayer-MSIX.ps1') -Destination $artifactRoot
# Sign the shipped verifier before computing its manifest hash.
$publicCertificate = [System.Security.Cryptography.X509Certificates.X509Certificate2]::new((Join-Path $PSScriptRoot 'MusicPlayer-Signing.cer'))
$signingCertificate = Get-Item "Cert:/CurrentUser/My/$($publicCertificate.Thumbprint)"
$signature = Set-AuthenticodeSignature -FilePath $verifierDestination -Certificate $signingCertificate -HashAlgorithm SHA256 -TimestampServer 'http://timestamp.digicert.com'
if ($signature.Status -ne 'Valid' -or -not $signature.TimeStamperCertificate) { throw 'Release verifier signing or timestamp verification failed.' }
$signature = Set-AuthenticodeSignature -FilePath (Join-Path $artifactRoot 'Uninstall-MusicPlayer-MSIX.ps1') -Certificate $signingCertificate -HashAlgorithm SHA256 -TimestampServer 'http://timestamp.digicert.com'
if ($signature.Status -ne 'Valid' -or -not $signature.TimeStamperCertificate) { throw 'MSIX uninstall helper signing or timestamp verification failed.' }
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'MusicPlayer-Signing.cer') -Destination $artifactRoot
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'SIGNING.md') -Destination $artifactRoot

$assetNames = @(
    "MusicPlayer-$Version-x64-portable.zip",
    "MusicPlayer-$Version-arm64-portable.zip",
    'MusicPlayer-Setup-x64.exe',
    'MusicPlayer-Setup-arm64.exe',
    "MusicPlayer-$Version-x64-arm64.msixbundle",
    'ThirdPartyNotices.md',
    'MusicPlayer-Signing.cer',
    'SIGNING.md',
    'Uninstall-MusicPlayer-MSIX.ps1',
    $verifierName
)

foreach ($assetName in $assetNames) {
    $assetPath = Join-Path $artifactRoot $assetName
    if (-not (Test-Path -LiteralPath $assetPath -PathType Leaf)) {
        throw "Required release asset is missing: $assetName"
    }
}

$sortedAssetNames = $assetNames | Sort-Object -CaseSensitive
$manifestLines = foreach ($assetName in $sortedAssetNames) {
    $assetPath = Join-Path $artifactRoot $assetName
    $hash = (Get-FileHash -LiteralPath $assetPath -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $assetName"
}
[IO.File]::WriteAllText(
    $manifestPath,
    ($manifestLines -join "`n") + "`n",
    [Text.UTF8Encoding]::new($false)
)

Add-Type -AssemblyName System.Security.Cryptography.Pkcs
$certificate = $null
try {
    $certificate = [System.Security.Cryptography.X509Certificates.X509Certificate2]::new(
        (Resolve-Path -LiteralPath $PfxPath).Path,
        $Password,
        [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::EphemeralKeySet
    )
    if (-not $certificate.HasPrivateKey) { throw 'Signing certificate has no private key.' }
    if ($certificate.Subject -ne 'CN=Kalabhaftu') { throw 'Signing certificate subject does not match CN=Kalabhaftu.' }

    $content = [System.Security.Cryptography.Pkcs.ContentInfo]::new([IO.File]::ReadAllBytes($manifestPath))
    $signedManifest = [System.Security.Cryptography.Pkcs.SignedCms]::new($content, $true)
    $signer = [System.Security.Cryptography.Pkcs.CmsSigner]::new(
        [System.Security.Cryptography.Pkcs.SubjectIdentifierType]::IssuerAndSerialNumber,
        $certificate
    )
    $signer.IncludeOption = [System.Security.Cryptography.X509Certificates.X509IncludeOption]::EndCertOnly
    $signer.DigestAlgorithm = [System.Security.Cryptography.Oid]::new('2.16.840.1.101.3.4.2.1')
    $signedManifest.ComputeSignature($signer)
    [IO.File]::WriteAllBytes($signaturePath, $signedManifest.Encode())

    $verifiedCms = [System.Security.Cryptography.Pkcs.SignedCms]::new($content, $true)
    $verifiedCms.Decode([IO.File]::ReadAllBytes($signaturePath))
    if ($verifiedCms.SignerInfos.Count -ne 1) { throw 'Release manifest must have exactly one signer.' }
    $verifiedSigner = $verifiedCms.SignerInfos[0].Certificate
    if ($null -eq $verifiedSigner -or $verifiedSigner.Thumbprint -ne $certificate.Thumbprint) {
        throw 'Release manifest was not signed by the configured release certificate.'
    }
    $verifiedCms.CheckSignature($true)
    & $verifierDestination -ManifestPath $manifestPath -SignaturePath $signaturePath

    $releaseDirectory = Join-Path (Split-Path -Parent $artifactRoot) 'release-assets'
    if (Test-Path -LiteralPath $releaseDirectory) { throw "Release staging directory already exists: $releaseDirectory" }
    [void][IO.Directory]::CreateDirectory($releaseDirectory)
    $publishedNames = @($assetNames) + $manifestName + [IO.Path]::GetFileName($signaturePath)
    foreach ($publishedName in $publishedNames) {
        Copy-Item -LiteralPath (Join-Path $artifactRoot $publishedName) -Destination (Join-Path $releaseDirectory $publishedName)
    }
    $releaseNotesPath = Join-Path $artifactRoot 'release-notes.md'
    if (-not (Test-Path -LiteralPath $releaseNotesPath -PathType Leaf)) { throw 'Release notes are missing from the release staging directory.' }
    Copy-Item -LiteralPath $releaseNotesPath -Destination (Join-Path $releaseDirectory 'release-notes.md')
    $stagedManifestPath = Join-Path $releaseDirectory $manifestName
    & $verifierSource -ManifestPath $stagedManifestPath -SignaturePath "$stagedManifestPath.p7s"
} finally {
    if ($certificate) { $certificate.Dispose() }
}

Write-Host "Signed and verified $manifestName and $($assetNames.Count) listed files."
