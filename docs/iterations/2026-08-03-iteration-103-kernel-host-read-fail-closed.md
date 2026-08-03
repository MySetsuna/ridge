# Iteration 103 — Kernel host read fail-closed

Date: 2026-08-03  
Status: implementation complete; release deferred by the daily publication freeze

## Root cause

The desktop `host_list_snapshot` command attempted to read the kernel-owned
remote-host topology, but silently fell back to `AppState.hosts.snapshot()` when
the kernel was unavailable or returned malformed data. That made stale shell
state look authoritative and violated the thin-shell/domain-SSOT contract.

## Delivered

- `kernel_host_snapshot` now returns a typed error for missing/unhealthy kernel
  endpoint and propagates domain decode failures.
- `host_list_snapshot` projects only a successful kernel snapshot and otherwise
  fails closed; it never returns the process-local cache as a substitute.
- Desktop startup logs a warning when the initial kernel projection cannot be
  restored, while the existing kernel-death watcher still exits the shell.
- Hosts refresh converts the command failure into visible `hostsError` feedback.
- Added a regression proving a failed kernel projection does not become a
  successful stale-shell response.

## Verification

- `cargo test -p ridge --lib hosts::tests`: 13 passed.
- `pnpm check`: 0 errors, 0 warnings.
- Worktree is clean after commit and `origin/main` is synchronized.

## Remaining gates

This closes the read-path fallback only. Full domain migration still requires
moving host mutation and live-session authority behind the kernel, then proving
desktop-exit/rdg-attach and kernel-shutdown behavior in the no-Tauri smoke path.
No version bump or publication was made under today's release freeze.
