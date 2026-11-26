param([Parameter(Mandatory=$true)][string] $BundlePath,[string] $Destination='artifacts/upgrade-fixtures')
$ErrorActionPreference='Stop'
if($env:GITHUB_ACTIONS -ne 'true'){throw 'Upgrade fixture packaging is restricted to the disposable runner.'}
$root=Join-Path $env:RUNNER_TEMP ('music-player-upgrade-'+[Guid]::NewGuid().ToString('N'))
$unbundle=Join-Path $root 'unbundle'
$packages=Join-Path $root 'packages'
New-Item -ItemType Directory -Path $packages -Force | Out-Null
New-Item -ItemType Directory -Path $Destination -Force | Out-Null
$sdk=Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/bin'
$makeappx=Get-ChildItem (Join-Path $sdk '*/x64/makeappx.exe') | Sort-Object FullName -Descending | Select-Object -First 1 -ExpandProperty FullName
$signtool=Get-ChildItem (Join-Path $sdk '*/x64/signtool.exe') | Sort-Object FullName -Descending | Select-Object -First 1 -ExpandProperty FullName
$public=[Security.Cryptography.X509Certificates.X509Certificate2]::new((Resolve-Path 'packaging/windows/MusicPlayer-Signing.cer').Path)
function Check-Tool {if($LASTEXITCODE -ne 0){throw 'MSIX upgrade fixture tool failed.'}}
& $makeappx unbundle /p (Resolve-Path $BundlePath).Path /d $unbundle /o;Check-Tool
foreach($package in Get-ChildItem $unbundle -Filter '*.msix'){
    $files=Join-Path $root $package.BaseName
    & $makeappx unpack /p $package.FullName /d $files /o;Check-Tool
    $manifestPath=Join-Path $files 'AppxManifest.xml'
    [xml]$manifest=Get-Content $manifestPath -Raw
    $manifest.Package.Identity.Version='0.9.9.0'
    $manifest.Save($manifestPath)
    foreach($generated in @('AppxSignature.p7x','AppxBlockMap.xml','[Content_Types].xml')){
        $path=Join-Path $files $generated
        if(Test-Path -LiteralPath $path){Remove-Item -LiteralPath $path -Force}
    }
    $older=Join-Path $packages $package.Name
    & $makeappx pack /d $files /p $older /o;Check-Tool
    & $signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /sha1 $public.Thumbprint /s My $older;Check-Tool
}
$output=Join-Path ([IO.Path]::GetFullPath($Destination)) 'MusicPlayer-0.9.9-x64-arm64.msixbundle'
& $makeappx bundle /d $packages /p $output /bv 0.9.9.0 /o;Check-Tool
& $signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /sha1 $public.Thumbprint /s My $output;Check-Tool
& $signtool verify /pa /all /tw $output;Check-Tool
