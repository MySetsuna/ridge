# Iteration 150 - Remote legacy response guard

Date: 2026-08-03

## Scope

Prevent a late LAN `workspace-panes` response from being consumed by a
different workspace request. The legacy frame has no request id, so routing
must validate the echoed workspace payload before settling the shared pending
slot.

## Implementation

- `RemoteConnection` pending requests can carry a response matcher.
- `listWorkspacePanes` accepts legacy replies without `workspaceId`, but rejects
  a reply that names another workspace; the dispatcher ignores such a stale
  frame and keeps the original request pending.
- The defensive method-level check remains fail-closed for transports that
  bypass the dispatcher guard.

## Evidence

- `pnpm exec vitest run packages/remote/src/shared/transport/wsRemoteRpcScheduler.test.ts --reporter=dot`:
  11 passed.
- `cmd /c "pnpm exec vitest run --reporter=dot"`: 147 files, 1537 passed,
  1 skipped, exit code 0.
- `pnpm check`: 0 errors and 0 warnings.
- Regression covers a stale reply arriving before the matching workspace reply;
  the queued request is sent only after the correct response settles.

## Boundaries

This closes the legacy workspace-response cross-wire/empty-tree path. Physical
and public Remote latency, WebView2 heap soak, dual-window/dual-host, and full
Kernel authority remain external or larger-scope evidence gates. No release or
version bump is made.
