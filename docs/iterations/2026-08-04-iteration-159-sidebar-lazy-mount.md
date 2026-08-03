# Iteration 159 — Desktop sidebar first-visit mounting

## Scope

The sidebar used CSS `hidden` for inactive panels but still instantiated every
panel. Explorer, Source Control, Remote, Agent Commune, Hosts, and Search
therefore ran startup effects, queries, listeners, and projections together;
tab switching only changed visibility. This explained a high-risk source of
desktop first-load contention and black/slow tab transitions.

## Change

- Added a reactive `sidebarVisited` set, seeded with the default Files tab.
- Guarded each sidebar panel with its first-visit bit.
- Retained visited panel instances while switching tabs, preserving drafts and
  Query snapshots without rebuilding the panel on every switch.
- Kept the existing `hidden` layout behavior and tab routing unchanged.
- Added `SidebarLazyMount.test.ts` source contracts for first-visit guards and
  retention semantics.

## Verification

- `pnpm exec vitest run src/lib/components/SidebarLazyMount.test.ts src/lib/components/SettingsPanel.test.ts --reporter=dot`:
  7 passed.
- `pnpm check`: 0 errors, 0 warnings.
- `git diff --check`: passed (LF/CRLF conversion warnings are checkout
  normalization only).

## Remaining external gates

Physical startup and tab-switch frame traces, black-screen reproduction,
WebView2 heap soak, public/physical Remote, dual-window workspace singleton,
remote Host latency, and complete kernel-domain authority remain required
before any release claim. No version bump or release was made.
