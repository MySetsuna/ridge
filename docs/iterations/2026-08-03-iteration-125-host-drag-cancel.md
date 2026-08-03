# Iteration 125 - Host session drag cancellation

Date: 2026-08-03  
Status: implementation complete; unreleased by the daily publication freeze

## Change

Host-session drag now has one cancellation path for `pointercancel` and window
`blur`, in addition to the normal pointer-up path. It clears the drag sentinel,
hover preview, cursor, and global listeners together, so a mobile system gesture
or focus loss cannot leave the next drag permanently stuck.

## Verification

- `pnpm exec vitest run src/lib/actions/hostSessionDrag.test.ts`: 3 passed;
- `pnpm check`: 0 errors, 0 warnings;
- Commit `871b251` is pushed to `origin/main`.

No version bump or publication was made because `v0.1.54` consumed today's
release allowance.
