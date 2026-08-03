# Iteration 151 - Remote peek error feedback

Date: 2026-08-03

## Scope

Make non-active workspace expansion honest on the mobile Remote workspace
popup. A failed read-only pane peek must retain the last known tree while
showing an actionable per-workspace error instead of silently rendering an
empty list.

## Implementation

- WorkspaceTree records a bounded per-workspace peek error alongside the
  existing transient pane snapshot.
- Successful refresh clears that workspace's error; failed refresh keeps the
  last good panes and renders the error beside them.
- Active workspace rendering never shows a stale non-active peek error.

## Evidence

- pnpm exec vitest run src/remote/lib/WorkspaceTree.test.ts src/remote/lib/cloudRemote.test.ts src/lib/hosts/hostForest.test.ts --reporter=dot:
  3 files, 50 passed.
- pnpm check: 0 errors and 0 warnings.
- Source regression guard asserts error capture and visible role=alert
  feedback.

## Boundaries

This closes the silent non-active Remote workspace peek failure path. Physical
and public Host latency, WebView2 heap soak, dual-window/dual-host, and full
Kernel authority remain external or larger-scope evidence gates. No release or
version bump is made.
