# Iteration 120 - Remote host workspace save refresh

Date: 2026-08-03  
Status: implementation complete; release deferred by the daily publication freeze

## Root cause

`saveHostWorkspace` successfully sent the remote save mutation but returned
without refreshing the linked host topology. The Hosts panel therefore kept the
previous workspace name/list until an unrelated poll, remount, or reconnect.

## Delivered

- Reuse `refreshLinkedHostAfterMutation(hostId)` after a successful save, with
  the same error propagation and single-flight refresh path used by create,
  rename, close, and pane mutations.
- Added a source contract regression that asserts refresh occurs after the
  save promise in `saveHostWorkspace`.

## Verification

- `pnpm exec vitest run src/lib/stores/hostsMutation.test.ts`: 1 passed.
- `git diff --check`: clean.
- No version bump or publication was made because `v0.1.54` consumed today's
  release allowance.
