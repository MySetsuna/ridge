#!/usr/bin/env bash
# Quick checks that wind-tmux matches what Claude Code TmuxBackend expects.
# Prerequisites: WIND_TEAMMATE_URL and WIND_TEAMMATE_TOKEN; tmux on PATH = wind-tmux.
set -euo pipefail
: "${WIND_TEAMMATE_URL:?set WIND_TEAMMATE_URL}"
: "${WIND_TEAMMATE_TOKEN:?set WIND_TEAMMATE_TOKEN}"
echo "== tmux -V =="
tmux -V
echo ""
echo "== tmux list-sessions =="
tmux list-sessions
echo ""
echo "== tmux has-session -t 0 =="
tmux has-session -t 0
echo "exit $?"
echo ""
echo "== tmux list-panes =="
tmux list-panes
echo ""
echo "== display-message -p window_panes =="
tmux display-message -p '#{window_panes}'
echo ""
echo "== display-message -pt (cluster) =="
tmux display-message -pt '%0' '#{pane_id}'
echo ""
echo "Done."
