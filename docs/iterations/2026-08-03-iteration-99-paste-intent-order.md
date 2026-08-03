# Iteration 99 — ordered asynchronous terminal input

Date: 2026-08-03  
Status: implementation complete; release gate pending

## Requirement

Close the remaining `REQ-TERMINAL-PASTE-ORDER-02` gap. A paste must reserve its
`(workspaceId,paneId)` input position before an asynchronous clipboard or image
read. Later keyboard, drag/drop, Agent/MCP, and programmatic writes must remain
behind that payload on desktop, LAN Remote, Cloud Remote, and the host-topology
remote path. Pane destruction/disconnect must cancel the pending intent.

## Root cause

The existing PTY/RPC FIFO started only after `readText()` or image conversion
completed. A user could paste, then type (or an Agent could inject input) while
the clipboard promise was pending; the later write entered the transport first.
The byte queues themselves were ordered, but the asynchronous source had not
reserved an input slot.

## Implementation

- Added `packages/remote/src/shared/terminal/paneInputGate.ts`, a bounded,
  per-pane intent gate. The first operation starts synchronously to preserve
  existing input admission latency; later operations chain behind it. Retire
  and generation checks prevent writes from targeting a destroyed pane.
- Desktop `RidgePane` reserves before clipboard/image awaits, then emits one
  bracketed-paste payload through the existing PTY write queue. Local keyboard
  writes and Agent group/member dispatches use the same gate.
- `RemoteLink` exposes optional `enqueueStdinTask`; LAN, Cloud, and public host
  topology links reserve before asynchronous paste reads while retaining the
  existing `PaneRpcScheduler` batching, retry, timeout, and byte limits.
- Mobile `TerminalCanvas` and `MainApp` use the task path for the clipboard
  button and Ctrl/Cmd+V; right-click/native paste remains a synchronous payload
  through the same transport queue.
- Close, prune, disconnect, and local pane cleanup retire intent lanes.

## Verification

- `pnpm check`: 0 errors, 0 warnings.
- Full Vitest: 143 files, 1479 passed, 1 skipped.
- Focused gate, PTY FIFO, pane RPC, Cloud Remote, LAN Remote, and host-topology
  tests: 46 passed.
- `pnpm build:remote:mobile`: production mobile/PWA build succeeded.
- `pnpm build`: production desktop build succeeded.
- Physical Windows ConPTY and real phone/public Remote timing evidence remain
  external user-track gates; no field proof is inferred from local tests.

## Release gate

The code must be committed and pushed with a clean worktree before publishing a
new version. If released, attach the normal matching desktop assets and publish
Remote/Cloud through `publish-remote.yml`; do not claim physical-device closure.
