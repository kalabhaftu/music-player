param([switch] $RemoveUserData,[switch] $Quiet)
$ErrorActionPreference='Stop'
if($PSEdition -eq 'Core'){Import-Module Appx -UseWindowsPowerShell -ErrorAction Stop}
$package=Get-AppxPackage -Name 'Kalabhaftu.MusicPlayer'
if(!$package){throw 'Music Player MSIX is not installed for this account.'}
$packageDirectory=[IO.Path]::GetFullPath($package.InstallLocation)
foreach($process in Get-Process MusicPlayer -ErrorAction SilentlyContinue){
    if($process.Path -and [IO.Path]::GetFullPath($process.Path).StartsWith($packageDirectory+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){
        $null=$process.CloseMainWindow()
        if(!$process.WaitForExit(15000)){throw 'Close Music Player before uninstalling so its library can be saved.'}
    }
}
if(!$Quiet -and !$RemoveUserData){
    $answer=Read-Host 'Type DELETE to remove the library, playlists, settings and cache, or press Enter to retain them. Music files are never deleted'
    $RemoveUserData=$answer -ceq 'DELETE'
}
$local=[Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
$data=[IO.Path]::GetFullPath((Join-Path $local 'MusicPlayer'))
$privateData=[IO.Path]::GetFullPath((Join-Path $local "Packages/$($package.PackageFamilyName)/LocalCache/Local/MusicPlayer"))
if(!$RemoveUserData -and (Test-Path -LiteralPath $privateData)){
    # Copy only after the app closes, including SQLite sidecars and recovery files.
    # Retain an existing unpackaged profile before replacing it with the MSIX one.
    if(Test-Path -LiteralPath $data){
        $backup=Join-Path $local ('MusicPlayerUninstallBackups/'+[DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss')+'-'+[Guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path (Split-Path $backup) -Force | Out-Null
        Copy-Item -LiteralPath $data -Destination $backup -Recurse
    }
    New-Item -ItemType Directory -Path $data -Force | Out-Null
    foreach($entry in Get-ChildItem -LiteralPath $privateData -Force){Copy-Item -LiteralPath $entry.FullName -Destination $data -Recurse -Force}
    if(!(Test-Path (Join-Path $data 'library.db'))){throw 'The retained library copy could not be verified; uninstall has been stopped.'}
}
Remove-AppxPackage -Package $package.PackageFullName
if($RemoveUserData -and (Test-Path -LiteralPath $data)){
    if(Get-Process MusicPlayer -ErrorAction SilentlyContinue){throw 'Close all Music Player copies before deleting their shared saved data.'}
    # Resolve and constrain the one deletion target; source music is outside it.
    if($data -ne [IO.Path]::GetFullPath((Join-Path $local 'MusicPlayer'))){throw 'Unexpected data deletion target.'}
    Remove-Item -LiteralPath $data -Recurse -Force
}
if(!$Quiet){Write-Host $(if($RemoveUserData){'Removed Music Player and its user data. Music files and recovery backups are unchanged.'}else{'Removed Music Player and retained its library in %LOCALAPPDATA%\MusicPlayer.'})}
