# Iteration 154 - Kernel bootstrap single-flight

Date: 2026-08-04
Status: code green for the lifecycle slice; physical restart/reattach remains
open.

## Scope

Remove the desktop startup race in which setup and the first Pane call
`ensure_kernel_running` concurrently. The old path could spawn twice or reject
an already-live kernel while its registry/health endpoint was still becoming
ready.

## Change

- `ensure_kernel_running` now holds one process-local `OnceLock<Mutex<()>>`
  across detection, registry cleanup, spawn, and readiness.
- An `AttachExisting` decision waits up to the existing eight-second bound for
  the same PID's authenticated, protocol-compatible endpoint instead of failing
  immediately during the short health publication window.
- No second kernel is started; existing fail-closed errors remain visible when
  the bounded wait expires or the boot lock is poisoned.

## Verification

- `cargo test -p ridge --lib kernel_lifecycle::tests`: 6 passed.
- The new deterministic test proves concurrent boot callers serialize at the
  shared gate.

## Remaining gates

Run the full Rust/TypeScript matrix, then exercise real desktop setup plus first
Pane startup and exit/restart reattach. Kernel workspace graph rehydrate,
orphan PTY reporting, health-aware death watching, physical/public Remote,
WebView2 heap soak, dual-window/Host, and complete domain authority remain
open. No version bump or release was made.
