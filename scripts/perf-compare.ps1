# P3.14 perf orchestrator — automated rust vs wasm backend comparison.
#
# Runs the wdio perf spec (tests/e2e-perf/stress.spec.ts) twice — once
# with RIDGE_PERF_BACKEND=rust, once with =wasm — and runs perf-bench.ps1
# in parallel during each spec's stress window. Each round produces a
# CSV + summary in scripts/perf-runs/.
#
# Usage:
#   pwsh scripts/perf-compare.ps1                    # default 25 s sample
#   pwsh scripts/perf-compare.ps1 -SampleSec 40
#
# Host safety:
#   - perf-bench.ps1's default RootPathSubstring filter ('src-tauri\target\')
#     ensures only the TEST ridge.exe spawned by tauri-driver is sampled,
#     never the installed host at C:\Program Files\ridge\.
#   - Leaked tauri-driver / msedgedriver processes are killed by PID
#     before each round so port 4444 is free.

[CmdletBinding()]
param(
  [int]$SampleSec = 25,
  # The spec sleeps a bit longer than the sampler so perf-bench finishes
  # cleanly before the spec exits. 10 s buffer covers spec startup +
  # backend flip + reload + initial PTY write before steady state.
  [int]$StressSecPad = 15,
  # Brief delay between sample start and spec startup-to-steady-state.
  # The spec needs to:
  #   - waitForAppReady (~3 s)
  #   - optionally flip backend + refresh + wait again (~5 s)
  #   - write the stress command + 500 ms settle
  # 12 s covers the worst case on a warm box; a cold start may need more.
  [int]$StartupSec = 12
)

$ErrorActionPreference = 'Stop'

# Resolve project root regardless of where pwsh is invoked from.
$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot

function Stop-LeakedDrivers {
  param([string]$Phase)
  # Kill any tauri-driver / msedgedriver from previous runs so port 4444
  # is free. Crucially does NOT touch ridge.exe — those are picked up by
  # name only in the test path subdirectory (handled by perf-bench's
  # filter on RootPathSubstring).
  $leaked = Get-CimInstance Win32_Process -Filter "Name='tauri-driver.exe' OR Name='msedgedriver.exe'" -ErrorAction SilentlyContinue
  if ($leaked) {
    Write-Host "[$Phase] Killing leaked drivers: $($leaked.Name -join ', ') (PIDs $($leaked.ProcessId -join ','))"
    foreach ($p in $leaked) {
      Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue
    }
    Start-Sleep -Milliseconds 400
  }
}

function Invoke-PerfRound {
  param(
    [Parameter(Mandatory)] [string]$Backend,
    [int]$SampleSec,
    [int]$StartupSec,
    [int]$StressSecPad
  )

  Stop-LeakedDrivers -Phase "pre-$Backend"

  $stressSec = $SampleSec + $StressSecPad
  Write-Host ""
  Write-Host "=============================================="
  Write-Host "  Round: $Backend"
  Write-Host "  Sample window : ${SampleSec}s"
  Write-Host "  Spec sleep    : ${stressSec}s"
  Write-Host "  Startup grace : ${StartupSec}s"
  Write-Host "=============================================="
  Write-Host ""

  # Spawn wdio as a background job. The job inherits env vars from the
  # parent at job-creation time, so set them just before Start-Job.
  $env:RIDGE_PERF_BACKEND = $Backend
  $env:RIDGE_PERF_STRESS_SEC = "$stressSec"

  $wdioJob = Start-Job -ScriptBlock {
    param($root, $backend, $stress)
    Set-Location $root
    $env:RIDGE_PERF_BACKEND = $backend
    $env:RIDGE_PERF_STRESS_SEC = $stress
    # NO_PROXY is set inside wdio.conf.ts already — repeating here costs
    # nothing and guards against subshell env scoping surprises.
    $env:NO_PROXY = "localhost,127.0.0.1,::1"
    & pnpm exec wdio run wdio.perf.conf.ts 2>&1
  } -ArgumentList $repoRoot, $Backend, "$stressSec"

  Write-Host "[$Backend] wdio job started (id=$($wdioJob.Id))"

  # Poll for the test ridge.exe to appear (tauri-driver spawns it as a
  # child of msedgedriver). The presence of a ridge.exe whose path
  # contains src-tauri\target\release is the signal that the spec's
  # `before` hook is running.
  Write-Host "[$Backend] Waiting up to 45s for test ridge.exe to spawn..."
  $deadline = (Get-Date).AddSeconds(45)
  $testRidgeUp = $false
  while ((Get-Date) -lt $deadline) {
    $r = Get-CimInstance Win32_Process -Filter "Name='ridge.exe'" -ErrorAction SilentlyContinue |
         Where-Object { $_.ExecutablePath -and $_.ExecutablePath.IndexOf('src-tauri\target\release', [System.StringComparison]::OrdinalIgnoreCase) -ge 0 }
    if ($r) {
      Write-Host "[$Backend] Test ridge.exe up (PID $($r.ProcessId -join ','))"
      $testRidgeUp = $true
      break
    }
    Start-Sleep -Milliseconds 500
  }
  if (-not $testRidgeUp) {
    Write-Warning "[$Backend] Test ridge.exe never appeared — wdio output:"
    Receive-Job -Job $wdioJob -Keep | Select-Object -Last 40
    Stop-Job -Job $wdioJob -ErrorAction SilentlyContinue
    Remove-Job -Job $wdioJob -Force -ErrorAction SilentlyContinue
    return $false
  }

  Write-Host "[$Backend] Waiting ${StartupSec}s for spec to reach stress phase..."
  Start-Sleep -Seconds $StartupSec

  # Sample. The script's default RootPathSubstring filter keeps the
  # installed host at C:\Program Files\ridge\ out of the sample.
  Write-Host "[$Backend] Sampling for ${SampleSec}s..."
  & "$PSScriptRoot/perf-bench.ps1" -Label "p3-$Backend" -Backend $Backend -DurationSec $SampleSec -IntervalSec 1

  Write-Host "[$Backend] Waiting for wdio job to finish (up to 60s)..."
  $finished = Wait-Job -Job $wdioJob -Timeout 60
  if (-not $finished) {
    Write-Warning "[$Backend] wdio job did not finish in time — last 30 lines:"
    Receive-Job -Job $wdioJob -Keep | Select-Object -Last 30
    Stop-Job -Job $wdioJob -ErrorAction SilentlyContinue
  } else {
    Write-Host "[$Backend] wdio job finished — tail:"
    Receive-Job -Job $wdioJob -Keep | Select-Object -Last 15
  }
  Remove-Job -Job $wdioJob -Force -ErrorAction SilentlyContinue
  return $true
}

try {
  $rustOk = Invoke-PerfRound -Backend 'rust' -SampleSec $SampleSec -StartupSec $StartupSec -StressSecPad $StressSecPad
  $wasmOk = Invoke-PerfRound -Backend 'wasm' -SampleSec $SampleSec -StartupSec $StartupSec -StressSecPad $StressSecPad

  Write-Host ""
  Write-Host "=============================================="
  Write-Host "  Results"
  Write-Host "=============================================="
  Write-Host ""

  $summaries = Get-ChildItem "$PSScriptRoot/perf-runs/p3-*.summary.txt" -ErrorAction SilentlyContinue |
               Sort-Object LastWriteTime -Descending | Select-Object -First 2
  if (-not $summaries) {
    Write-Warning "No summary files produced — both rounds likely failed."
    exit 1
  }
  foreach ($s in $summaries) {
    Write-Host "--- $($s.Name) ---"
    Get-Content $s.FullName | ForEach-Object { Write-Host $_ }
    Write-Host ""
  }
} finally {
  Stop-LeakedDrivers -Phase 'final-cleanup'
  Pop-Location
}
