# Iteration 152 — Kernel-owned desktop PTY authority

Date: 2026-08-04
Status: code green; physical and cross-host gates remain open.

## Scope

Close the desktop PTY authority gap from the approved kernel convergence
requirements. Desktop pane creation must use the singleton `ridge-kernel`
process and must not silently create a second Tauri-owned child when bootstrap,
list, create, attach, or install fails.

## Change

- `kernel_endpoint_for_shell` now returns a typed failure instead of `Option`.
- Ordinary and structured Agent pane creation fail closed with an actionable
  `AppError` when kernel bootstrap or PTY RPC fails.
- A concurrent install (`Ok(false)`) is treated as an idempotent success; no
  duplicate local spawn is attempted.
- The legacy `initial_command` path is rejected explicitly because it has no
  structured argv/env representation. Callers must use
  `StructuredPtyCommand`.
- The native pending-spawn implementation remains only as a `cfg(test)` seam;
  production returns before it can create a local PTY.

## Verification

- `cargo check -p ridge --lib`: exit 0 (warnings pre-existing).
- `cargo test -p ridge --lib pty_lifecycle_contract_tests`: 2 passed.
- `cargo test -p ridge --lib`: 253 passed, 0 failed.
- `git diff --check`: passed.

## Remaining gates

Physical desktop exit/restart and rdg deep-root reattach, public/physical
Remote, WebView2 heap soak, dual-window/dual-Host, and complete Kernel domain
authority evidence remain external gates. No version bump or release was made.
