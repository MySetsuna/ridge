#Requires -Version 5.1
<#
.SYNOPSIS
  Quick checks that wind-tmux matches what Claude Code TmuxBackend expects.

  Prerequisites: WIND_TEAMMATE_URL and WIND_TEAMMATE_TOKEN (Wind injects these in app PTYs).
  Put wind-tmux on PATH as `tmux` (see pnpm run build:teammate-shim).
#>
$ErrorActionPreference = "Stop"
if (-not $env:WIND_TEAMMATE_URL -or -not $env:WIND_TEAMMATE_TOKEN) {
    Write-Error "Set WIND_TEAMMATE_URL and WIND_TEAMMATE_TOKEN (run from Wind terminal or export manually)."
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
