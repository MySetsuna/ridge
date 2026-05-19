# Wind terminal — performance benchmark sampler
#
# Usage (run while Wind / Tauri dev is open):
#   pwsh scripts/perf-bench.ps1 -Label before-p1 -DurationSec 60
#   pwsh scripts/perf-bench.ps1 -Label after-p1  -DurationSec 60
#
# Samples CPU% and RSS of Wind-related processes once per `IntervalSec`,
# reports mean / p50 / p95 / max over the run, and writes a CSV + summary
# into `scripts/perf-runs/` (gitignored). Pair before/after labels to
# quantify each PR's impact.
#
# Notes
# - CPU% is whole-machine (process CPU / interval / logical cores * 100),
#   so 1.5% means the combined Wind tree is using 1.5% of the box.
# - "wind", "ridge", "msedgewebview2" cover Tauri (debug + release naming)
#   and the WebView2 children; "node" picks up `pnpm tauri dev`'s vite.

[CmdletBinding()]
param(
  [int]$DurationSec = 60,
  [int]$IntervalSec = 1,
  [string]$Label = 'idle',
  [string[]]$ProcessFilter = @('wind', 'ridge', 'msedgewebview2', 'node'),
  [string]$OutputDir = $null
)

$ErrorActionPreference = 'Stop'

# --- output paths ---
if (-not $OutputDir) {
  $scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
  $OutputDir = Join-Path $scriptRoot 'perf-runs'
}
if (-not (Test-Path $OutputDir)) {
  New-Item -ItemType Directory -Path $OutputDir | Out-Null
}
$timestamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$csvPath = Join-Path $OutputDir ("$Label-$timestamp.csv")
$summaryPath = Join-Path $OutputDir ("$Label-$timestamp.summary.txt")

# --- discover target processes ---
function Get-TargetProcs {
  param([string[]]$filter)
  Get-Process | Where-Object {
    $name = $_.ProcessName.ToLower()
    $hit = $false
    foreach ($f in $filter) { if ($name -like "*$f*") { $hit = $true; break } }
    $hit
  }
}

$initialProcs = Get-TargetProcs -filter $ProcessFilter
if (-not $initialProcs -or $initialProcs.Count -eq 0) {
  Write-Error "No target processes found. Start Wind first (pnpm tauri dev or installed app)."
  exit 1
}

$cores = [Environment]::ProcessorCount

Write-Host ""
Write-Host "perf-bench: tracking $($initialProcs.Count) process(es) across $cores logical cores"
$initialProcs | ForEach-Object {
  Write-Host ("  PID {0,-6}  {1}" -f $_.Id, $_.ProcessName)
}
Write-Host "Sampling every ${IntervalSec}s for ${DurationSec}s -> $csvPath"
Write-Host ""

# --- sample loop ---
$samples = New-Object System.Collections.Generic.List[object]
$prev = @{}
foreach ($p in $initialProcs) {
  $prev[$p.Id] = @{ cpu = $p.CPU; ts = (Get-Date) }
}

Start-Sleep -Seconds 1  # warm-up: first interval gets a real delta

$endTs = (Get-Date).AddSeconds($DurationSec)
$tick = 0
$totalSamples = [math]::Floor($DurationSec / $IntervalSec)

while ((Get-Date) -lt $endTs) {
  Start-Sleep -Seconds $IntervalSec
  $tick++
  $now = Get-Date
  $procs = Get-TargetProcs -filter $ProcessFilter
  $cpuSum = 0.0
  $memSum = 0.0
  foreach ($p in $procs) {
    if (-not $prev.ContainsKey($p.Id)) {
      $prev[$p.Id] = @{ cpu = $p.CPU; ts = $now }
      continue
    }
    $dCpu = $p.CPU - $prev[$p.Id].cpu
    $dT = ($now - $prev[$p.Id].ts).TotalSeconds
    if ($dT -gt 0) {
      $cpuSum += ($dCpu / $dT) / $cores * 100.0
    }
    $memSum += $p.WorkingSet64 / 1MB
    $prev[$p.Id] = @{ cpu = $p.CPU; ts = $now }
  }
  $samples.Add([pscustomobject]@{
    tick          = $tick
    timestamp     = $now.ToString('o')
    cpu_pct_total = [math]::Round($cpuSum, 2)
    rss_mb_total  = [math]::Round($memSum, 1)
    proc_count    = $procs.Count
  })
  Write-Host ("[{0,3}/{1}]  CPU={2,6:N2}%   RSS={3,8:N1} MB   procs={4}" -f $tick, $totalSamples, $cpuSum, $memSum, $procs.Count)
}

# --- export + summary ---
$samples | Export-Csv -Path $csvPath -NoTypeInformation -Encoding UTF8

$cpus = @($samples | ForEach-Object { $_.cpu_pct_total })
$sorted = $cpus | Sort-Object
$count = $sorted.Count
if ($count -lt 1) {
  Write-Error "No samples recorded — aborting."
  exit 1
}

$mean = ($cpus | Measure-Object -Average).Average
$p50idx = [math]::Min($count - 1, [int]($count * 0.5))
$p95idx = [math]::Min($count - 1, [int]($count * 0.95))
$p50 = $sorted[$p50idx]
$p95 = $sorted[$p95idx]
$max = ($cpus | Measure-Object -Maximum).Maximum

$summary = @"
perf-bench label='$Label'
  duration : ${DurationSec}s
  interval : ${IntervalSec}s
  cores    : $cores
  samples  : $count
  processes: $($initialProcs.ProcessName -join ', ')

CPU% (whole-machine, all tracked procs summed)
  mean : $([math]::Round($mean, 2))
  p50  : $([math]::Round($p50, 2))
  p95  : $([math]::Round($p95, 2))
  max  : $([math]::Round($max, 2))
"@

$summary | Out-File -FilePath $summaryPath -Encoding UTF8
Write-Host ""
Write-Host $summary
Write-Host ""
Write-Host "CSV     : $csvPath"
Write-Host "Summary : $summaryPath"
