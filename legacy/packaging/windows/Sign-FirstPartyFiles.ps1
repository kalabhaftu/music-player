param([Parameter(Mandatory = $true)][string] $Directory)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'Release signing helper runs only on the isolated packaging runner.' }
$public = [System.Security.Cryptography.X509Certificates.X509Certificate2]::new((Join-Path $PSScriptRoot 'MusicPlayer-Signing.cer'))
$certificate = Get-Item "Cert:/CurrentUser/My/$($public.Thumbprint)"
if (-not $certificate.HasPrivateKey) { throw 'The persistent release private key is not available.' }
$sdkRoot = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/bin'
$signtool = Get-ChildItem (Join-Path $sdkRoot '*/x64/signtool.exe') | Sort-Object FullName -Descending | Select-Object -First 1 -ExpandProperty FullName
if (-not $signtool) { throw 'Windows SDK signing tool is missing.' }
$root = (Resolve-Path -LiteralPath $Directory).Path
$files = @(Get-ChildItem -LiteralPath $root -File | Where-Object { $_.Name -like 'MusicPlayer*.exe' -or $_.Name -like 'MusicPlayer*.dll' })
foreach ($file in $files) {
    & $signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /sha1 $certificate.Thumbprint /s My $file.FullName
    if ($LASTEXITCODE -ne 0) { throw "Signing failed: $($file.Name)" }
    & $signtool verify /pa /all $file.FullName
    if ($LASTEXITCODE -ne 0) { throw "Signature verification failed: $($file.Name)" }
    $signature = Get-AuthenticodeSignature -LiteralPath $file.FullName
    if ($signature.SignerCertificate.Thumbprint -ne $public.Thumbprint -or -not $signature.TimeStamperCertificate) { throw "Wrong signer or absent timestamp: $($file.Name)" }
}
foreach ($script in Get-ChildItem -LiteralPath $root -File -Filter '*.ps1') {
    $signature = Set-AuthenticodeSignature -FilePath $script.FullName -Certificate $certificate -HashAlgorithm SHA256 -TimestampServer 'http://timestamp.digicert.com'
    if ($signature.Status -ne 'Valid' -or -not $signature.TimeStamperCertificate) { throw "Helper signing or timestamp failed: $($script.Name)" }
}
