param(
    [Parameter(Mandatory = $true)]
    [string] $PublishDirectory,

    [Parameter(Mandatory = $true)]
    [string] $DestinationDirectory,

    [Parameter(Mandatory = $true)]
    [string] $Version,

    [Parameter(Mandatory = $true)]
    [ValidateSet('x64', 'arm64')]
    [string] $Architecture
)

$ErrorActionPreference = 'Stop'

$source = (Resolve-Path -LiteralPath $PublishDirectory).Path
$destination = [IO.Path]::GetFullPath($DestinationDirectory)

# Locate executables
$targetExe = Join-Path $source 'MusicPlayer.exe'
$appExe = Join-Path $source 'music-player-app.exe'
if (-not (Test-Path -LiteralPath $targetExe -PathType Leaf)) {
    if (Test-Path -LiteralPath $appExe -PathType Leaf) {
        Copy-Item -LiteralPath $appExe -Destination $targetExe -Force
    } else {
        throw "Published app executable was not found: $targetExe or $appExe"
    }
}

# Locate project roots to find LICENSE, ThirdPartyNotices, Assets
$current = $source
$repoRoot = $null
$rustRoot = $null
while ($current) {
    if (Test-Path (Join-Path $current 'crates\app') -PathType Container) {
        $rustRoot = $current
        $repoRoot = $current
        break
    }
    if (Test-Path (Join-Path $current 'music-player-rust') -PathType Container) {
        $repoRoot = $current
        $rustRoot = Join-Path $current 'music-player-rust'
        break
    }
    $parent = Split-Path -Parent $current
    if ($parent -eq $current) { break }
    $current = $parent
}
if (-not $repoRoot) {
    $repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
    $rustRoot = $repoRoot
}

$license = Join-Path $source 'LICENSE'
if (-not (Test-Path -LiteralPath $license -PathType Leaf)) {
    $license = Join-Path $repoRoot 'LICENSE'
}
$notices = Join-Path $source 'ThirdPartyNotices.md'
if (-not (Test-Path -LiteralPath $notices -PathType Leaf)) {
    $notices = Join-Path $repoRoot 'ThirdPartyNotices.md'
}

if (-not (Test-Path -LiteralPath $license -PathType Leaf)) {
    throw "LICENSE was not found: $license"
}
if (-not (Test-Path -LiteralPath $notices -PathType Leaf)) {
    throw "Third-party notices were not found: $notices"
}

if (Test-Path -LiteralPath $destination) {
    Remove-Item -LiteralPath $destination -Recurse -Force
}
[void][IO.Directory]::CreateDirectory($destination)

# Copy binaries, libraries, and plugins from release output
$entries = Get-ChildItem -LiteralPath $source -Recurse -Force | Sort-Object { $_.FullName.Length }
foreach ($entry in $entries) {
    if ($entry.FullName.Length -le $source.Length) { continue }
    $relativePath = $entry.FullName.Substring($source.Length).TrimStart('\', '/')
    $pathSegments = $relativePath -split '[\\/]+'
    
    # Skip intermediate compilation folders
    if ($pathSegments -contains '.fingerprint' -or $pathSegments -contains 'build' -or $pathSegments -contains 'deps' -or $pathSegments -contains 'incremental' -or $pathSegments -contains 'examples') {
        continue
    }

    $targetPath = Join-Path $destination $relativePath
    if ($entry.PSIsContainer) {
        [void][IO.Directory]::CreateDirectory($targetPath)
        continue
    }

    if ($entry.Extension -in @('.pdb', '.lib', '.d', '.rlib', '.lock', '.cargo-lock', '.cargo-artifact-lock', '.cargo-build-lock')) {
        continue
    }

    $targetDirectory = Split-Path -Parent $targetPath
    [void][IO.Directory]::CreateDirectory($targetDirectory)
    Copy-Item -LiteralPath $entry.FullName -Destination $targetPath -Force
}

# Ensure MusicPlayer.exe in destination matches the latest music-player-app.exe build and omit duplicate Cargo exe
$destMusicPlayer = Join-Path $destination 'MusicPlayer.exe'
$destAppExe = Join-Path $destination 'music-player-app.exe'
if (Test-Path -LiteralPath $destAppExe) {
    Copy-Item -LiteralPath $destAppExe -Destination $destMusicPlayer -Force
    Remove-Item -LiteralPath $destAppExe -Force
}

# Ensure Assets folder is copied (for installer icon and branding)
$destAssets = Join-Path $destination 'Assets'
[void][IO.Directory]::CreateDirectory($destAssets)
$sourceAssets = Join-Path $rustRoot 'crates\app\assets'
if (Test-Path $sourceAssets) {
    Copy-Item -Path (Join-Path $sourceAssets '*') -Destination $destAssets -Recurse -Force
}
$destLowerAssets = Join-Path $destination 'assets'
if (-not (Test-Path $destLowerAssets)) {
    [void][IO.Directory]::CreateDirectory($destLowerAssets)
    if (Test-Path $sourceAssets) {
        Copy-Item -Path (Join-Path $sourceAssets '*') -Destination $destLowerAssets -Recurse -Force
    }
}

# Copy LICENSE and ThirdPartyNotices.md
Copy-Item -LiteralPath $license -Destination (Join-Path $destination 'LICENSE') -Force
Copy-Item -LiteralPath $notices -Destination (Join-Path $destination 'ThirdPartyNotices.md') -Force

# Generate native Rust Portable-README.txt
$readme = @'
Music Player __VERSION__ — portable for Windows __ARCHITECTURE__ (Native Rust + Slint)

Launch MusicPlayer.exe from this folder.

To add this portable copy to Open with and add Add to Music Player queue / Create Music Player playlist to supported audio-file context menus for your Windows account, run:
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\Register-MusicPlayer-FileActions.ps1
Or run directly from a command line:
.\MusicPlayer.exe --register-associations

To remove those entries before moving or deleting this folder, run:
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\Register-MusicPlayer-FileActions.ps1 -Unregister
Or run directly:
.\MusicPlayer.exe --unregister-associations

To uninstall this portable copy and optionally remove its local index, playlists, settings, artwork cache, and logs, run:
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\Uninstall-MusicPlayer.ps1
On Windows 11, classic context-menu actions are under Show more options.

Your library database, settings, artwork cache, and logs are stored in:
%LOCALAPPDATA%\MusicPlayer

This package contains the self-contained native Windows __ARCHITECTURE__ app built with Rust, Slint, and LibVLC. No external runtimes (.NET or WebView2) are required.
See LICENSE and ThirdPartyNotices.md for license information.
'@
$readme = $readme.Replace('__VERSION__', $Version).Replace('__ARCHITECTURE__', $Architecture)
[IO.File]::WriteAllText(
    (Join-Path $destination 'Portable-README.txt'),
    $readme,
    [Text.UTF8Encoding]::new($false)
)

Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'Register-MusicPlayer-FileActions.ps1') -Destination (Join-Path $destination 'Register-MusicPlayer-FileActions.ps1') -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'Uninstall-MusicPlayer.ps1') -Destination (Join-Path $destination 'Uninstall-MusicPlayer.ps1') -Force

# Validation assertions
if (-not (Test-Path -LiteralPath (Join-Path $destination 'MusicPlayer.exe') -PathType Leaf)) {
    throw 'Staged package is missing MusicPlayer.exe.'
}
if (-not (Test-Path -LiteralPath (Join-Path $destination 'libvlc.dll') -PathType Leaf)) {
    throw 'Staged package is missing libvlc.dll.'
}
if (-not (Test-Path -LiteralPath (Join-Path $destination 'libvlccore.dll') -PathType Leaf)) {
    throw 'Staged package is missing libvlccore.dll.'
}
if (-not (Test-Path -LiteralPath (Join-Path $destination 'plugins') -PathType Container)) {
    throw 'Staged package is missing the VLC plugins directory.'
}
if (-not (Test-Path -LiteralPath (Join-Path $destination 'Assets\MusicPlayer.ico') -PathType Leaf)) {
    throw 'Staged package is missing Assets\MusicPlayer.ico.'
}
if (-not (Test-Path -LiteralPath (Join-Path $destination 'LICENSE') -PathType Leaf)) {
    throw 'Staged package is missing LICENSE.'
}
if (-not (Test-Path -LiteralPath (Join-Path $destination 'ThirdPartyNotices.md') -PathType Leaf)) {
    throw 'Staged package is missing ThirdPartyNotices.md.'
}
if (-not (Test-Path -LiteralPath (Join-Path $destination 'Register-MusicPlayer-FileActions.ps1') -PathType Leaf)) {
    throw 'Staged package is missing the portable file-action registration script.'
}
