param(
    [switch] $RemoveUserData,
    [switch] $Quiet
)

$ErrorActionPreference = 'Stop'
$installDirectory = [IO.Path]::GetFullPath($PSScriptRoot)
$executable = Join-Path $installDirectory 'MusicPlayer.exe'
$registrationScript = Join-Path $installDirectory 'Register-MusicPlayer-FileActions.ps1'

if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    throw 'Run this uninstaller from the portable Music Player folder.'
}

$runningCopies = Get-CimInstance Win32_Process -Filter "Name = 'MusicPlayer.exe'" -ErrorAction SilentlyContinue |
    Where-Object { $_.ExecutablePath -and [IO.Path]::GetFullPath($_.ExecutablePath).Equals($executable, [StringComparison]::OrdinalIgnoreCase) }
foreach ($process in $runningCopies) {
    $running = Get-Process -Id $process.ProcessId -ErrorAction SilentlyContinue
    if ($running) {
        $null = $running.CloseMainWindow()
        if (-not $running.WaitForExit(15000)) {
            throw 'Close Music Player before uninstalling so its playback session and library can be saved.'
        }
    }
}

if (Test-Path -LiteralPath $registrationScript -PathType Leaf) {
    & $registrationScript -Unregister -Quiet
}

$dataDirectory = [IO.Path]::GetFullPath((Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)) 'MusicPlayer'))

if (-not $Quiet -and -not $RemoveUserData) {
    $answer = Read-Host 'Also delete the library index, playlists, favorites, settings, artwork cache, and logs? Music files are never deleted. Type DELETE to confirm, or press Enter to keep this data'
    $RemoveUserData = $answer -ceq 'DELETE'
}

if ($RemoveUserData -and (Test-Path -LiteralPath $dataDirectory -PathType Container)) {
    if (Get-Process MusicPlayer -ErrorAction SilentlyContinue) {
        throw 'Close all Music Player copies before deleting their shared saved data.'
    }
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
    Write-Host 'The app has been unregistered and stopped. Delete this portable folder to remove its program files.'
}
