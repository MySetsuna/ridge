# REQ-RIDGE-KERNEL-HOST/DOMAIN/MCP smoke (Windows).
# Usage: pwsh -File scripts/kernel-host-smoke.ps1
$ErrorActionPreference = "Continue"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$env:RIDGE_CONFIRM_QUIT_KERNEL = "1"

function Assert-True($cond, $msg) {
  if (-not $cond) { throw "FAIL: $msg" }
  Write-Host "OK: $msg"
}

& cargo build -p ridge-kernel -p ridge-cli | Out-Host
if ($LASTEXITCODE -ne 0) { throw "cargo build failed: $LASTEXITCODE" }
$rdg = Join-Path $root "target\debug\rdg.exe"
$kbin = Join-Path $root "target\debug\ridge-kernel.exe"
Assert-True (Test-Path $rdg) "rdg binary"
Assert-True (Test-Path $kbin) "ridge-kernel binary"

& $rdg kernel stop 2>$null | Out-Null
Start-Sleep -Milliseconds 300

# ensure spawn
$out = (& $rdg kernel ensure | Out-String)
Assert-True ($out -match "kernel ready") "ensure: $out"
$st = (& $rdg kernel status | Out-String)
Assert-True ($st -match "health=ok") "status healthy: $st"

# dual ensure attaches
$out2 = (& $rdg kernel ensure | Out-String)
Assert-True ($out2 -match "kernel ready") "second ensure attach: $out2"

# DOMAIN
$agents = (& $rdg kernel agents | Out-String)
Assert-True ($agents -match "claude|profiles") "domain agents: $agents"
$fs = (& $rdg kernel fs-list $root | Out-String)
Assert-True ($fs -match "ok") "domain fs-list: $fs"

# MCP
$mcp = (& $rdg kernel mcp-smoke | Out-String)
Assert-True ($mcp -match "ridge_kernel_list_agents|tools") "mcp tools/list: $mcp"

# stop
& $rdg kernel stop | Out-Null
Start-Sleep -Milliseconds 400
$st2 = (& $rdg kernel status | Out-String)
Assert-True ($st2 -match "未登记|已退出|残留") "stopped: $st2"

Write-Host "ALL SMOKE PASSED"
exit 0
