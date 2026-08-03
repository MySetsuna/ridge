# Iteration 127 - Remote Agent status parity

Date: 2026-08-03  
Status: implementation complete; unreleased by the daily publication freeze

## Change

Remote `SidebarTeamRoster` now consumes the same `agentCardStatus` and
`agentStatusLabel` projection as the desktop Commune cards. `Suspended` and
`Disappeared` therefore render as the red `stopped` rail/dot, pending approval
remains the yellow `waiting` state, and working/idle semantics stay shared.
The old Remote-only `suspended`/`gone` mapping and unused dot style were
removed, eliminating status drift between desktop and mobile.

## Verification

- `pnpm exec vitest run src/lib/teammate/agentCommuneModel.test.ts src/remote/lib/SidebarTeamRoster.test.ts`:
  2 files, 14 passed;
- `pnpm check`: 0 errors, 0 warnings;
- `git diff --check`: clean.

No version bump or publication was made because `v0.1.54` consumed today's
release allowance.
