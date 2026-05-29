# P3.14.r4 frame-time orchestrator — JS main thread responsiveness
# under heavy PTY traffic, rust vs wasm.
#
# The CPU sampler (perf-compare.ps1) shows rust mode costs ~3% MORE
# total CPU than wasm — that's the cost of the postcard-encoded delta
# pipeline. This script measures what rust mode is supposed to WIN
# on: keeping the JS main thread free.
#
# Each round:
#   1. Spawns wdio with wdio.frame.conf.ts → frame-time.spec.ts
#   2. Spec drives 25 s of PowerShell stress at the PTY
#   3. Spec samples rAF intervals from inside the webview
#   4. Spec writes p3-frame-{backend}-{ts}.json to scripts/perf-runs/
# After both rounds: print both JSONs side by side.
#
# Usage:
#   pwsh scripts/perf-frame-compare.ps1                 # default 25 s
#   pwsh scripts/perf-frame-compare.ps1 -StressSec 40
#
# Unlike perf-compare.ps1, no external sampler is needed — the
# measurement instrument (rAF) runs inside the same main thread we
# want to characterize.

[CmdletBinding()]
param(
  [int]$StressSec = 25
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot

function Stop-LeakedDrivers {
  param([string]$Phase)
  $leaked = Get-CimInstance Win32_Process -Filter "Name='tauri-driver.exe' OR Name='msedgedriver.exe'" -ErrorAction SilentlyContinue
  if ($leaked) {
    Write-Host "[$Phase] Killing leaked drivers: $($leaked.Name -join ', ') (PIDs $($leaked.ProcessId -join ','))"
    foreach ($p in $leaked) {
      Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue
    }
    Start-Sleep -Milliseconds 400
  }
}

function Invoke-FrameRound {
  param([Parameter(Mandatory)] [string]$Backend, [int]$StressSec)

  Stop-LeakedDrivers -Phase "pre-$Backend"
  Write-Host ""
  Write-Host "=============================================="
  Write-Host "  Frame-time round: $Backend"
  Write-Host "  Stress window   : ${StressSec}s"
  Write-Host "=============================================="
  Write-Host ""

  # Sequential — no background job — since the sampler is in-process
  # (rAF inside the webview). The spec self-contains its own stress
  # phase and stop signal.
  $env:RIDGE_PERF_BACKEND = $Backend
  $env:RIDGE_PERF_STRESS_SEC = "$StressSec"
  $env:NO_PROXY = "localhost,127.0.0.1,::1"
  & pnpm exec wdio run wdio.frame.conf.ts 2>&1 | Out-Host

  if ($LASTEXITCODE -ne 0) {
    Write-Warning "[$Backend] wdio exited with code $LASTEXITCODE"
    return $false
  }
  return $true
}

try {
  $rustOk = Invoke-FrameRound -Backend 'rust' -StressSec $StressSec
  $wasmOk = Invoke-FrameRound -Backend 'wasm' -StressSec $StressSec

  Write-Host ""
  Write-Host "=============================================="
  Write-Host "  Frame-time results"
  Write-Host "=============================================="
  Write-Host ""

  $rustJson = Get-ChildItem "$PSScriptRoot/perf-runs/p3-frame-rust-*.json" -ErrorAction SilentlyContinue |
              Sort-Object LastWriteTime -Descending | Select-Object -First 1
  $wasmJson = Get-ChildItem "$PSScriptRoot/perf-runs/p3-frame-wasm-*.json" -ErrorAction SilentlyContinue |
              Sort-Object LastWriteTime -Descending | Select-Object -First 1

  if (-not $rustJson -or -not $wasmJson) {
    Write-Warning "Missing frame-time JSON for one or both backends — runs may have failed."
    if ($rustJson) { Write-Host "--- rust ($($rustJson.Name)) ---"; Get-Content $rustJson.FullName }
    if ($wasmJson) { Write-Host "--- wasm ($($wasmJson.Name)) ---"; Get-Content $wasmJson.FullName }
    exit 1
  }

  $rust = Get-Content $rustJson.FullName | ConvertFrom-Json
  $wasm = Get-Content $wasmJson.FullName | ConvertFrom-Json

  Write-Host ("{0,-12} {1,12} {2,12}" -f "metric", "rust", "wasm")
  Write-Host ("{0,-12} {1,12} {2,12}" -f "------", "----", "----")
  Write-Host ("{0,-12} {1,12} {2,12}" -f "frames",   $rust.frames,    $wasm.frames)
  Write-Host ("{0,-12} {1,12} {2,12}" -f "fps",      $rust.fps,       $wasm.fps)
  Write-Host ("{0,-12} {1,12} {2,12}" -f "meanMs",   $rust.meanMs,    $wasm.meanMs)
  Write-Host ("{0,-12} {1,12} {2,12}" -f "p50Ms",    $rust.p50Ms,     $wasm.p50Ms)
  Write-Host ("{0,-12} {1,12} {2,12}" -f "p95Ms",    $rust.p95Ms,     $wasm.p95Ms)
  Write-Host ("{0,-12} {1,12} {2,12}" -f "p99Ms",    $rust.p99Ms,     $wasm.p99Ms)
  Write-Host ("{0,-12} {1,12} {2,12}" -f "maxMs",    $rust.maxMs,     $wasm.maxMs)
  Write-Host ("{0,-12} {1,12} {2,12}" -f "jank33",   $rust.jank33,    $wasm.jank33)
  Write-Host ("{0,-12} {1,12} {2,12}" -f "jank50",   $rust.jank50,    $wasm.jank50)
  Write-Host ("{0,-12} {1,12} {2,12}" -f "jank100",  $rust.jank100,   $wasm.jank100)
  Write-Host ""
  Write-Host "Notes:"
  Write-Host "  - p50/p95/p99/max are rAF interval percentiles (ms)."
  Write-Host "  - jank{N} = frames whose interval exceeded N ms"
  Write-Host "    (>33ms = 2 missed vsyncs; >50ms = perceptible stutter;"
  Write-Host "     >100ms = 'UI froze' for an observer)."
  Write-Host "  - Lower is better across the board. The expected story:"
  Write-Host "    rust keeps p95/p99 near 16-17ms with zero jank100;"
  Write-Host "    wasm spikes p95+ and accumulates jank as kernel.feed"
  Write-Host "    blocks the main thread on each PTY burst."
} finally {
  Stop-LeakedDrivers -Phase 'final-cleanup'
  Pop-Location
}
