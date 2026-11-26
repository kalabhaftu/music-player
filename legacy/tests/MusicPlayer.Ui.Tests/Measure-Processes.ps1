param([Parameter(Mandatory=$true)][int] $AppProcessId)
$ErrorActionPreference='Stop'
$all=@(Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId)
$ids=[Collections.Generic.HashSet[int]]::new()
$null=$ids.Add($AppProcessId)
do {
    $added=$false
    foreach($entry in $all){if($ids.Contains([int]$entry.ParentProcessId) -and $ids.Add([int]$entry.ProcessId)){$added=$true}}
} while($added)
$processes=@($ids | ForEach-Object {Get-Process -Id $_ -ErrorAction SilentlyContinue})
[pscustomobject]@{
    Utc=[DateTime]::UtcNow.ToString('o')
    ProcessCount=$processes.Count
    WorkingSetBytes=($processes | Measure-Object WorkingSet64 -Sum).Sum
    PrivateBytes=($processes | Measure-Object PrivateMemorySize64 -Sum).Sum
    CpuSeconds=($processes | Measure-Object CPU -Sum).Sum
} | ConvertTo-Json -Compress
