param([Parameter(Mandatory=$true)][string] $Executable,[string] $ApplicationId)
$ErrorActionPreference='Stop'
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'This smoke check must run on an isolated GitHub runner.' }
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class MusicPlayerShellTest {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string name, string title);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window,out uint id);
}
'@
& (Join-Path $PSScriptRoot 'Probe-NotificationArea.ps1')
# A fresh Windows 11 image can leave Start covering the desktop and ignoring
# injected Escape. Recover that observed system UI before launching any player.
$foreground=[MusicPlayerShellTest]::GetForegroundWindow()
$foregroundOwner=[uint32]0
$null=[MusicPlayerShellTest]::GetWindowThreadProcessId($foreground,[ref]$foregroundOwner)
$startMenu=Get-Process -Id $foregroundOwner -ErrorAction SilentlyContinue
if($startMenu -and $startMenu.ProcessName -eq 'StartMenuExperienceHost'){
    if(Get-Process MusicPlayer -ErrorAction SilentlyContinue){throw 'Disposable Start-menu recovery must precede player launch.'}
    if($startMenu.SessionId -ne (Get-Process -Id $PID).SessionId -or !$startMenu.Path -or
        !$startMenu.Path.StartsWith((Join-Path $env:WINDIR 'SystemApps')+'\',[StringComparison]::OrdinalIgnoreCase)){
        throw 'The foreground Start menu is not the disposable session Windows system process.'
    }
    Write-Host 'Resetting the foreground Start menu on the disposable Windows desktop.'
    Stop-Process -Id $startMenu.Id -Force
    for($attempt=0;$attempt -lt 30 -and [MusicPlayerShellTest]::GetForegroundWindow() -eq $foreground;$attempt++){Start-Sleep -Milliseconds 100}
    if([MusicPlayerShellTest]::GetForegroundWindow() -eq $foreground){throw 'The disposable Start-menu overlay did not clear.'}
}
& (Join-Path $PSScriptRoot 'Close-DisposableWelcomeWindows.ps1')
# Tray and thumbnail tests need Explorer in this disposable runner session.
if([MusicPlayerShellTest]::FindWindow('Shell_TrayWnd',$null) -eq [IntPtr]::Zero){
    Start-Process explorer.exe -WindowStyle Hidden
    for($attempt=0;$attempt -lt 60 -and [MusicPlayerShellTest]::FindWindow('Shell_TrayWnd',$null) -eq [IntPtr]::Zero;$attempt++){Start-Sleep -Milliseconds 500}
    if([MusicPlayerShellTest]::FindWindow('Shell_TrayWnd',$null) -eq [IntPtr]::Zero){throw 'The isolated Windows session has no Explorer notification area; tray validation cannot run.'}
}
$music=Join-Path ([Environment]::GetFolderPath('MyMusic')) 'MusicPlayerSmoke'
New-Item -ItemType Directory -Path $music -Force | Out-Null
$videoFixture=Join-Path $env:MUSICPLAYER_MEDIA_FIXTURES 'fixture.mkv'
if(!(Test-Path -LiteralPath $videoFixture)){throw 'The genuine video/subtitle fixture is required for UI validation.'}
Copy-Item -LiteralPath $videoFixture -Destination (Join-Path $music 'MusicPlayerVideoSmoke.mkv') -Force
foreach($name in @('A','B','C','D')) {
    $path=Join-Path $music "MusicPlayerSmoke-$name.wav"
    $writer=[IO.BinaryWriter]::new([IO.File]::Create($path))
    try {
        $size=44100*90*2
        $writer.Write([Text.Encoding]::ASCII.GetBytes('RIFF')); $writer.Write([int](36+$size))
        $writer.Write([Text.Encoding]::ASCII.GetBytes('WAVEfmt ')); $writer.Write([int]16)
        $writer.Write([int16]1); $writer.Write([int16]1); $writer.Write([int]44100); $writer.Write([int]88200)
        $writer.Write([int16]2); $writer.Write([int16]16)
        $writer.Write([Text.Encoding]::ASCII.GetBytes('data')); $writer.Write([int]$size)
        $samples=[byte[]]::new($size)
        # Distinct valid PCM fixtures must survive default exact-duplicate hiding.
        $samples[0]=[byte][char]$name
        $writer.Write($samples)
    } finally {$writer.Dispose()}
    '[00:00.00]Smoke lyric one' + "`n" + '[00:02.00]Smoke lyric two' | Set-Content -LiteralPath (Join-Path $music "MusicPlayerSmoke-$name.lrc")
}
# Elevated WebView2 150+ ignores environment/HKCU overrides. This app-specific
# machine policy is set only on the disposable runner and removed in finally.
$policy='HKLM:/Software/Policies/Microsoft/Edge/WebView2/AdditionalBrowserArguments'
New-Item -Path $policy -Force | Out-Null
New-ItemProperty -Path $policy -Name 'MusicPlayer.exe' -Value '--remote-debugging-port=9222' -PropertyType String -Force | Out-Null
$exe=(Resolve-Path -LiteralPath $Executable).Path
# Loading the installed TagLib assembly into this long-lived test host locks it
# across setup upgrade/uninstall. Prepare the fixture in a process that exits
# before launching the app; hiding it also preserves the interactive desktop.
$fixtureArguments=@('-NoLogo','-NoProfile','-File',('"'+(Join-Path $PSScriptRoot 'Prepare-ArtworkFixture.ps1')+'"'),
    '-TrackPath',('"'+(Join-Path $music 'MusicPlayerSmoke-A.wav')+'"'),
    '-TagLibPath',('"'+(Join-Path (Split-Path -Parent $exe) 'TagLibSharp.dll')+'"'))
$fixtureProcess=Start-Process -FilePath (Join-Path $env:WINDIR 'System32/WindowsPowerShell/v1.0/powershell.exe') -ArgumentList $fixtureArguments -WindowStyle Hidden -Wait -PassThru
if($fixtureProcess.ExitCode -ne 0){throw "Isolated artwork fixture preparation failed with $($fixtureProcess.ExitCode)."}
function Save-Diagnostics {
    $out=Join-Path $PWD 'artifacts/ui-evidence'
    New-Item -ItemType Directory -Path $out -Force | Out-Null
    Get-Process MusicPlayer,msedgewebview2 -ErrorAction SilentlyContinue | Select-Object Id,ProcessName,MainWindowHandle,MainWindowTitle,WorkingSet64,CPU | ConvertTo-Json | Set-Content (Join-Path $out 'startup-processes.json')
    $logs=Join-Path $env:LOCALAPPDATA 'MusicPlayer/Logs'
    if(Test-Path -LiteralPath $logs){Copy-Item -LiteralPath $logs -Destination $out -Recurse -Force}
    Get-WinEvent -FilterHashtable @{LogName='Application';StartTime=(Get-Date).AddMinutes(-10)} -ErrorAction SilentlyContinue | Where-Object {$_.ProviderName -in @('Application Error','.NET Runtime')} | Select-Object TimeCreated,ProviderName,Message | ConvertTo-Json | Set-Content (Join-Path $out 'startup-errors.json')
}
function Start-App {
    if($ApplicationId){return & (Join-Path $PSScriptRoot 'Start-PackagedApp.ps1') -ApplicationId $ApplicationId}
    Start-Process -FilePath $exe -PassThru
}
try {
    foreach($arguments in @(@(),@('--restart'))){
        $process=Start-App
        $env:MUSICPLAYER_TEST_APP_PID=[string]$process.Id
        try {
            node (Join-Path $PSScriptRoot 'smoke.mjs') @arguments
            if($LASTEXITCODE -ne 0){throw 'Windows UI smoke failed.'}
            $portableUninstaller=Join-Path (Split-Path -Parent $exe) 'Uninstall-MusicPlayer.ps1'
            if(!$ApplicationId -and $arguments -contains '--restart' -and (Test-Path -LiteralPath $portableUninstaller)){
                & $portableUninstaller -Quiet
                if(!(Test-Path (Join-Path $env:LOCALAPPDATA 'MusicPlayer/library.db'))){throw 'Portable uninstall did not retain the saved library.'}
                if(!(Test-Path (Join-Path $music 'MusicPlayerSmoke-A.wav'))){throw 'Portable uninstall removed source music.'}
            }
        } finally {
            Save-Diagnostics
            $process.Refresh()
            if(!$process.HasExited){$null=$process.CloseMainWindow();if(!$process.WaitForExit(20000)){Stop-Process -Id $process.Id -Force}}
        }
    }
} finally {
    Remove-ItemProperty -Path $policy -Name 'MusicPlayer.exe' -ErrorAction SilentlyContinue
    Remove-Item Env:/MUSICPLAYER_TEST_APP_PID -ErrorAction SilentlyContinue
}
