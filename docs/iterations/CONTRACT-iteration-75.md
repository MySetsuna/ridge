# Iteration 75 Contract — Mobile Remote input continuity

- Date: 2026-07-29
- Status: approved / executing
- Requirements: `REQ-REMOTE-SMOOTH-STATE-02`,
  `REQ-MOBILE-REMOTE-STATE-01`,
  `REQ-MOBILE-REMOTE-WORKER-AUTHORITY-01`,
  `REQ-MOBILE-REMOTE-INPUT-FEEDBACK-01`,
  `REQ-AGENT-COMMUNE-LAUNCH-PROFILE-01`,
  `REQ-RIDGE-MCP-INSTALLER-01`

## Rationale

Current code already owns per-pane parked kernels, a real scrollback decode
Worker, cursor-only IME anchoring, and high/low transport queues. Two remaining
identity transitions still permit a blank or wrongly-associated pane:
Cloud Remote emits an unscoped `panes` snapshot, and WorkspaceTree mutates UI
identity before the host switch has succeeded.

## Scope

1. Every `panes` snapshot carries its source workspace. MainApp never maps an
   unscoped post-handshake snapshot onto the currently selected workspace.
2. Workspace/pane switching commits `{workspaceId,paneId}` only after the host
   switch succeeds. Keep the old frame during the request; use an already
   peeked target pane when available instead of introducing a null-frame gap.
3. Re-verify the explicit keyboard order:
   `scrollToBottom → cursor/fallback anchor → focus`; pointer coordinates never
   influence the input anchor and PTY rows/cols remain unchanged.
4. Re-verify that stdin/control and active raw can advance while scrollback or
   background output is stalled; queues and cancellation return to zero.
5. Make the existing singleton render Worker the production render authority
   when Worker + OffscreenCanvas are available. Wire the already-exported
   `RenderHandle.newFromOffscreen`; bound requests, teardown, and failure
   fallback. Keep the live main-thread kernel only as input/fallback state, not
   as a second painter.
6. Separate virtual-keyboard visibility from system-IME focus; expose
   pane-local scrollback retry and reuse existing dirty/resync truth for
   freshness feedback.
7. Ship the `ridge-mcp` companion in every desktop installer and expose a
   stable registration/config path without persisting endpoint credentials.
8. Add discoverable typed launch profiles plus composite cross-workspace
   Agent creation and message delivery. Explicit workspace identity is
   validated by the host; omission alone retains current-workspace behavior.

## Allowed changes

- `src/remote/MainApp.svelte`
- `src/remote/lib/WorkspaceTree.svelte`
- `src/remote/lib/cloudRemote.ts`
- `packages/remote/src/shared/terminal/{manager,renderWorker,workerHostedRenderer,workerRendererBridge,workerRendererSingleton}*`
- `packages/ridge-mcp/**`
- `packages/ridge-mcp-bridge/**`
- `src-tauri/src/teammate/**`
- Tauri bundle/release workflow and MCP registration documentation
- Focused tests and `scripts/remote-state-e2e.mjs`

## Non-goals

- No second WebSocket, DataChannel, pane cache, or state source.
- No sessionStorage cache restoration.
- No absolute latency/FPS promise without measured baseline.

## Deterministic gates

- Cloud/LAN `panes` snapshots update only their explicit workspace query key.
- A failed workspace switch leaves old workspace, pane, canvas, focus, and
  subscription unchanged.
- A successful cross-workspace pane selection exposes no intermediate
  `{new workspace, old pane}` pair.
- Existing pane kernels/subscriptions survive `A → B → C → A`; true close
  still tears down the exact composite identity.
- Keyboard, weak-network priority, worker, Remote query, Svelte, and Dev CDP
  regression tests pass.
- Worker bind/apply/resize/destroy is real, failure falls back without losing
  pane content, and pending/worker counts return to zero.
- Clean-install bundle inspection finds a version-matched `ridge-mcp` on every
  platform; endpoint rotation reconnects without persisted port/token.
- MCP capability discovery gates launch overrides. Cross-workspace create/send
  reaches only the requested `(workspaceId,paneId/agentId)` and rejects forged
  or mismatched composite identities.

## Session token usage snapshot

- Measured `2026-07-30` with `token-usage --json <current CODEX_THREAD_ID session>`.
- Current thread `019fad54-d6cc-7213-b3a0-7be2b3a3626b` maps to
  `rollout-2026-07-29T18-04-06-019fad54-d6cc-7213-b3a0-7be2b3a3626b.jsonl`.
- Requests: `1403`; input/accounted input: `184099033`; cache read:
  `179900160`; output: `394035`; reasoning output: `152350`; total:
  `184493068`; cache hit: `97.72%`.
- Snapshot only; later turns increase the session total. Cost and
  tokens-per-changed-line remain unknown because no confirmed cost evidence was supplied.

## Stop conditions

- Protocol compatibility requires accepting unscoped steady-state messages.
- A fix needs a second state source or physical connection.
- Worker authority requires changing the Rust canvas backend beyond this
  contract.
