# Iteration 148 - Preserve Agent identity while idle

Date: 2026-08-03

## Scope

Keep a pane marked as an Agent while its runtime state changes. `busy` is a
runtime state, not the identity marker; an Agent must remain visible when it is
`idle` or `starting` so the Pane header, Agent card, CWD, and attention state
do not disappear between updates.

## Implementation

- Cloud Host topology projection now sets `isAgent` when either
  `agent_state` or `agent_id` is present.
- Mobile Cloud projection uses the same rule, keeping desktop/Remote pane
  metadata aligned.
- Existing `agentState` and `agentId` fields remain unchanged; only the
  identity boolean was corrected.

## Evidence

- `pnpm exec vitest run src/lib/remote/cloud/cloudHostTopologyLink.test.ts src/remote/lib/cloudRemote.test.ts`:
  39 passed.
- Regression fixtures assert an `idle` Agent remains `isAgent: true`.

## Boundaries

This closes the projection identity regression only. Physical mobile/public
Remote, WebView2 heap soak, and full Kernel authority remain open evidence
gates. No release or version bump is made.
