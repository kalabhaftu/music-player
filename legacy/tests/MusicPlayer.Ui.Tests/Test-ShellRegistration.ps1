param([Parameter(Mandatory)][string] $Executable)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'Shell registration tests require a disposable Windows runner.' }
$directory = Split-Path -Parent (Resolve-Path -LiteralPath $Executable).Path
$helper = Join-Path $directory 'Register-MusicPlayer-FileActions.ps1'
$openWith = 'HKCU:/Software/Classes/.mp3/OpenWithProgids'
if (!(Test-Path -LiteralPath $openWith)) { New-Item -Path $openWith -Force | Out-Null }
New-ItemProperty -LiteralPath $openWith -Name 'OtherPlayer.Test' -Value '' -PropertyType String -Force | Out-Null
$provider = 'Registry::HKEY_CLASSES_ROOT/SystemFileAssociations/.mp3/shellex/{E357FCCD-A995-4576-B01F-234630154E96}'
$thumbnailBefore = if (Test-Path -LiteralPath $provider) { (Get-Item -LiteralPath $provider).GetValue('') } else { $null }
foreach ($attempt in 1..2) {
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $helper -Quiet
    if ($LASTEXITCODE) { throw 'Shell registration failed.' }
    foreach ($entry in @(@('MusicPlayerAddToQueue','Add to Music Player queue','--add-to-queue'), @('MusicPlayerCreatePlaylist','Create Music Player playlist','--create-playlist'))) {
        $path = "HKCU:/Software/Classes/SystemFileAssociations/.mp3/shell/$($entry[0])"
        $key = Get-Item -LiteralPath $path
        if ($key.GetValue('') -ne $entry[1] -or !$key.GetValue('Icon') -or $key.GetValue('MultiSelectModel') -ne 'Player') { throw 'A repeated registration lost its menu label or icon.' }
        if ((Get-Item -LiteralPath "$path/command").GetValue('') -notlike "*$($entry[2])*") { throw 'The action switch was lost.' }
    }
    if ((Get-Item -LiteralPath $openWith).GetValueNames() -notcontains 'OtherPlayer.Test') { throw 'Registration removed another player.' }
    $progId = 'HKCU:/Software/Classes/MusicPlayer.MusicPlayer.mp3'
    if ((Get-Item -LiteralPath $progId).GetValue('TypeOverlay') -ne (Get-Item -LiteralPath "$progId/DefaultIcon").GetValue('')) { throw 'File icon and thumbnail overlay disagree.' }
}
$thumbnailAfter = if (Test-Path -LiteralPath $provider) { (Get-Item -LiteralPath $provider).GetValue('') } else { $null }
if ($thumbnailBefore -ne $thumbnailAfter) { throw 'Registration replaced the Windows album-art thumbnail provider.' }
Write-Host 'Repeated registration preserved labels, icons, action switches, other players and thumbnail providers.'
