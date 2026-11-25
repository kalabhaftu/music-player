param(
    [switch] $RebuildApp,
    [switch] $Install,
    [string] $Version,
    [switch] $SkipSigning
)
$ErrorActionPreference = 'Stop'

# Determine workspace roots
$scriptDir = $PSScriptRoot
$current = $scriptDir
$repoRoot = $null
$rustRoot = $null
while ($current) {
    if (Test-Path (Join-Path $current 'music-player-rust') -PathType Container) {
        $repoRoot = $current
        $rustRoot = Join-Path $current 'music-player-rust'
        break
    }
    if (Test-Path (Join-Path $current 'crates\app') -PathType Container) {
        $rustRoot = $current
        $repoRoot = Split-Path -Parent $current
        break
    }
    $parent = Split-Path -Parent $current
    if ($parent -eq $current) { break }
    $current = $parent
}
if (-not $repoRoot) {
    $repoRoot = [IO.Path]::GetFullPath((Join-Path $scriptDir '..\..\..'))
    if (-not (Test-Path (Join-Path $repoRoot 'LICENSE'))) {
        $repoRoot = [IO.Path]::GetFullPath((Join-Path $scriptDir '..\..'))
    }
    $rustRoot = Join-Path $repoRoot 'music-player-rust'
}

$work = Join-Path $repoRoot 'artifacts\local-installer'
$payload = Join-Path $work 'payload-x64'
$manifest = Join-Path $work 'payload-sha256.json'
$releaseDir = Join-Path $rustRoot 'target\release'

if (-not $Version) {
    $propsPath = Join-Path $repoRoot 'Directory.Build.props'
    if (Test-Path -LiteralPath $propsPath) {
        $Version = [regex]::Match([IO.File]::ReadAllText($propsPath), '<Version>([^<]+)</Version>').Groups[1].Value
    }
    if (-not $Version) {
        $cargoPath = Join-Path $rustRoot 'crates\app\Cargo.toml'
        if (Test-Path -LiteralPath $cargoPath) {
            $cargoMatch = [regex]::Match([IO.File]::ReadAllText($cargoPath), '(?m)^version\s*=\s*"([^"]+)"')
            if ($cargoMatch.Success) { $Version = $cargoMatch.Groups[1].Value }
        }
    }
    if (-not $Version) { $Version = '1.0.4' }
}
if ($Version -notmatch '^\d+\.\d+\.\d+') { throw "A valid MAJOR.MINOR.PATCH app version is required. Received: $Version" }

Write-Host "Target Version: $Version"

# Find Inno Setup compiler
$compiler = Get-Command ISCC.exe -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty Source
if (-not $compiler) {
    $searchDirs = @(
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 7'),
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6'),
        (Join-Path $env:ProgramFiles 'Inno Setup 7'),
        (Join-Path $env:ProgramFiles 'Inno Setup 6'),
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 7'),
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6')
    )
    foreach ($dir in $searchDirs) {
        $candidate = Join-Path $dir 'ISCC.exe'
        if (Test-Path -LiteralPath $candidate) { $compiler = $candidate; break }
    }
}
if (-not $compiler) { throw 'Inno Setup 6 or 7 is required. Install from: https://jrsoftware.org/isdl.php' }
Write-Host "Inno Setup Compiler: $compiler"

# Find signtool and check code-signing certificate
$signtool = $null
$certificate = $null
$canSign = $false
if (-not $SkipSigning) {
    $sdkPattern = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin\*\x64\signtool.exe'
    $signtool = Get-ChildItem -Path $sdkPattern -ErrorAction SilentlyContinue | Sort-Object FullName -Descending | Select-Object -First 1 -ExpandProperty FullName
    
    $cerPath = Join-Path $scriptDir 'MusicPlayer-Signing.cer'
    if (Test-Path -LiteralPath $cerPath) {
        $public = [Security.Cryptography.X509Certificates.X509Certificate2]::new($cerPath)
        $cert = Get-Item "Cert:\CurrentUser\My\$($public.Thumbprint)" -ErrorAction SilentlyContinue
        if ($cert -and $cert.HasPrivateKey -and $cert.NotAfter -gt (Get-Date) -and $cert.NotBefore -le (Get-Date)) {
            $certificate = $cert
            $canSign = ($signtool -ne $null)
        }
    }
}
if ($canSign) {
    Write-Host "Code signing enabled with certificate $($certificate.Thumbprint) via $signtool"
} else {
    Write-Host "Code signing not available or skipped; building unsigned installer."
}

# Stop any running instances before packaging/staging
Get-Process -Name 'MusicPlayer', 'music-player-app' -ErrorAction SilentlyContinue | ForEach-Object {
    try {
        $null = $_.CloseMainWindow()
        if (-not $_.WaitForExit(3000)) { $_.Kill() }
    } catch { }
}

New-Item -ItemType Directory -Path $work -Force | Out-Null

$targetExe = Join-Path $releaseDir 'MusicPlayer.exe'
$appExe = Join-Path $releaseDir 'music-player-app.exe'

# Build Rust app if requested or if missing
if ($RebuildApp -or (-not (Test-Path $targetExe) -and -not (Test-Path $appExe))) {
    Write-Host "Building Rust release binary (cargo build --release -p music-player-app)..."
    Push-Location $rustRoot
    try {
        cargo build --release -p music-player-app
        if ($LASTEXITCODE -ne 0) { throw "Cargo release build failed." }
    } finally {
        Pop-Location
    }
}

# Ensure MusicPlayer.exe in release dir matches the latest music-player-app.exe build
if (Test-Path $appExe) {
    Copy-Item -LiteralPath $appExe -Destination $targetExe -Force
} elseif (-not (Test-Path $targetExe)) {
    throw "Neither MusicPlayer.exe nor music-player-app.exe found in $releaseDir"
}

# Prepare portable payload
Write-Host "Staging portable payload in $payload..."
& (Join-Path $scriptDir 'Prepare-PortablePackage.ps1') `
    -PublishDirectory $releaseDir `
    -DestinationDirectory $payload `
    -Version $Version `
    -Architecture 'x64'

# Sign first-party binaries if signing is available
$tsaServers = @('http://ts.ssl.com', 'http://timestamp.sectigo.com', 'http://timestamp.digicert.com')
$activeTsa = $tsaServers[0]
if ($canSign) {
    Write-Host "Signing first-party executables and helper scripts..."
    $filesToSign = @(Get-ChildItem -LiteralPath $payload -File | Where-Object {
        $_.Name -like 'MusicPlayer*.exe' -or $_.Name -like 'music-player*.exe' -or $_.Name -like 'MusicPlayer*.dll' -or $_.Extension -eq '.ps1'
    })
    foreach ($file in $filesToSign) {
        $signed = $false
        foreach ($tsa in $tsaServers) {
            if ($file.Extension -eq '.ps1') {
                $sigResult = Set-AuthenticodeSignature -LiteralPath $file.FullName -Certificate $certificate -HashAlgorithm SHA256 -TimestampServer $tsa -ErrorAction SilentlyContinue
                if ($sigResult -and $sigResult.SignerCertificate -and $sigResult.TimeStamperCertificate) {
                    $activeTsa = $tsa
                    $signed = $true
                    break
                }
            } else {
                & $signtool sign /fd SHA256 /tr $tsa /td SHA256 /sha1 $certificate.Thumbprint /s My $file.FullName
                if ($LASTEXITCODE -eq 0) {
                    $activeTsa = $tsa
                    $signed = $true
                    break
                }
            }
        }
        if (-not $signed) { throw "Signing failed: $($file.Name)" }
        & (Join-Path $scriptDir 'Assert-LocalSignature.ps1') -Path $file.FullName
    }
}

# Generate payload manifest
$entries = @(Get-ChildItem -LiteralPath $payload -File -Recurse | ForEach-Object {
    [pscustomobject]@{
        path = $_.FullName.Substring($payload.Length + 1)
        sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash
    }
})
[pscustomobject]@{
    version = $Version
    sourceCommit = (git -C $repoRoot rev-parse HEAD 2>$null)
    files = $entries
} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $manifest -Encoding UTF8

# Compile installer with Inno Setup
Write-Host "Compiling Inno Setup installer..."
$issFile = Join-Path $scriptDir 'MusicPlayer.iss'
$innoArgs = @(
    "/DAppVersion=$Version",
    '/DArchitecture=x64',
    "/DSourceDir=$payload",
    "/DOutputDir=$work"
)
if ($canSign) {
    $signCommand = '$q' + $signtool + '$q sign /fd SHA256 /tr ' + $activeTsa + ' /td SHA256 /sha1 ' + $certificate.Thumbprint + ' /s My $f'
    $innoArgs += '/DSignedBuild'
    $innoArgs += "/Smusicplayer=$signCommand"
}
$innoArgs += $issFile

& $compiler @innoArgs
if ($LASTEXITCODE -ne 0) { throw 'Inno Setup compilation failed.' }

$setup = Join-Path $work 'MusicPlayer-Setup-x64.exe'
if (-not (Test-Path -LiteralPath $setup)) {
    throw "Expected setup binary not found: $setup"
}

if ($canSign) {
    & (Join-Path $scriptDir 'Assert-LocalSignature.ps1') -Path $setup
}

$hash = (Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash
"$hash  $([IO.Path]::GetFileName($setup))" | Set-Content -LiteralPath "$setup.sha256" -Encoding ASCII

Write-Host "=========================================================="
Write-Host "Installer created successfully: $setup"
Write-Host ('Size: {0:N1} MB. SHA-256: {1}' -f ((Get-Item -LiteralPath $setup).Length / 1e6), $hash)
Write-Host "=========================================================="

if ($Install) {
    Write-Host "Testing installer execution..."
    $runningCopies = Get-Process -Name 'MusicPlayer', 'music-player-app' -ErrorAction SilentlyContinue
    if ($runningCopies) { throw 'Close running Music Player instances before test-installing.' }
    
    $log = Join-Path $work 'setup-install.log'
    $process = Start-Process -FilePath $setup -ArgumentList '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART',('/LOG="'+$log+'"') -WindowStyle Hidden -Wait -PassThru
    if ($process.ExitCode -notin @(0, 3010)) { throw "Installation failed with exit code $($process.ExitCode). Log: $log" }
    Write-Host "Test installation passed cleanly. Log: $log"
}
