# Iteration 147 - Remote Host discovery error visibility

Date: 2026-08-03

## Scope

Keep remote Host workspace and pane discovery failures observable. A failed
RPC must not be converted into an empty workspace or pane list, because that
looks like a healthy Host with no data and prevents Query/Hosts UI from
showing actionable progress, retry, or error state.

## Implementation

- `CloudRemoteConnection.listWorkspaces` now propagates `list_workspaces`
  failures while retaining the existing active-workspace fallback only for the
  independent active-id lookup.
- `CloudRemoteConnection.listWorkspacePanes` now propagates layout discovery
  failures instead of returning `[]` and erasing a previously known tree.
- Existing Host topology callers remain responsible for retaining their last
  good snapshot and presenting the error through their Query/Hosts state.

## Evidence

- `pnpm exec vitest run src/remote/lib/cloudRemote.test.ts`: 35 passed.
- Added regression coverage for workspace and pane discovery rejection.

## Boundaries

This closes the silent-error conversion in the Cloud Remote adapter only. A
physical/public Host connection, slow-device feedback, and full Kernel-domain
authority remain external or larger-scope evidence gates. No release or
version bump is made.
