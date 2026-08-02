# Iteration 97 — Git descendant process reclamation

Date: 2026-08-03  
Status: code closed; release gate pending

## Scope

Close the remaining external-process lifecycle gap for Git timeout and
latest-win cancellation on Unix. Logical semaphore limits alone are not enough
if a shell/helper descendant survives the parent process kill.

## Root cause

The shared process guard already kills a dedicated Unix process group, but the
Git command path spawned its child without first creating that group. A Git
wrapper, credential helper, or shell descendant could therefore outlive the
parent and retain CPU, locks, or pipes after timeout/cancellation.

## Implementation

- `packages/ridge-core/src/process_guard.rs` exposes the existing process-group
  setup to sibling core modules.
- `packages/ridge-core/src/commands/git.rs` now applies that setup before every
  guarded Git spawn, preserving Windows `taskkill /T` behavior and Unix group
  TERM/KILL behavior.
- Added a Unix path-level descendant timeout regression (the Windows shared
  process-tree guard remains covered by its existing hanging-binary test).

## Verification

- `cargo test -p ridge-core commands::git --lib --quiet`: 39/39
- `cargo test -p ridge-core process_guard --lib --quiet`: 3/3
- `git diff --check`: passed

## Residual gates

Physical phone attribution, WebView2 heap soak, public WebRTC, dual-window
workspace singleton, authenticated Git push, and full Kernel-domain migration
remain separately tracked; this slice only closes Git child-tree reclamation.
