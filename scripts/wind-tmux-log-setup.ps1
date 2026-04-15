#Requires -Version 5.1
<#
.SYNOPSIS
  Point wind-tmux file logging at a fixed path (e.g. C:\temp\wind-tmux-full.log) before starting Claude Code.

  Wind injects WIND_TMUX_LOG into PTYs; for a manual shell you must set it yourself.
  After running Claude, open the log and search for "split-window" / "post_split".
#>
$ErrorActionPreference = "Stop"
$LogPath = if ($args.Count -ge 1 -and $args[0].Trim()) { $args[0].Trim() } else { "C:\temp\wind-tmux-full.log" }

$env:WIND_TMUX_LOG = $LogPath
Remove-Item $LogPath -ErrorAction SilentlyContinue
Write-Host "WIND_TMUX_LOG=$LogPath"
Write-Host "Start Claude Code from this PowerShell session (or ensure child inherits env)."
Write-Host "Tip: run from a non-home cwd so TMUX socket path matches your repo (see terminal.rs)."
