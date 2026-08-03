# Iteration 126 - Context-aware Resize deduplication

Date: 2026-08-03  
Status: implementation complete; unreleased by the daily publication freeze

## Change

`PaneRpcScheduler` now includes PTY mode parameters (`isAlt` and
`isInlineTui`) in the Resize signature before comparing the active/latest
request. Two equal dimensions with equal mode context therefore coalesce while
the first request is in flight; a genuine context change still schedules a
new Resize. This removes a subtle duplicate-RPC path without changing the
latest-wins behavior.

## Verification

- `pnpm exec vitest run packages/remote/src/shared/transport/paneRpcScheduler.test.ts`:
  15 passed;
- `pnpm check`: 0 errors, 0 warnings;
- `git diff --check`: clean.

No version bump or publication was made because `v0.1.54` consumed today's
release allowance.
