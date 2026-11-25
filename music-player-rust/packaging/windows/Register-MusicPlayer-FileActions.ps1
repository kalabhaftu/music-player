param(
    [string] $ApplicationName = 'Music Player',
    [switch] $Unregister,
    [switch] $Quiet
)

$ErrorActionPreference = 'Stop'

# Find the executable
$executable = Join-Path $PSScriptRoot 'MusicPlayer.exe'
if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    $alt = Join-Path $PSScriptRoot 'music-player-app.exe'
    if (Test-Path -LiteralPath $alt -PathType Leaf) {
        $executable = $alt
    } else {
        throw "Music Player executable was not found: $executable"
    }
}
$executable = [IO.Path]::GetFullPath($executable)

# Delegate to native Rust --register-associations / --unregister-associations when available
if ($Unregister) {
    & $executable --unregister-associations
    if ($LASTEXITCODE -eq 0 -and -not $Quiet) {
        Write-Host 'Removed Music Player file associations and context menu verbs.'
        return
    }
} else {
    & $executable --register-associations
    if ($LASTEXITCODE -eq 0 -and -not $Quiet) {
        Write-Host 'Registered Music Player file associations and context menu verbs.'
        return
    }
}

# Fallback PowerShell implementation if executable invocation is not supported
$extensions = @('.mp3', '.flac', '.wav', '.wave', '.aif', '.aiff', '.m4a', '.m4b', '.aac', '.ogg', '.oga', '.opus', '.wma', '.ape', '.wv', '.tta', '.mpc', '.dsf', '.dff')
$registeredAppsKey = 'HKCU:\Software\RegisteredApplications'
$capabilitiesRelativePath = "Software\MusicPlayer\Capabilities\$ApplicationName"
$capabilities = "HKCU:\$capabilitiesRelativePath"
$associationName = "MusicPlayer.AssocFile"
$integrationRoot = "HKCU:\Software\MusicPlayer\ShellIntegration\$ApplicationName"
$appKey = "HKCU:\Software\Classes\Applications\$([IO.Path]::GetFileName($executable))"

function Set-DefaultValue([string] $Path, [string] $Value) {
    if (-not (Test-Path -LiteralPath $Path)) { New-Item -Path $Path -Force | Out-Null }
    Set-Item -LiteralPath $Path -Value $Value
}

function Set-StringValue([string] $Path, [string] $Name, [string] $Value) {
    if (-not (Test-Path -LiteralPath $Path)) { New-Item -Path $Path -Force | Out-Null }
    New-ItemProperty -LiteralPath $Path -Name $Name -Value $Value -PropertyType String -Force | Out-Null
}

if (-not ('MusicPlayerShellAssoc' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class MusicPlayerShellAssoc {
    [DllImport("shell32.dll")]
    public static extern void SHChangeNotify(uint eventId, uint flags, IntPtr item1, IntPtr item2);
}
'@
}

function Refresh-ShellAssociations {
    [MusicPlayerShellAssoc]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
}

if ($Unregister) {
    Remove-Item -LiteralPath $integrationRoot -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $capabilities -Recurse -Force -ErrorAction SilentlyContinue
    Remove-ItemProperty -LiteralPath $registeredAppsKey -Name $ApplicationName -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $appKey -Recurse -Force -ErrorAction SilentlyContinue
    foreach ($extension in $extensions) {
        $cleanExt = $extension.TrimStart('.')
        $progId = "$associationName.$cleanExt"
        Remove-ItemProperty -LiteralPath "HKCU:\Software\Classes\$extension\OpenWithProgids" -Name $progId -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath "HKCU:\Software\Classes\$progId" -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath "HKCU:\Software\Classes\SystemFileAssociations\$extension\shell\MusicPlayerAddToQueue" -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath "HKCU:\Software\Classes\SystemFileAssociations\$extension\shell\MusicPlayerCreatePlaylist" -Recurse -Force -ErrorAction SilentlyContinue
    }
    Refresh-ShellAssociations
    if (-not $Quiet) { Write-Host 'Removed Music Player file associations.' }
} else {
    Set-DefaultValue $integrationRoot $executable
    Set-StringValue $integrationRoot 'ExecutablePath' $executable
    Set-StringValue $registeredAppsKey $ApplicationName $capabilitiesRelativePath
    Set-StringValue $capabilities 'ApplicationName' $ApplicationName
    Set-StringValue $capabilities 'ApplicationDescription' 'Offline music library and player'
    Set-StringValue $capabilities 'ApplicationIcon' "`"$executable`",0"

    Set-DefaultValue "$appKey\shell\open\command" "`"$executable`" `"%1`""
    Set-StringValue "$appKey\shell\open" 'MultiSelectModel' 'Player'
    Set-StringValue "$appKey\shell\open\DropTarget" 'CLSID' '{A3B79A2E-C313-45F7-BB02-F9F4B0E4C101}'
    Set-DefaultValue "$appKey\DefaultIcon" "`"$executable`",0"

    foreach ($clsid in @(
        '{A3B79A2E-C313-45F7-BB02-F9F4B0E4C101}',
        '{A3B79A2E-C313-45F7-BB02-F9F4B0E4C102}',
        '{A3B79A2E-C313-45F7-BB02-F9F4B0E4C103}'
    )) {
        Set-DefaultValue "HKCU:\Software\Classes\CLSID\$clsid\LocalServer32" "`"$executable`" --shell-drop-target"
        Set-StringValue "HKCU:\Software\Classes\CLSID\$clsid\LocalServer32" 'ServerExecutable' $executable
    }

    foreach ($extension in $extensions) {
        $cleanExt = $extension.TrimStart('.')
        $progId = "$associationName.$cleanExt"
        Set-StringValue "$appKey\SupportedTypes" $extension ""
        Set-StringValue "$capabilities\FileAssociations" $extension $progId
        Set-StringValue "HKCU:\Software\Classes\$extension\OpenWithProgids" $progId ""
        Set-DefaultValue "HKCU:\Software\Classes\$progId" "$ApplicationName audio file"
        Set-DefaultValue "HKCU:\Software\Classes\$progId\DefaultIcon" "`"$executable`",0"
        Set-DefaultValue "HKCU:\Software\Classes\$progId\shell\open\command" "`"$executable`" `"%1`""
        Set-StringValue "HKCU:\Software\Classes\$progId\shell\open" 'MultiSelectModel' 'Player'
        Set-StringValue "HKCU:\Software\Classes\$progId\shell\open\DropTarget" 'CLSID' '{A3B79A2E-C313-45F7-BB02-F9F4B0E4C101}'

        $queueKey = "HKCU:\Software\Classes\SystemFileAssociations\$extension\shell\MusicPlayerAddToQueue"
        Set-DefaultValue $queueKey 'Add to Music Player queue'
        Set-StringValue $queueKey 'Icon' "`"$executable`",0"
        Set-StringValue $queueKey 'MultiSelectModel' 'Player'
        Set-StringValue "$queueKey\DropTarget" 'CLSID' '{A3B79A2E-C313-45F7-BB02-F9F4B0E4C102}'
        Set-DefaultValue "$queueKey\command" "`"$executable`" --add-to-queue `"%1`""

        $playKey = "HKCU:\Software\Classes\SystemFileAssociations\$extension\shell\MusicPlayerCreatePlaylist"
        Set-DefaultValue $playKey 'Create Music Player playlist'
        Set-StringValue $playKey 'Icon' "`"$executable`",0"
        Set-StringValue $playKey 'MultiSelectModel' 'Player'
        Set-StringValue "$playKey\DropTarget" 'CLSID' '{A3B79A2E-C313-45F7-BB02-F9F4B0E4C103}'
        Set-DefaultValue "$playKey\command" "`"$executable`" --create-playlist `"%1`""
    }
    Refresh-ShellAssociations
    if (-not $Quiet) { Write-Host 'Registered Music Player file associations.' }
}
