param([string] $ExpectedVersion)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$metadata = ([xml](Get-Content -LiteralPath (Join-Path $root 'Directory.Build.props') -Raw)).Project.PropertyGroup
$manifest = ([xml](Get-Content -LiteralPath (Join-Path $root 'src/MusicPlayer.App/Package.appxmanifest') -Raw)).Package.Identity
$project = [IO.File]::ReadAllText((Join-Path $root 'src/MusicPlayer.App/MusicPlayer.App.csproj'))
if ($metadata.Version -notmatch '^\d+\.\d+\.\d+$' -or $metadata.AppxPackageVersion -ne "$($metadata.Version).0") { throw 'Shared assembly/MSIX versions disagree.' }
if ($manifest.Version -ne $metadata.AppxPackageVersion) { throw 'MSIX manifest version disagrees with shared build metadata.' }
if ($project -match '<(Version|AppxPackageVersion)>') { throw 'The app project must use shared release metadata.' }
if ($ExpectedVersion -and $ExpectedVersion -ne $metadata.Version) { throw 'The release tag differs from the source version.' }
[pscustomobject]@{version=$metadata.Version;msixVersion=$manifest.Version}
