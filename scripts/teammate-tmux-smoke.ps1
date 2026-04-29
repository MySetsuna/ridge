#Requires -Version 5.1
<#
.SYNOPSIS
  Quick checks that the tmux shim matches what Claude Code TmuxBackend expects.

  Prerequisites: RIDGE_TEAMMATE_URL and RIDGE_TEAMMATE_TOKEN (Ridge injects these in app PTYs).
  Put the tmux shim on PATH as `tmux` (see pnpm run build:teammate-shim).
#>
$ErrorActionPreference = "Stop"
if (-not $env:RIDGE_TEAMMATE_URL -or -not $env:RIDGE_TEAMMATE_TOKEN) {
    Write-Error "Set RIDGE_TEAMMATE_URL and RIDGE_TEAMMATE_TOKEN (run from Ridge terminal or export manually)."
}
Write-Host "== tmux -V =="
& tmux -V
Write-Host "`n== tmux list-sessions =="
& tmux list-sessions
Write-Host "`n== tmux has-session -t 0 =="
& tmux has-session -t 0
Write-Host "exit $LASTEXITCODE"
Write-Host "`n== tmux list-panes =="
& tmux list-panes
Write-Host "`n== display-message -p window_panes =="
& tmux display-message -p '#{window_panes}'
Write-Host "`n== display-message -pt (cluster) =="
& tmux display-message -pt '%0' '#{pane_id}'
Write-Host "`nDone."
