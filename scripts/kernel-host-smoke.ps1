# REQ-RIDGE-KERNEL-HOST/DOMAIN/MCP smoke (Windows).
# Usage: pwsh -File scripts/kernel-host-smoke.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$env:RIDGE_CONFIRM_QUIT_KERNEL = "1"
# Exercise the same root policy a headless/remote host can grant to the
# kernel. Unset/empty keeps desktop compatibility; this smoke opts in.
$env:RIDGE_KERNEL_FS_ROOT = $root

# Every external process in this smoke must have a wall-clock bound and a
# process-tree cleanup path. A timed-out `rdg kernel ensure` otherwise leaves a
# detached kernel behind and the next run can attach to stale state.
function Invoke-Checked {
  param(
    [Parameter(Mandatory = $true)][string]$FilePath,
    [Parameter(Mandatory = $false)][string[]]$ArgumentList = @(),
    [int]$TimeoutMs = 120000
  )
  $outPath = [IO.Path]::GetTempFileName()
  $errPath = [IO.Path]::GetTempFileName()
  $proc = $null
  try {
    $proc = Start-Process -FilePath $FilePath -ArgumentList $ArgumentList -NoNewWindow -PassThru `
      -RedirectStandardOutput $outPath -RedirectStandardError $errPath
    if (-not $proc.WaitForExit($TimeoutMs)) {
      # Windows smoke only: kill the exact spawned PID and its descendants.
      & taskkill.exe /PID $proc.Id /T /F 2>$null | Out-Null
      throw "command timed out after ${TimeoutMs}ms: $FilePath $($ArgumentList -join ' ')"
    }
    $proc.WaitForExit()
    $proc.Refresh()
    $exitCode = [int]$proc.ExitCode
    $stdout = if (Test-Path $outPath) { Get-Content -Raw -Encoding utf8 $outPath } else { '' }
    $stderr = if (Test-Path $errPath) { Get-Content -Raw -Encoding utf8 $errPath } else { '' }
    if ($exitCode -ne 0) {
      $detail = ("$stderr $stdout").Trim()
      throw "command failed ($exitCode): $FilePath $($ArgumentList -join ' ') $detail"
    }
    return ("$stdout$stderr").Trim()
  } finally {
    if ($proc -and -not $proc.HasExited) {
      & taskkill.exe /PID $proc.Id /T /F 2>$null | Out-Null
    }
    Remove-Item -LiteralPath $outPath,$errPath -Force -ErrorAction SilentlyContinue
  }
}

function Assert-True($cond, $msg) {
  if (-not $cond) { throw "FAIL: $msg" }
  Write-Host "OK: $msg"
}

$cargo = Get-Command cargo.exe -ErrorAction Stop
Invoke-Checked -FilePath $cargo.Source -ArgumentList @('build', '-p', 'ridge-kernel', '-p', 'ridge-cli') -TimeoutMs 600000 | Out-Host
$rdg = Join-Path $root "target\debug\rdg.exe"
$kbin = Join-Path $root "target\debug\ridge-kernel.exe"
Assert-True (Test-Path $rdg) "rdg binary"
Assert-True (Test-Path $kbin) "ridge-kernel binary"

try { Invoke-Checked -FilePath $rdg -ArgumentList @('kernel', 'stop') -TimeoutMs 10000 | Out-Null } catch { }
Start-Sleep -Milliseconds 300

try {
  # ensure spawn
  $out = Invoke-Checked -FilePath $rdg -ArgumentList @('kernel', 'ensure') -TimeoutMs 20000
  Assert-True ($out -match "kernel ready") "ensure: $out"
  $st = Invoke-Checked -FilePath $rdg -ArgumentList @('kernel', 'status') -TimeoutMs 10000
  Assert-True ($st -match "health=ok") "status healthy: $st"

  # dual ensure attaches
  $out2 = Invoke-Checked -FilePath $rdg -ArgumentList @('kernel', 'ensure') -TimeoutMs 20000
  Assert-True ($out2 -match "kernel ready") "second ensure attach: $out2"

  # DOMAIN
  $agents = Invoke-Checked -FilePath $rdg -ArgumentList @('kernel', 'agents') -TimeoutMs 10000
  Assert-True ($agents -match "claude|profiles") "domain agents: $agents"
  $fs = Invoke-Checked -FilePath $rdg -ArgumentList @('kernel', 'fs-list', $root) -TimeoutMs 10000
  Assert-True ($fs -match "ok") "domain fs-list: $fs"
  $outside = Join-Path (Split-Path -Parent $root) ("ridge-kernel-fs-outside-$PID")
  $blocked = Invoke-Checked -FilePath $rdg -ArgumentList @('kernel', 'fs-list', $outside) -TimeoutMs 10000
  Assert-True ($blocked -match '"ok"\s*:\s*false' -and $blocked -match 'outside kernel filesystem root') "fs root blocks outside path: $blocked"
  $git = Invoke-Checked -FilePath $rdg -ArgumentList @('kernel', 'git-status', $root) -TimeoutMs 30000
  Assert-True ($git -match "ridge-kernel|is_repo|status") "domain git-status: $git"
  $hosts = Invoke-Checked -FilePath $rdg -ArgumentList @('kernel', 'remote-hosts') -TimeoutMs 10000
  Assert-True ($hosts -match '"source"\s*:\s*"ridge-kernel"' -and $hosts -match '"hosts"\s*:') "domain remote-hosts: $hosts"

  # MCP
  $mcp = Invoke-Checked -FilePath $rdg -ArgumentList @('kernel', 'mcp-smoke') -TimeoutMs 10000
  Assert-True ($mcp -match "ridge_kernel_list_agents|tools") "mcp tools/list: $mcp"

  Write-Host "ALL SMOKE PASSED"
} finally {
  # Cleanup is mandatory even when an assertion or timeout fails.
  try { Invoke-Checked -FilePath $rdg -ArgumentList @('kernel', 'stop') -TimeoutMs 15000 | Out-Null } catch { }
}
exit 0
