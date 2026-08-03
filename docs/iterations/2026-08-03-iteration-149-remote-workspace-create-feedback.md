# Iteration 149 - Remote workspace create feedback

Date: 2026-08-03

## Scope

Make remote workspace creation observable. Transport/auth/RPC failures must
reach the Host or mobile workspace operation layer; a null/empty workspace ID
must also produce visible feedback instead of looking like an ignored tap.

## Implementation

- Cloud Host topology and mobile Cloud adapters no longer swallow
  `create_workspace` RPC failures.
- The mobile workspace popup reports an explicit failure when the adapter
  returns no workspace ID.
- Existing LAN response semantics remain intact; successful IDs still follow
  the same switch, refresh, and pane-creation path.

## Evidence

- `pnpm exec vitest run src/lib/remote/cloud/cloudHostTopologyLink.test.ts src/remote/lib/cloudRemote.test.ts src/remote/lib/WorkspaceTree.test.ts`:
  44 passed.
- Regression coverage asserts both rejected RPCs and empty-ID UI feedback.

## Boundaries

This closes silent create feedback in the remote adapters/UI. Physical/public
Host latency, WebView2 heap soak, and full Kernel authority remain open
evidence gates. No release or version bump is made.
