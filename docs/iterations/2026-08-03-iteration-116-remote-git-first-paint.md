# Iteration 116 - Remote Git first-paint path

Date: 2026-08-03  
Status: implementation complete; release deferred by the daily publication freeze

## Root cause

The mobile client already deferred Git Graph and requested
`includeDetails:false`, but the host still ran the full SCM snapshot. That
snapshot spawned two `git diff --numstat` children for line counts that the
compact Remote panel discards, so opening Git waited on avoidable subprocesses.

## Delivered

- Add a shared, semaphore- and timeout-guarded fast SCM status path that parses
  one porcelain status result and skips numstat children.
- Route Remote `git_status` first paint to the fast path when
  `includeDetails:false`; desktop/full compatibility requests keep the complete
  line-count path.
- Keep Graph/history lazy and separate; no protocol shape change.

## Verification

- `cargo test -p ridge-core --lib --quiet`: 315 passed.
- `cargo test -p ridge --lib hosts:: --quiet`: 59 passed.
- `pnpm exec vitest run src/remote/lib/RemoteGitPanel.test.ts src/remote/lib/sidebarProvider.test.ts`: 16 passed.

## Remaining gates

Real-device latency and public Remote measurements remain external. No version
bump or publication was made because `v0.1.54` consumed today's release
allowance.
