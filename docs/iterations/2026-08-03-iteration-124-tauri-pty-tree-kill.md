# Iteration 124 - Tauri PTY descendant cleanup

Date: 2026-08-03  
Status: implementation complete; unreleased by the daily publication freeze

## Change

Pane replacement and explicit close/reap now share one `kill_pty_process_tree`
guard. The guard records the child PID, kills the shell handle, then invokes
`ridge_core::process_guard::kill_process_tree` so descendants such as tool
runners and language servers cannot survive a closed Pane. This matches the
Kernel PTY destroy contract and keeps PTY resources bounded.

## Verification

- `cargo test -p ridge --lib commands::terminal --quiet`: 3 passed;
- `cargo test -p ridge-core --lib process_guard --quiet`: 3 passed;
- The source contract proves both local teardown paths use the shared guard;
- No version bump or publication was made because `v0.1.54` consumed today's
  release allowance.

The test suite covers the routing contract; physical Windows process-tree
evidence remains part of the external PTY/WebView2 soak gate.
