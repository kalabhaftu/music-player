param([Parameter(Mandatory=$true)][string] $SetupPath,[Parameter(Mandatory=$true)][string] $PreviousSetupPath,[Parameter(Mandatory=$true)][string] $Architecture)
$ErrorActionPreference='Stop'
if($env:GITHUB_ACTIONS -ne 'true'){throw 'Install tests require an isolated GitHub Windows runner.'}
$setup=(Resolve-Path -LiteralPath $SetupPath).Path
$install=Join-Path $env:RUNNER_TEMP "installed-music-player-$Architecture"
$data=Join-Path $env:LOCALAPPDATA 'MusicPlayer'
$interactionErrors=[Collections.Generic.List[string]]::new()
$script:installerAttempt=0
function Test-Interaction([string] $Executable){
    try { & (Join-Path $PSScriptRoot 'Run-Smoke.ps1') -Executable $Executable }
    catch { $interactionErrors.Add($_.Exception.Message); Write-Warning "UI check failed; independent installer lifecycle checks will continue: $($_.Exception.Message)" }
}
function Run-Installer([string] $File,[string[]] $Arguments){
    $script:installerAttempt++
    $evidence=[IO.Path]::GetFullPath('artifacts/ui-evidence')
    New-Item -ItemType Directory -Path $evidence -Force | Out-Null
    $log=Join-Path $evidence "$Architecture-installer-$script:installerAttempt.log"
    $script:lastInstallerLog=$log
    $process=Start-Process -FilePath $File -ArgumentList ($Arguments+@('/LOG="'+$log+'"')) -WindowStyle Hidden -Wait -PassThru
    if($process.ExitCode -notin @(0,3010)){throw "Installer exited with $($process.ExitCode)."}
}
function Verify-Signature([string] $File){
    $signature=Get-AuthenticodeSignature -LiteralPath $File
    $public=[Security.Cryptography.X509Certificates.X509Certificate2]::new((Resolve-Path 'packaging/windows/MusicPlayer-Signing.cer').Path)
    if($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Thumbprint -ne $public.Thumbprint -or !$signature.TimeStamperCertificate){throw "Invalid first-party signature: $File"}
}
Verify-Signature $setup
Verify-Signature $PreviousSetupPath
Run-Installer (Resolve-Path $PreviousSetupPath).Path @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART',"/DIR=`"$install`"")
$registration='HKCU:/Software/Microsoft/Windows/CurrentVersion/Uninstall/{8B3371D0-13A1-4410-9E60-A49AD1A5F08C}_is1'
if((Get-ItemProperty -LiteralPath $registration).DisplayVersion -ne '0.9.9'){throw 'The previous setup fixture has the wrong installed version.'}
$exe=Join-Path $install 'MusicPlayer.exe'
$uninstaller=Join-Path $install 'unins000.exe'
foreach($file in @($exe,(Join-Path $install 'MusicPlayer.dll'),(Join-Path $install 'MusicPlayer.Core.dll'),$uninstaller)){Verify-Signature $file}
foreach($script in Get-ChildItem -LiteralPath $install -Filter '*.ps1'){Verify-Signature $script.FullName}
if(!(Test-Path 'HKCU:/Software/Classes/MusicPlayer.MusicPlayer.wav/shell/open/command')){throw 'Setup file association was not registered.'}
Test-Interaction $exe
if(!(Test-Path (Join-Path $data 'library.db'))){throw 'Installed library was not persisted.'}
$sentinel=Join-Path $data 'retention-test.txt'
'Retain installed user data' | Set-Content -LiteralPath $sentinel
$ownedMusic=Join-Path $install 'user-owned.wav'
$sourceMusic=Join-Path ([Environment]::GetFolderPath('MyMusic')) 'MusicPlayerSmoke/MusicPlayerSmoke-A.wav'
Copy-Item -LiteralPath $sourceMusic -Destination $ownedMusic
$ownedHash=(Get-FileHash -LiteralPath $ownedMusic -Algorithm SHA256).Hash
# Replace the previous-version installation with the final signed installer.
Run-Installer $setup @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART',"/DIR=`"$install`"")
if(!(Select-String -LiteralPath $script:lastInstallerLog -SimpleMatch 'WebView2 Runtime already installed; skipping runtime installation.' -Quiet)){throw 'Setup upgrade did not skip installation of the existing WebView2 Runtime.'}
if(!(Test-Path -LiteralPath $sentinel)){throw 'Setup upgrade removed user data.'}
if(!(Test-Path -LiteralPath $ownedMusic) -or (Get-FileHash -LiteralPath $ownedMusic -Algorithm SHA256).Hash -ne $ownedHash){throw 'Setup upgrade removed or changed a user-owned file in the install folder.'}
if((Get-ItemProperty -LiteralPath $registration).DisplayVersion -ne '1.0.0'){throw 'Setup did not register the final upgraded version.'}
$env:MUSICPLAYER_TEST_OUTPUT='artifacts/ui-evidence/upgraded-setup'
try { Test-Interaction $exe }
finally {Remove-Item Env:/MUSICPLAYER_TEST_OUTPUT -ErrorAction SilentlyContinue}
Run-Installer $uninstaller @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART')
if(!(Test-Path -LiteralPath $sentinel)){throw 'Default uninstall failed to retain user data.'}
if(Test-Path -LiteralPath $exe){throw 'Uninstall left the application executable.'}
if(!(Test-Path -LiteralPath $ownedMusic)){throw 'Uninstall removed a user-owned file in the install folder.'}
if(Test-Path 'HKCU:/Software/Classes/MusicPlayer.MusicPlayer.wav'){throw 'Uninstall left the registered file association.'}
Run-Installer $setup @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART',"/DIR=`"$install`"")
Run-Installer $uninstaller @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/REMOVEUSERDATA=1')
if(Test-Path -LiteralPath $data){throw 'Explicit data-removal uninstall retained user data.'}
$music=Join-Path ([Environment]::GetFolderPath('MyMusic')) 'MusicPlayerSmoke/MusicPlayerSmoke-A.wav'
if(!(Test-Path -LiteralPath $music)){throw 'Uninstall removed a source music file.'}
[pscustomobject]@{architecture=$Architecture;setupInstall=$true;upgradeFrom='0.9.9';signedEmbeddedUninstaller=$true;retainData=$true;removeData=$true;sourceMusicPreserved=$true;userOwnedInstallFilePreserved=(Test-Path $ownedMusic);interactionPassed=($interactionErrors.Count -eq 0);interactionErrors=$interactionErrors.ToArray()} | ConvertTo-Json | Set-Content 'artifacts/ui-evidence/setup-results.json'
if($interactionErrors.Count){throw "Installed setup interaction checks failed: $($interactionErrors -join '; ')"}
