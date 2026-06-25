#Requires -Version 5.1
<#
.SYNOPSIS
  teammate HTTP 后端「楔死」探针(option 3 诊断脚手架)。

  背景:teammate HTTP server 跑在单线程 Tokio 运行时——某个 handler 一旦阻塞,后续所有
  请求排不上,各自卡满垫片超时(实测 split + list-sessions 各卡 60s → "Failed to create
  teammate pane")。本脚本反复发一条**只读、必经后端**的 tmux 命令并计时,任何明显变慢
  (≥ 阈值)即楔死前兆/已楔死的信号。

  命中后去看后端日志 ridge.log(<app_data_dir>\logs\ridge-YYYY-MM-DD.log),搜 "diag":
  那条**只有 `diag >>` 没有对应 `diag <<`** 的请求就是卡住的 handler;若是 split-window,
  再看 "route_split:" 的 checkpoint①②③ 看卡在哪一步。

  前置:RIDGE_TEAMMATE_URL / RIDGE_TEAMMATE_TOKEN(Ridge 在 app PTY 内自动注入);
  tmux 垫片在 PATH 上叫 `tmux`(pnpm run build:teammate-shim)。

.PARAMETER Count      探测次数(默认 30)
.PARAMETER DelayMs    每次间隔毫秒(默认 500)
.PARAMETER ThreshMs   判为「慢」的阈值毫秒(默认 4000,后端健康路径远低于此)
#>
param(
    [int]$Count = 30,
    [int]$DelayMs = 500,
    [int]$ThreshMs = 4000
)
$ErrorActionPreference = "Stop"
if (-not $env:RIDGE_TEAMMATE_URL -or -not $env:RIDGE_TEAMMATE_TOKEN) {
    Write-Error "Set RIDGE_TEAMMATE_URL and RIDGE_TEAMMATE_TOKEN (run from Ridge terminal)."
}
Write-Host "wedge-probe: $Count 次 read-only 后端往返,阈值 ${ThreshMs}ms" -ForegroundColor Cyan
$slow = 0
for ($i = 1; $i -le $Count; $i++) {
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    # display-message '#{window_panes}' 需动态变量 → 必走后端 /api/v1/list-panes 往返(只读)。
    & tmux display-message -p '#{window_panes}' | Out-Null
    $code = $LASTEXITCODE
    $sw.Stop()
    $ms = [int]$sw.Elapsed.TotalMilliseconds
    $flag = ""
    if ($code -ne 0) { $flag = " EXIT=$code"; $slow++ }
    elseif ($ms -ge $ThreshMs) { $flag = " <<< SLOW"; $slow++ }
    $color = if ($flag) { "Red" } else { "DarkGray" }
    Write-Host ("#{0,-3} {1,6}ms{2}" -f $i, $ms, $flag) -ForegroundColor $color
    Start-Sleep -Milliseconds $DelayMs
}
if ($slow -gt 0) {
    Write-Host "`n命中 $slow 次慢/失败 → 后端疑似楔死。去 ridge.log 搜 'diag':找只有 '>>' 没 '<<' 的请求。" -ForegroundColor Yellow
} else {
    Write-Host "`n全部正常,无楔死迹象。" -ForegroundColor Green
}
