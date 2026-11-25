param(
    [switch] $RemoveUserData,
    [switch] $Quiet
)

$ErrorActionPreference = 'Stop'
$installDirectory = [IO.Path]::GetFullPath($PSScriptRoot)
$executable = Join-Path $installDirectory 'MusicPlayer.exe'
if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    $alt = Join-Path $installDirectory 'music-player-app.exe'
    if (Test-Path -LiteralPath $alt -PathType Leaf) {
        $executable = $alt
    }
}

# Close any running copies before uninstalling
$processNames = @('MusicPlayer', 'music-player-app')
foreach ($pName in $processNames) {
    $runningCopies = Get-Process -Name $pName -ErrorAction SilentlyContinue
    foreach ($proc in $runningCopies) {
        try {
            $null = $proc.CloseMainWindow()
            if (-not $proc.WaitForExit(5000)) {
                $proc.Kill()
            }
        } catch { }
    }
}

$registrationScript = Join-Path $installDirectory 'Register-MusicPlayer-FileActions.ps1'
if (Test-Path -LiteralPath $registrationScript -PathType Leaf) {
    & $registrationScript -Unregister -Quiet
} elseif (Test-Path -LiteralPath $executable -PathType Leaf) {
    & $executable --unregister-associations
}

$dataDirectory = [IO.Path]::GetFullPath((Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)) 'MusicPlayer'))

if (-not $Quiet -and -not $RemoveUserData) {
    $answer = Read-Host 'Also delete the library index, playlists, favorites, settings, artwork cache, and logs? Music files are never deleted. Type DELETE to confirm, or press Enter to keep this data'
    $RemoveUserData = $answer -ceq 'DELETE'
}

if ($RemoveUserData -and (Test-Path -LiteralPath $dataDirectory -PathType Container)) {
    $expectedData = [IO.Path]::GetFullPath((Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)) 'MusicPlayer'))
    if (-not $dataDirectory.Equals($expectedData, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Unexpected saved-data deletion target.'
    }
    Remove-Item -LiteralPath $dataDirectory -Recurse -Force
    if (-not $Quiet) { Write-Host 'Removed Music Player library data. Your music files were not changed.' }
} elseif (-not $Quiet) {
    Write-Host 'Kept Music Player library data. Remove this portable folder after closing the uninstaller.'
}

if (-not $Quiet) {
    Write-Host 'The app has been unregistered and stopped. Delete this folder to remove its program files.'
}
