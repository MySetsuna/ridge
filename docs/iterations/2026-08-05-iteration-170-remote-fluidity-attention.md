# Iteration 170 — Remote input-first pane continuity and Agent attention

## Scope

Close the approved Mobile Remote backpressure/live-tail slice together with
the desktop/Remote Pane attention contract. Input responsiveness is the hard
priority: after a Pane switch, the latest PTY tail must remain visible and the
first keystroke must not be lost. Output buffering may use more bandwidth and
memory, but every retained resource remains bounded and reclaimable.

## Root causes

- A keyed `TerminalCanvas` remount left a short raw-frame gap; frames in that
  gap were dropped or the first input arrived before the new handler existed.
- An eight-millisecond input throttle and the async intent gate added avoidable
  round trips; a synchronous keystroke burst could be split into separate RPCs.
- Component parking freed the renderer on every switch, forcing WebGPU/Canvas
  reconstruction and delaying the first paint. Retaining it without a memory
  pressure path would leak GPU/DOM resources.
- Remote Agent roster polling lived only inside the open Team drawer, so Pane
  attention could not appear while the drawer was closed. Active-pane changes,
  resize claims, and background input also incorrectly acknowledged attention.

## Change

- Add a bounded per-key `PaneSwitchBuffer` (4 MiB cap, FIFO copy, overflow
  resync) and drain it before the first fit/render.
- Focus the mobile IME sink synchronously on mount, queue at most 64 KiB of
  pre-attach input, encode early control keys, and flush in order after attach.
- Keep one in-flight input RPC but remove the artificial 8 ms throttle; direct
  synchronous keystrokes reach the scheduler in the same turn while async
  paste intents remain strictly ordered behind their gate.
- Retain a ready renderer across component parks for instant Pane switching;
  memory/hidden reclaim and final detach free it, and component parking replaces
  the old DOM subtree with a tiny sentinel to avoid retaining hidden inputs.
- Add `flushPaneFeed` with an 8 ms bounded catch-up slice, keeping the regular
  frame budget and deferred-feed cap intact.
- Emit Agent attention only on completion/approval/disappearance transitions;
  keep it sticky until the actual terminal input surface receives focus. Mount
  a hidden live roster monitor while the Team drawer is closed; keep history
  refresh at five minutes and live status at three seconds.

## Verification

- `pnpm check` — 0 errors, 0 warnings.
- Full Vitest — 152 files, 1571 passed, 1 skipped (Node emitted only the
  existing invalid `--localstorage-file` warning); focused Remote/desktop
  attention, switch-buffer, terminal input, and RPC scheduler tests are green.
- Iteration gate — `write_scope_exceeded` resolved; current context reports
  `ok: true` with no pending requirements.

## Release boundary

This is runtime work after `v0.1.59` was tagged. The tag's workflow must not be
called complete for these changes; after verification, commit/push the code,
align the next version, tag once, and wait for the full desktop asset matrix.
Remote/Cloud publication remains a separate post-release gate. If CI fails,
rerun/fix the same tag and do not increment the version merely to bypass a
failed build.
