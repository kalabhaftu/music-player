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
$executable = Join-Path $source 'MusicPlayer.exe'
$license = Join-Path $source 'LICENSE'
$notices = Join-Path $source 'ThirdPartyNotices.md'

if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    throw "Published app executable was not found: $executable"
}
if (-not (Test-Path -LiteralPath $license -PathType Leaf)) {
    throw "Published LICENSE was not found: $license"
}
if (-not (Test-Path -LiteralPath $notices -PathType Leaf)) {
    throw "Published third-party notices were not found: $notices"
}
if (Test-Path -LiteralPath $destination) {
    throw "Package destination already exists: $destination"
}

[void][IO.Directory]::CreateDirectory($destination)

$entries = Get-ChildItem -LiteralPath $source -Recurse -Force | Sort-Object { $_.FullName.Length }
foreach ($entry in $entries) {
    $relativePath = [IO.Path]::GetRelativePath($source, $entry.FullName)
    $pathSegments = $relativePath -split '[\\/]+'
    if ($pathSegments -contains 'WebUI' -or ($pathSegments | Where-Object { $_ -match '\.WebView2$' })) {
        continue
    }
    $targetPath = Join-Path $destination $relativePath

    if ($entry.PSIsContainer) {
        [void][IO.Directory]::CreateDirectory($targetPath)
        continue
    }

    if ($entry.Extension -in @('.pdb', '.lib', '.appxrecipe', '.msix', '.msixbundle', '.appinstaller')) {
        continue
    }

    $targetDirectory = Split-Path -Parent $targetPath
    [void][IO.Directory]::CreateDirectory($targetDirectory)
    Copy-Item -LiteralPath $entry.FullName -Destination $targetPath
}

$readme = @'
Music Player __VERSION__ — portable for Windows __ARCHITECTURE__

Launch MusicPlayer.exe from this folder.

To add this portable copy to Open with and add Add to Music Player queue / Create Music Player playlist to supported audio-file context menus for your Windows account, run:
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\Register-MusicPlayer-FileActions.ps1
To remove those entries before moving or deleting this folder, run the same command with -Unregister.
To uninstall this portable copy and optionally remove its local index, playlists, settings, artwork cache, and logs, run:
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\Uninstall-MusicPlayer.ps1
On Windows 11, these classic context-menu actions are under Show more options.

Your library database, settings, artwork cache, and logs are stored in:
%LOCALAPPDATA%\MusicPlayer

This package contains the self-contained Windows __ARCHITECTURE__ app and its .NET and Windows App SDK runtime files. It uses the shared Microsoft Edge WebView2 Evergreen Runtime installed on Windows. If WebView2 is missing, install it from https://developer.microsoft.com/microsoft-edge/webview2/ before launching Music Player.
See LICENSE and ThirdPartyNotices.md for license information.
'@
$readme = $readme.Replace('__VERSION__', $Version).Replace('__ARCHITECTURE__', $Architecture)
[IO.File]::WriteAllText(
    (Join-Path $destination 'Portable-README.txt'),
    $readme,
    [Text.UTF8Encoding]::new($false)
)
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'Register-MusicPlayer-FileActions.ps1') -Destination (Join-Path $destination 'Register-MusicPlayer-FileActions.ps1')
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'Uninstall-MusicPlayer.ps1') -Destination (Join-Path $destination 'Uninstall-MusicPlayer.ps1')

if (-not (Test-Path -LiteralPath (Join-Path $destination 'MusicPlayer.exe') -PathType Leaf)) {
    throw 'Staged package is missing MusicPlayer.exe.'
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
if (-not (Test-Path -LiteralPath (Join-Path $destination 'Uninstall-MusicPlayer.ps1') -PathType Leaf)) {
    throw 'Staged package is missing the portable uninstaller.'
}
if (Get-ChildItem -LiteralPath $destination -Recurse -File -Force | Where-Object { $_.Extension -in @('.pdb', '.lib') }) {
    throw 'Staged package unexpectedly contains a PDB or LIB file.'
}
