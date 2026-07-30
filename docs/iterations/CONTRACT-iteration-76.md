# Iteration 76 Contract — Remote stability, bounded RPC, and desktop ownership

- Date: 2026-07-30
- Status: in progress
- Implementation baseline: `origin/main@f62133c`, local code `fc6a73b598f4`
- Requirements: `REQ-20260730-01`, `REQ-MOBILE-REMOTE-RUNTIME-LASTERROR-01`
- Delivery rule: one concern per commit; no push, release, or generated Remote artifact in this iteration unless separately authorized.

## Sources and authority

1. Recent NotebookLM artifacts:
   - `2026-07-29-notebooklm-guidance-65.md`: bounded scrollback Worker, composite `PaneRef`, one LAN writer, background pane survival, keyboard focus order.
   - `2026-07-28-notebooklm-guidance-64.md`: end-to-end `(workspaceId,paneId)`, bounded priority writer, Agent's Commune three-tab/history visibility.
   - Live NotebookLM query was admitted by `notebook_gate.py`, but failed because local Google auth expired. These checked-in artifacts remain the usable conversation record; live comparison stays open, and NLM does not decide code facts.
2. Previous iteration:
   - `CONTRACT-iteration-75.md` and commits `c73ce87`, `d2b8b82`, `dc2788b`.
   - Completion claims were re-checked against current symbols and tests instead of copied as fact.
3. Current user report:
   - Remote public RPC/SCM/PTY/log storms, WebView2 memory, true clear, Commune visibility, multi-window ownership, Remote Host attach/resize, and mobile `runtime.lastError`.

## Three-source reconciliation

| Concern | Current evidence | Classification | This iteration |
| --- | --- | --- | --- |
| Composite `PaneRef` and scoped snapshots | Iteration 75 scoped cloud pane snapshots and switching; remaining Host/Pane lifecycle paths still lack one cancellation registry | Partly complete | Preserve identity; add pane-scoped generation/cancel |
| LAN single writer / bounded lanes | Current state records exclusive writer and bounded high/low lanes | Complete, retain regression | Do not add a second connection or writer |
| Scrollback Worker | Worker authority, pending cap, timeout, teardown exist | Complete core; memory proof missing | Add memory/clear pressure proof |
| Background pane continuity | Parked kernels and subscriptions exist; physical mobile proof remains external | Partly complete | Run composite-identity weak-network/physical matrix |
| Agent's Commune | Members/groups/history UI exists, but `teammateEnabled` gates entry; typed launch profiles/cross-workspace MCP have no visible UI proof | Regression / partly complete | Fix discoverability and add visible E2E |
| Git process containment | Unified spawn, timeout, Windows tree kill, semaphore, latest-win slot and hanging-binary tests exist | Complete backend | Connect frontend stable slot and negative cache |
| RPC request containment | Per-request timeout/Abort/reconnect cleanup existed; pending cap, timeout cancel, metrics absent | Partly complete | First slice landed in `5eece08` |
| Mobile Cloud input/resize | Both invoked per event; errors silently swallowed | Missing | Resize slice landed in `45355db`; input protocol remains P0 |
| Non-Git polling | Periodic scan removed and backend rejects before spawn; empty discovery was not cached, branch/stash lack shared detection | Partly complete / regression | Empty cache + status single-flight landed in `fc6a73b` |
| Host attach | Topology/list/create, single-flight, abort, last-good exist; slow Host blocks aggregate refresh and stage feedback is absent | Partly complete | Stage loading, isolate slow Host, then drag/resize |
| Terminal clear/memory | Scrollback cap and teardown exist; right-click/`clear`/kernel/backend equality and WebView2 release are unproved | Partly complete | Instrument, unify clear, memory-pressure regression |
| Desktop multi-window | Tauri release path remains single-instance and creates only `main`; no workspace owner registry | Conflicts with approved behavior | New process/window ownership protocol after lifecycle guards |
| Mobile `runtime.lastError` | No project `chrome.runtime`, `chrome.tabs`, `browser.runtime`, `sendResponse`, or `runtime.lastError`; PWA worker only uses `Client.postMessage` | Project code excluded; environment attribution pending | Zero business diff; mobile clean-profile/extension A/B |

## Execution plan

### 76.1 Shared RPC containment — P0, first

- Current: `RpcClient` had a 20 s timeout and cleanup, but an unbounded pending map; timeout removed only the local entry.
- Target: bounded in-flight set, timeout cancellation on the host, monotonic queue/timeout counters.
- Implementation: `DEFAULT_MAX_IN_FLIGHT=256`; reject excess before wire send with `RpcQueueFullError`; timeout sends exactly one `$/cancel`; expose immutable diagnostics snapshot.
- Acceptance: peak never exceeds cap; excess request emits no business frame; timeout leaves `inFlight=0` and emits one cancel; reconnect/dispose behavior unchanged.
- Regression: correlation, out-of-order responses, AbortSignal, reconnect, notification, cloud fault injection.
- Status/evidence: complete in `5eece08`; `rpcClient.test.ts` 25/25.

### 76.2 Mobile Cloud Resize latest-value-wins — P0, second

- Current: each ResizeObserver/claim event directly invoked `resize_pane`.
- Target: per composite pane, at most one in flight and one replaceable latest geometry.
- Implementation: dedupe active/pending/applied dimensions; drain latest after completion; close/prune/disconnect delete the lane; no automatic timeout retry fan-out.
- Acceptance: 1,000 synthetic observations yield `inFlight<=1`, `pending<=1`, final host size equals final observation, identical size sends zero extra RPC, close sends no queued resize.
- Regression: claim/refresh semantics, workspace scoping, reconnect and pane close.
- Status/evidence: complete in `45355db`; focused test covers burst, dedupe, final geometry, close cleanup.

### 76.3 Mobile terminal input protocol — P0, third

- Current: every input event calls `write_to_pty` fire-and-forget; timeout ambiguity prevents safe blind retry.
- Target: ordered, bounded, observable input without loss, reordering, or duplicate replay.
- Implementation: add per-pane sequence/idempotency acknowledgement at the shared transport boundary; one in-flight batch plus one bounded aggregation buffer; pause after consecutive timeouts with exponential backoff; reconnect resumes only from last acknowledged sequence.
- Acceptance: slow/hanging transport preserves exact byte stream and order; one in-flight write; queue/bytes bounded; timeout threshold pauses; close returns pending/bytes/timers to zero; no `Pane not found`.
- Regression: IME, paste, TUI mouse, normal typing, reconnect, pane close, LAN/cloud parity.
- Stop condition: do not ship a retry-only client queue without host acknowledgement—it can duplicate input.

### 76.4 SCM repository detection and status scheduling — P0/P1

- Current: backend hard guards and `slot` existed; frontend omitted `slot`. Empty discovery was not a negative cache entry.
- Target: one detection per cwd signature; one status request per repo; status/branch/stash share repository truth.
- Implementation: empty-result negative cache; per-root status single-flight; stable `scm-status:${root}` slot. Next slice moves detection cache into a shared store consumed by branch/stash.
- Acceptance: 100 same-cwd non-Git triggers produce one discovery and zero status/branch/stash git processes; cwd change rechecks once; concurrent same-root status produces one RPC.
- Regression: repo creation after cwd transition, multi-repo discovery, watcher/manual refresh, commit/pull graph invalidation.
- Status/evidence: first slice complete in `fc6a73b`; shared branch/stash cache and process-count test remain.

### 76.5 Pane-scoped lifecycle registry — P1, after 76.3

- Current: global RPC dispose and subscription cleanup exist; pane close does not cancel write/resize/history RPC as one unit.
- Target: destroy marks pane dead before close, cancels every pending operation, rejects stale completion by generation.
- Implementation: composite-key lifecycle token with `AbortController`, queues, listeners, scrollback fetch and resize lane; destroy order is mark-dead → cancel/clear → unsubscribe → close.
- Acceptance: hanging write/resize/scrollback followed by destroy yields zero pending/timers/listeners/bytes; late result cannot mutate state or retry.
- Regression: park/unpark is not destroy; `A→B→C→A` preserves live panes; reconnect creates a fresh generation.

### 76.6 Error aggregation and RPC backoff — P1

- Current: identical SourceControl/RidgePane errors print per failure; some cloud failures are fully silent.
- Target: fingerprinted aggregation, useful first failure, periodic `repeated N times`, method/key timeout statistics and circuit state.
- Implementation: one shared logger/telemetry sink; cancelled/superseded are debug; timeout/queue are counters; exponential backoff with capped delay and half-open probe for idempotent reads only.
- Acceptance: 126 identical errors produce at most first event plus one `repeated 126 times` summary; no blind retry for terminal input; counters are deterministic.
- Regression: distinct errors remain distinct, development stack retained once, reconnect clears only transient circuit state.

### 76.7 Terminal memory and true clear — P1

- Current: manager defaults to bounded scrollback and teardown releases worker/kernel/timers; `clearScrollback` alone does not prove UI, kernel and backend buffers all clear.
- Target: bounded long-run memory, idle/hidden pane release, identical right-click and command clear semantics.
- Implementation: one `clearPaneHistory(PaneRef)` path for canvas/kernel/scrollback/backend; pressure/hidden-pane eviction preserves active screen and input; expose per-pane bytes/kernel/worker counters.
- Acceptance: over-limit history evicts oldest blocks; both clear entries render empty and report zero scrollback/backend buffered bytes; closed/evicted pane memory falls after GC observation; WebView2 long-run curve plateaus.
- Regression: alternate screen, selection/search, parked panes, reconnect replay, active TUI.

### 76.8 Remote Host staged attach — P1/P2

- Current: topology/list/create have partial guards, but `Promise.all` lets a slow Host delay the whole forest; attach stage and first geometry proof are missing.
- Target: modal closes immediately; panel shows discovery/auth/list/attach/resize stages; each Host resolves independently.
- Implementation: per-Host settled refresh with last-good data, abort/generation guard, explicit progress DTO; converge drag, button and programmatic attach on one binding path; read actual pane rect and send immediate Resize.
- Acceptance: fast Host appears while another hangs; errors remain actionable; existing/create workspaces refresh; drag attaches; first frame uses measured size; Resize button resyncs.
- Regression: LAN/public/offline/slow Host, reconnect, duplicate attach, pane destroy during attach.

### 76.9 Desktop multi-window and workspace ownership — P2

- Current: release build uses single-instance and only a `main` window.
- Target: windows may multiply; a Remote workspace has one owning window process-wide.
- Implementation: app-level `workspaceId→windowLabel` registry, atomic claim/release, focus existing owner on conflict; new windows may open only unowned workspaces.
- Acceptance: two windows coexist; opening an owned workspace focuses/restores old window; crash/close releases ownership; race has exactly one winner.
- Regression: tray/deep-root close, restore set, local workspace behavior, remote reconnect, minimized/background owner.

### 76.10 Agent's Commune visibility — P2

- Current: three tabs exist, but entry is gated and iteration 75 typed launch/cross-workspace functions lack visible UI evidence.
- Target: enabled capability is discoverable; disabled state explains why; launch profile and cross-workspace actions are visible and scoped.
- Implementation: make gate state explicit, connect typed profiles to existing create flow, preserve backend roster as SSOT.
- Acceptance: visible E2E covers enabled, disabled, no-agent, cross-workspace create/send and forged identity rejection.
- Regression: member/group/history keyboard access, HITL controls, pane header status, no duplicate history source.

### 76.11 Mobile `runtime.lastError` attribution — P2, zero-code unless proven

- Current: source audit found no Chrome Extension Messaging API.
- Target: identify the first warning's script URL/frame/extension owner and stop sustained repetition at its owner.
- Implementation: reproduce on the affected phone with clean browser profile/incognito where supported, then enable extensions/injected tools one at a time; record first warning source and count.
- Acceptance: clean profile has zero sustained warnings; enabling the offending extension reproduces it, or a project URL proves ownership and opens a new approved code slice.
- Regression: PWA service worker storage message still works; no Console filtering/suppression is added.

### 76.12 Low-priority browser warnings and performance A/B — last

- Current: deprecated parameters, Canvas `willReadFrequently`, and runtime warnings are unbaselined.
- Target: remove owned warnings after core stability; quantify improvement without hardware-independent promises.
- Acceptance: same scenario before/after records RPC by method/key, timeout/queue peaks, p50/p95 input latency, CPU, network, WebView2 memory and correctness; repeated Console errors fall at least 90%, invalid Git work is near zero.
- Regression: desktop, LAN, public cloud; normal/latency/loss/disconnect/reconnect.

## Deterministic gates run this round

- `requirements_gate.py assert-task-executable`: exit `0`.
- `notebook_gate.py assert-allowed --trigger user_requested`: exit `0`; subsequent query failed only at external authentication.
- Three read-only worker packets: batch validation `valid=true`, no writes outside result/evidence paths.
- `pnpm vitest run packages/remote/src/shared/transport/rpcClient.test.ts src/remote/lib/cloudRemote.test.ts`: 47/47.
- `pnpm check`: 0 errors, 0 warnings.

## Remaining external evidence

- Refresh NotebookLM authentication, then compare the live newest two conversations with guidance 64/65; any new product behavior enters Pending approval.
- Affected phone: capture first `runtime.lastError` source and clean-profile/extension A/B.
- Public Remote and WebView2 long-run A/B cannot be claimed from unit fixtures.
- No push or release performed.
