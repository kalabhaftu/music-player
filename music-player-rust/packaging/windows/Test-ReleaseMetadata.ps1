param([string] $ExpectedVersion)
$ErrorActionPreference = 'Stop'

# Find repo root and rust crate root
$scriptDir = $PSScriptRoot
$current = $scriptDir
$repoRoot = $null
$rustRoot = $null
while ($current) {
    if (Test-Path (Join-Path $current 'music-player-rust') -PathType Container) {
        $repoRoot = $current
        $rustRoot = Join-Path $current 'music-player-rust'
        break
    }
    if (Test-Path (Join-Path $current 'crates\app') -PathType Container) {
        $rustRoot = $current
        $repoRoot = Split-Path -Parent $current
        break
    }
    $parent = Split-Path -Parent $current
    if ($parent -eq $current) { break }
    $current = $parent
}
if (-not $repoRoot) {
    $repoRoot = (Resolve-Path (Join-Path $scriptDir '..\..\..')).Path
    $rustRoot = Join-Path $repoRoot 'music-player-rust'
}

$cargoFile = Join-Path $rustRoot 'crates\app\Cargo.toml'
$propsFile = Join-Path $repoRoot 'Directory.Build.props'

$cargoContent = Get-Content -LiteralPath $cargoFile -Raw
$cargoMatch = [regex]::Match($cargoContent, '(?m)^version\s*=\s*"([^"]+)"')
$cargoVersion = if ($cargoMatch.Success) { $cargoMatch.Groups[1].Value } else { "unknown" }

$propsVersion = $null
if (Test-Path -LiteralPath $propsFile) {
    $metadata = ([xml](Get-Content -LiteralPath $propsFile -Raw)).Project.PropertyGroup
    $propsVersion = $metadata.Version
}

Write-Host "Crate version (Cargo.toml): $cargoVersion"
if ($propsVersion) {
    Write-Host "Build props version: $propsVersion"
}
if ($ExpectedVersion -and $ExpectedVersion -ne $cargoVersion -and $ExpectedVersion -ne $propsVersion) {
    throw "The expected version ($ExpectedVersion) differs from source metadata ($cargoVersion / $propsVersion)."
}

[pscustomobject]@{
    cargoVersion = $cargoVersion
    propsVersion = $propsVersion
}
