# Iteration 104 — Kernel-authoritative host writes

Date: 2026-08-03
Status: implementation complete; release deferred by the daily publication freeze

## Root cause

The desktop host registry mirrored mutations to `ridge-kernel` on a best-effort
basis and updated the process-local topology regardless of the mirror result.
After a kernel outage or restart, that shell-only record could be shown as a
real host and then disappear on the next kernel projection.

## Delivered

- Added a kernel-authoritative upsert path for frontend registration and host
  probing. The shell projection changes only after the domain write succeeds.
- Added kernel-first host removal for `forget_host`, so a failed domain delete
  cannot silently leave the shell and kernel diverged.
- Kept legacy internal status/session mirrors isolated while live-session
  migration remains a separate follow-up; repeated identical frontend records
  avoid duplicate domain writes.
- Added a deterministic regression proving a rejected kernel write never
  publishes a shell-only host record.

## Verification

- `cargo test -p ridge --lib hosts::tests`: 14 passed.
- `pnpm check`: 0 errors, 0 warnings.
- Worktree must be clean and `origin/main` synchronized before handoff.

## Remaining gates

Host live-session/status mutation and PTY authority still need migration behind
the kernel, plus no-Tauri desktop-exit/rdg-attach lifecycle evidence. No version
bump or publication was made under today's release freeze.
