param(
    [switch] $RemoveUserData,
    [string] $PackageFamilyName = 'Kalabhaftu.MusicPlayer_4f9y3f1m9j27a'
)

$ErrorActionPreference = 'Stop'

# Preserves library data when uninstalling legacy MSIX package
$appData = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
$packagedData = Join-Path $appData "Packages\$PackageFamilyName\LocalCache\Local\MusicPlayer"
$localData = Join-Path $appData 'MusicPlayer'
$backupRoot = Join-Path $appData 'MusicPlayerUninstallBackups'

if (Test-Path -LiteralPath $packagedData -PathType Container) {
    if (-not (Test-Path -LiteralPath $localData)) {
        Copy-Item -LiteralPath $packagedData -Destination $localData -Recurse -Force
    } else {
        $backupDir = Join-Path $backupRoot (Get-Date -Format 'yyyyMMdd-HHmmss')
        New-Item -ItemType Directory -Path $backupDir -Force | Out-Null
        Copy-Item -LiteralPath $packagedData -Destination (Join-Path $backupDir 'MSIX-MusicPlayer') -Recurse -Force
    }
}

Get-AppxPackage -Name '*MusicPlayer*' -ErrorAction SilentlyContinue | Remove-AppxPackage -ErrorAction SilentlyContinue

if ($RemoveUserData -and (Test-Path -LiteralPath $localData)) {
    Remove-Item -LiteralPath $localData -Recurse -Force
}
