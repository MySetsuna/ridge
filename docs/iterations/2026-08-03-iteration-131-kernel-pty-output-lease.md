# Iteration 131 — Kernel PTY bounded output lease

Date: 2026-08-03

## Outcome

The Kernel PTY domain now has a bounded output replay seam for future shell
adapters. Domain PTYs publish monotonic sequence frames into a 256-frame /
256KiB window. A lease can attach from a cursor, long-poll with timeout, report
lag, explicitly resync, detach, and fail closed during pane destruction. The
existing CLI `spawn_with_output` mpsc path remains unchanged.

## Evidence

- `cargo test -p ridge-kernel --lib --quiet`: 27/27 passed.
- `cargo test -p ridge-cli --bin rdg --quiet`: 126/126 passed; CLI output API
  compatibility retained.
- Commit `b304ea7` pushed to `origin/main`.

## Boundary

This is an internal Kernel protocol seam only. HTTP lease routes, stable
composite Pane identity, persistent PTY recovery, Tauri adapter migration, and
the final “Tauri pure shell” claim remain open. No version bump or publication
was made because `v0.1.54` consumed today's release allowance.

