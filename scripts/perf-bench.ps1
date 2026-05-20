# Wind terminal — performance benchmark sampler
#
# Usage (run while Wind / Tauri dev is open):
#   pwsh scripts/perf-bench.ps1 -Label before-p1 -DurationSec 60
#   pwsh scripts/perf-bench.ps1 -Label after-p1  -DurationSec 60
#
# P3 backend comparison:
#   1. Open Settings → set parserBackend = 'wasm'.
#   2. pwsh scripts/perf-bench.ps1 -Label p3-baseline -Backend wasm -DurationSec 30
#   3. Open Settings → set parserBackend = 'rust'.
#   4. pwsh scripts/perf-bench.ps1 -Label p3-rust     -Backend rust -DurationSec 30
#   Compare the summary CPU means; rust mode is expected to drop the
#   JS main-thread share materially since vte parsing moved to Rust.
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
  # P3.14 (2026-05-20) — record which `Settings.parserBackend` was
  # active during the run so before/after comparisons are unambiguous
  # in the summary report. Tag-only: the script does NOT flip the
  # backend (no IPC channel into the running app); the caller is
  # expected to set Settings.parserBackend manually in the UI before
  # starting the sampler. Acceptable values: 'wasm' | 'rust' | 'unknown'.
  [ValidateSet('wasm', 'rust', 'unknown')]
  [string]$Backend = 'unknown',
  # Names of the actual Wind binary to use as the process-tree root.
  # `ridge` is the published debug+release name; `wind` is the in-flight
  # rebrand. Either matches the launcher process; the script then walks
  # ParentProcessId backwards from every running process and keeps only
  # those whose tree root is in this list — so unrelated webview2 / node
  # processes (other Electron apps, dev tools, Claude Code MCP servers)
  # don't pollute the CPU sum.
  [string[]]$RootProcessNames = @('ridge', 'wind'),
  # Substring filter on `Win32_Process.ExecutablePath` for the root match.
  # Defaults to the in-tree `src-tauri\target\` so a developer running
  # the installed v0.0.2 release IN PARALLEL with `pnpm tauri dev` doesn't
  # accidentally include the installed instance in the sample. Pass an
  # empty string to disable (count every ridge/wind regardless of where
  # it lives).
  [string]$RootPathSubstring = 'src-tauri\target\',
  # Explicit PID(s) to exclude from the sample tree even if their name
  # matches. Belt-and-suspenders against `RootPathSubstring` missing an
  # edge case. Mostly useful when both the installed and the dev binary
  # live under similar paths.
  [int[]]$ExcludePids = @(),
  # P3.14.r3 (2026-05-20) — include `msedgewebview2.exe` children of
  # the test ridge.exe OR (under tauri-driver e2e) of `msedgedriver.exe`
  # in the sample. Off by default because the standard ridge.exe owns
  # its WebView2 children directly — so the existing ancestor walk
  # picks them up. The flag matters for the tauri-driver harness,
  # where msedgewebview2.exe is parented to msedgedriver, NOT to
  # ridge.exe, and would otherwise be missed entirely — making the
  # wasm parser path (whose VTE parsing happens in WebView2's JS
  # thread) look ~5-15 percentage points cheaper than reality.
  [switch]$IncludeWebView2,
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
#
# Walk the process tree: find every process whose ancestor chain
# reaches a root in `$RootProcessNames` (default ridge/wind). Excludes
# unrelated `node` / `msedgewebview2` / dev-tool processes that share
# the substring but live under another parent. CIM is preferred over
# `Get-WmiObject` (deprecated, slow on PS 7).
function Get-TargetProcs {
  param(
    [string[]]$rootNames,
    [string]$pathSubstring,
    [int[]]$excludePids,
    [bool]$includeWebView2
  )
  $all = Get-CimInstance Win32_Process -ErrorAction SilentlyContinue
  if (-not $all) { return @() }
  # Index by PID for fast parent lookup.
  $byPid = @{}
  foreach ($p in $all) { $byPid[[int]$p.ProcessId] = $p }
  # Identify roots: any process whose own name (without `.exe`) matches
  # AND whose ExecutablePath contains `pathSubstring` (when set). The
  # path filter is what keeps the installed v0.0.2 release out of a dev
  # sample — its ExecutablePath lives under Program Files / AppData, not
  # under `src-tauri\target\`.
  $excludeSet = @{}
  foreach ($pid_ in $excludePids) { $excludeSet[[int]$pid_] = $true }
  $roots = @{}
  foreach ($p in $all) {
    $bare = ($p.Name -replace '\.exe$','').ToLower()
    $matchedName = $false
    foreach ($r in $rootNames) {
      if ($bare -eq $r.ToLower()) { $matchedName = $true; break }
    }
    if (-not $matchedName) { continue }
    if ($excludeSet.ContainsKey([int]$p.ProcessId)) { continue }
    if ($pathSubstring) {
      $execPath = $p.ExecutablePath
      if (-not $execPath -or ($execPath.IndexOf($pathSubstring, [System.StringComparison]::OrdinalIgnoreCase) -lt 0)) {
        continue
      }
    }
    $roots[[int]$p.ProcessId] = $true
  }
  if ($roots.Count -eq 0) { return @() }
  # For every process, walk up the parent chain (capped) and keep it
  # iff a root sits in the chain. Cap = 20 levels to break any cycle
  # caused by PID reuse mid-walk.
  $keep = @{}
  foreach ($p in $all) {
    $cur = [int]$p.ProcessId
    for ($i = 0; $i -lt 20 -and $cur; $i++) {
      if ($roots.ContainsKey($cur)) { $keep[[int]$p.ProcessId] = $true; break }
      $parent = $byPid[$cur]
      if (-not $parent) { break }
      $next = [int]$parent.ParentProcessId
      if ($next -eq $cur -or $next -eq 0) { break }
      $cur = $next
    }
  }
  # P3.14.r3 — pick up msedgewebview2.exe spawned by the tauri-driver
  # e2e harness. Without this, wasm-mode VTE parsing (which runs on
  # the JS thread inside WebView2) is invisible to the sampler — the
  # rust-vs-wasm CPU comparison then under-counts wasm.
  #
  # Why ancestor walk doesn't work here: WebView2 host processes are
  # NOT parented to the ridge.exe that spawned them. The Windows
  # process tree has them as siblings of explorer.exe (parent points
  # to a short-lived launcher / svchost). Empirically: under
  # tauri-driver, test ridge.exe at PID X had ZERO WebView2 children;
  # the matching WebView2 root was several PIDs higher with parent =
  # explorer (PID 12332 on this box).
  #
  # The reliable disambiguator is the command line. Every WebView2
  # host process embeds two markers:
  #   --webview-exe-name=ridge.exe          (Tauri sets this to its
  #                                          binary's filename)
  #   --user-data-dir="<path>\EBWebView"    (where localStorage lives)
  #
  # Host ridge.exe (the installed v0.0.2) gets:
  #   user-data-dir = %LOCALAPPDATA%\com.tauri-app.ridge\EBWebView
  # Test ridge.exe (spawned by tauri-driver) gets:
  #   user-data-dir = %LOCALAPPDATA%\Temp\scoped_dir*\EBWebView
  # The `scoped_dir` segment is the key — tauri-driver always
  # isolates the test instance into a per-launch temp dir, so this
  # substring is a perfect filter for "WebView2 belonging to a
  # tauri-driver-spawned ridge.exe".
  if ($includeWebView2) {
    foreach ($p in $all) {
      if ($p.Name -ne 'msedgewebview2.exe') { continue }
      if ($keep.ContainsKey([int]$p.ProcessId)) { continue }
      $cl = $p.CommandLine
      if (-not $cl) { continue }
      if ($cl -notmatch 'webview-exe-name=ridge\.exe') { continue }
      # `scoped_dir` is what tauri-driver appends to %TEMP%. Matching
      # on the literal substring (not full path) keeps this robust
      # against Windows TEMP path variations.
      if ($cl -match 'user-data-dir="[^"]*scoped_dir') {
        $keep[[int]$p.ProcessId] = $true
      }
    }
  }
  # Promote to System.Diagnostics.Process so we get CPU + WorkingSet64.
  $result = New-Object System.Collections.Generic.List[object]
  foreach ($pid_ in $keep.Keys) {
    try { $result.Add((Get-Process -Id $pid_ -ErrorAction Stop)) } catch { }
  }
  $result
}

$initialProcs = Get-TargetProcs -rootNames $RootProcessNames -pathSubstring $RootPathSubstring -excludePids $ExcludePids -includeWebView2 $IncludeWebView2.IsPresent
if (-not $initialProcs -or $initialProcs.Count -eq 0) {
  $hint = "No matching root processes found."
  if ($RootPathSubstring) {
    $hint += " Looking for {$($RootProcessNames -join '|')}.exe whose path contains '$RootPathSubstring'."
    $hint += " Start a dev/debug Wind (`pnpm tauri dev`) or pass `-RootPathSubstring ''` to count any ridge/wind."
  }
  Write-Error $hint
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
  $procs = Get-TargetProcs -rootNames $RootProcessNames -pathSubstring $RootPathSubstring -excludePids $ExcludePids -includeWebView2 $IncludeWebView2.IsPresent
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
  backend  : $Backend
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
