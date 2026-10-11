param(
    [switch] $RebuildApp,
    [switch] $Install,
    [string] $Version
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$work = Join-Path $root 'artifacts\local-installer'
$payload = Join-Path $work 'payload-x64'
$manifest = Join-Path $work 'payload-sha256.json'
$publish = Join-Path $work 'publish-x64'
$cache = Join-Path $env:LOCALAPPDATA 'MusicPlayerBuildTools'
$project = Join-Path $root 'src\MusicPlayer.App\MusicPlayer.App.csproj'
if (!$Version) { $Version = [regex]::Match([IO.File]::ReadAllText((Join-Path $root 'Directory.Build.props')), '<Version>([^<]+)</Version>').Groups[1].Value }
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw 'A stable MAJOR.MINOR.PATCH app version is required.' }

$compiler = Get-Command ISCC.exe -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty Source
if (!$compiler) {
    foreach ($directory in @((Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 7'), (Join-Path $env:ProgramFiles 'Inno Setup 7'))) {
        $candidate = Join-Path $directory 'ISCC.exe'
        if (Test-Path -LiteralPath $candidate) { $compiler = $candidate; break }
    }
}
if (!$compiler) { throw 'Install Inno Setup 7 first: https://jrsoftware.org/isdl.php' }
$signtool = Get-ChildItem -Path (Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin\*\x64\signtool.exe') | Sort-Object FullName -Descending | Select-Object -First 1 -ExpandProperty FullName
if (!$signtool) { throw 'Windows SDK signtool.exe is required for the signed local installer.' }
$public = [Security.Cryptography.X509Certificates.X509Certificate2]::new((Join-Path $PSScriptRoot 'MusicPlayer-Signing.cer'))
$certificate = Get-Item "Cert:\CurrentUser\My\$($public.Thumbprint)"
if (!$certificate.HasPrivateKey -or $certificate.NotAfter -le (Get-Date) -or $certificate.NotBefore -gt (Get-Date)) { throw 'The persistent project signing key is missing or outside its validity period.' }
New-Item -ItemType Directory -Path $work,$cache -Force | Out-Null

if ($RebuildApp -or !(Test-Path -LiteralPath $manifest)) {
    if (Get-Process MusicPlayer -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($work, [StringComparison]::OrdinalIgnoreCase) }) { throw 'Close the local test player before rebuilding its cached payload.' }
    $resolvedPublish = [IO.Path]::GetFullPath($publish)
    if (!$resolvedPublish.StartsWith([IO.Path]::GetFullPath($work) + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Publish output is outside the local build workspace.' }
    if (Test-Path -LiteralPath $resolvedPublish) { Remove-Item -LiteralPath $resolvedPublish -Recurse -Force }
    Write-Host 'Publishing the app once for Windows x64. Subsequent runs reuse this payload.'
    Push-Location $root
    try {
        dotnet restore $project --runtime win-x64 -p:Platform=x64
        if ($LASTEXITCODE -ne 0) {
            Write-Host 'Restore failed; retrying once. NuGet vulnerability auditing remains enabled.'
            dotnet restore $project --runtime win-x64 -p:Platform=x64
            if ($LASTEXITCODE -ne 0) { throw 'App dependency restore failed.' }
        }
        dotnet publish $project --configuration Release --runtime win-x64 --self-contained true --no-restore --output $publish -p:Platform=x64 -p:WindowsPackageType=None -p:WindowsAppSDKSelfContained=true "-p:Version=$Version"
        if ($LASTEXITCODE -ne 0) { throw 'App publish failed.' }
    } finally { Pop-Location }
    # Replace only the fixed generated payload under this checkout.
    foreach ($directory in @($payload)) {
        $resolved = [IO.Path]::GetFullPath($directory)
        if (!$resolved.StartsWith([IO.Path]::GetFullPath($work) + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Generated directory is outside the local build workspace.' }
        if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force }
    }
    & (Join-Path $PSScriptRoot 'Prepare-PortablePackage.ps1') -PublishDirectory $publish -DestinationDirectory $payload -Version $Version -Architecture x64
    foreach ($file in Get-ChildItem -LiteralPath $payload -File | Where-Object { $_.Name -like 'MusicPlayer*.exe' -or $_.Name -like 'MusicPlayer*.dll' -or $_.Extension -eq '.ps1' }) {
        if ($file.Extension -eq '.ps1') {
            $null = Set-AuthenticodeSignature -LiteralPath $file.FullName -Certificate $certificate -HashAlgorithm SHA256 -TimestampServer 'http://timestamp.digicert.com'
        } else {
            & $signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /sha1 $certificate.Thumbprint /s My $file.FullName
            if ($LASTEXITCODE -ne 0) { throw "Signing failed: $($file.Name)" }
        }
        & (Join-Path $PSScriptRoot 'Assert-LocalSignature.ps1') -Path $file.FullName
    }
    $entries = @(Get-ChildItem -LiteralPath $payload -File -Recurse | ForEach-Object {
        [pscustomobject]@{path=$_.FullName.Substring($payload.Length + 1);sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash}
    })
    [pscustomobject]@{version=$Version;sourceCommit=(git -C $root rev-parse HEAD);files=$entries} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $manifest -Encoding UTF8
    $resolvedPublish = [IO.Path]::GetFullPath($publish)
    if (!$resolvedPublish.StartsWith([IO.Path]::GetFullPath($work) + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Publish output is outside the local build workspace.' }
    Remove-Item -LiteralPath $resolvedPublish -Recurse -Force
}
$saved = Get-Content -LiteralPath $manifest -Raw | ConvertFrom-Json
if ($saved.sourceCommit -ne (git -C $root rev-parse HEAD)) { throw 'Cached payload belongs to another source revision. Run again with -RebuildApp.' }
if ($saved.version -ne $Version) { throw 'Cached app version differs. Run again with -RebuildApp.' }
foreach ($entry in $saved.files) {
    $file = Join-Path $payload $entry.path
    if (!(Test-Path -LiteralPath $file) -or (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $entry.sha256) { throw "Cached payload changed or is damaged: $($entry.path). Run with -RebuildApp." }
}
Write-Host 'Using the verified cached app payload; compiling only the installer.'
$bootstrapper = Join-Path $cache 'MicrosoftEdgeWebView2Setup.exe'
if (!(Test-Path -LiteralPath $bootstrapper)) { Invoke-WebRequest -Uri 'https://go.microsoft.com/fwlink/p/?LinkId=2124703' -OutFile $bootstrapper }
$vendor = Get-AuthenticodeSignature -LiteralPath $bootstrapper
if ($vendor.Status -ne 'Valid' -or $vendor.SignerCertificate.Subject -notmatch 'CN=Microsoft Corporation') { throw 'The WebView2 bootstrapper is damaged or lacks a valid Microsoft signature.' }
Copy-Item -LiteralPath $bootstrapper -Destination (Join-Path $payload 'MicrosoftEdgeWebView2Setup.exe') -Force
$signCommand = '$q' + $signtool + '$q sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /sha1 ' + $certificate.Thumbprint + ' /s My $f'
& $compiler "/DAppVersion=$Version" '/DArchitecture=x64' "/DSourceDir=$payload" "/DOutputDir=$work" '/DSignedBuild' "/Smusicplayer=$signCommand" (Join-Path $PSScriptRoot 'MusicPlayer.iss')
if ($LASTEXITCODE -ne 0) { throw 'Inno Setup compilation failed.' }
$setup = Join-Path $work 'MusicPlayer-Setup-x64.exe'
& (Join-Path $PSScriptRoot 'Assert-LocalSignature.ps1') -Path $setup
$hash = (Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash
"$hash  $([IO.Path]::GetFileName($setup))" | Set-Content -LiteralPath "$setup.sha256" -Encoding ASCII
Write-Host "Signed installer: $setup"
Write-Host ('Size: {0:N1} MB. SHA-256: {1}' -f ((Get-Item -LiteralPath $setup).Length / 1e6), $hash)
Write-Host 'The persistent project certificate is self-signed. Windows publisher trust has not been installed by this script.'

if ($Install) {
    if (Get-Process MusicPlayer -ErrorAction SilentlyContinue) { throw 'Installer is built. Close the running player before installing; it has not been stopped.' }
    $log = Join-Path $work 'setup-install.log'
    $process = Start-Process -FilePath $setup -ArgumentList '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART',('/LOG="'+$log+'"') -WindowStyle Hidden -Wait -PassThru
    if ($process.ExitCode -notin @(0,3010)) { throw "Installation failed with exit code $($process.ExitCode). See $log" }
    $registration = Get-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{8B3371D0-13A1-4410-9E60-A49AD1A5F08C}_is1'
    foreach ($entry in $saved.files | Where-Object { $_.path -notin @('Portable-README.txt','Uninstall-MusicPlayer.ps1','MicrosoftEdgeWebView2Setup.exe') }) {
        $installed = Join-Path $registration.InstallLocation $entry.path
        if (!(Test-Path -LiteralPath $installed) -or (Get-FileHash -LiteralPath $installed -Algorithm SHA256).Hash -ne $entry.sha256) { throw "Installed file is missing or differs from the cached payload: $($entry.path)" }
    }
    & (Join-Path $PSScriptRoot 'Assert-LocalSignature.ps1') -Path (Join-Path $registration.InstallLocation 'unins000.exe')
    Write-Host "Installation passed; every installed payload file matches its source. Log: $log"
}
