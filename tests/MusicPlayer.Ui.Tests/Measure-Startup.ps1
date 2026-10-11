param([Parameter(Mandatory=$true)][string] $BaselineExecutable,[Parameter(Mandatory=$true)][string] $CandidateExecutable)
$ErrorActionPreference='Stop'
if($env:GITHUB_ACTIONS -ne 'true'){throw 'Profile replacement is restricted to an isolated Windows runner.'}
$profile=[IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'MusicPlayer'))
if($profile -ne [IO.Path]::GetFullPath((Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'MusicPlayer'))){throw 'Unexpected measurement profile path.'}
$policy='HKLM:/Software/Policies/Microsoft/Edge/WebView2/AdditionalBrowserArguments'
New-Item $policy -Force | Out-Null
New-ItemProperty $policy -Name 'MusicPlayer.exe' -Value '--remote-debugging-port=9222' -PropertyType String -Force | Out-Null
try {
    foreach($count in @(100,100000)){
        $fixture=Join-Path $env:RUNNER_TEMP "fixture-$count"
        dotnet run --project tests/MusicPlayer.Benchmarks/MusicPlayer.Benchmarks.csproj --configuration Release -- --startup-fixture $fixture $count
        if($LASTEXITCODE -ne 0){throw 'Library fixture preparation failed.'}
        foreach($revision in @(@{name='before';exe=$BaselineExecutable},@{name='after';exe=$CandidateExecutable})){
            if(Get-Process MusicPlayer -ErrorAction SilentlyContinue){throw 'Measurement requires no running Music Player instance.'}
            # The preserved baseline predates explicit WebView shutdown. Wait
            # for its child browser to release profile files before replacement.
            for($attempt=0;Test-Path -LiteralPath $profile;$attempt++){
                try {Remove-Item -LiteralPath $profile -Recurse -Force}
                catch {
                    if($attempt -ge 59){throw}
                    Start-Sleep -Milliseconds 500
                }
            }
            Copy-Item -LiteralPath $fixture -Destination $profile -Recurse
            $env:MUSICPLAYER_TEST_OUTPUT="artifacts/ui-evidence/performance/$count/$($revision.name)"
            $env:MUSICPLAYER_TEST_REVISION=$revision.name
            $env:MUSICPLAYER_TEST_TRACK_COUNT=[string]$count
            $env:MUSICPLAYER_STARTED_MS=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds().ToString()
            $process=Start-Process (Resolve-Path $revision.exe).Path -PassThru
            $env:MUSICPLAYER_TEST_APP_PID=$process.Id.ToString()
            try {
                node (Join-Path $PSScriptRoot 'measure-startup.mjs')
                if($LASTEXITCODE -ne 0){throw "Startup measurement failed for $count/$($revision.name)."}
            } finally {
                $process.Refresh()
                if(!$process.HasExited){$null=$process.CloseMainWindow();if(!$process.WaitForExit(20000)){Stop-Process -Id $process.Id -Force}}
            }
        }
    }
} finally {
    Remove-ItemProperty $policy -Name 'MusicPlayer.exe' -ErrorAction SilentlyContinue
    foreach($variable in @('MUSICPLAYER_TEST_OUTPUT','MUSICPLAYER_TEST_REVISION','MUSICPLAYER_TEST_TRACK_COUNT','MUSICPLAYER_STARTED_MS','MUSICPLAYER_TEST_APP_PID')){Remove-Item "Env:/$variable" -ErrorAction SilentlyContinue}
}
