# Iteration 105 — Kernel-authoritative host attach gate

Date: 2026-08-03
Status: implementation complete; release deferred by the daily publication freeze

## Root cause

`attach_host_session` validated connectivity against the desktop
`HostRegistry` projection. A host removed or disconnected in the kernel could
therefore remain attachable until the next UI refresh.

## Delivered

- `ensure_host_connected` now reads the kernel-owned remote-host topology before
  accepting an attach request.
- A successful read replaces the shell projection; missing, disconnected, or
  kernel-unavailable hosts fail closed before any remote session is routed.

## Verification

- `cargo test -p ridge --lib hosts::tests`: 14 passed.
- `pnpm check`: 0 errors, 0 warnings (unchanged by this Rust-only slice).
- Worktree must be clean and `origin/main` synchronized before handoff.

## Remaining gates

Attach-side session flags and live PTY/status updates still need a complete
kernel mutation transaction, including rollback of local pane allocation when a
later transport step fails. No version bump or publication was made under
today's release freeze.
