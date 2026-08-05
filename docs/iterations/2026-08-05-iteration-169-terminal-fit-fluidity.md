# Iteration 169 — terminal cold-fit and Remote fluidity

## Scope

Release-blocking regression reported on `0.1.57`: a newly opened terminal could
remain at the attach seed (`80×24`) while its pane had already reached its real
layout size; Remote output could also accumulate a long render tail during a
burst.

## Root cause

`ResizeObserver` is not a complete lifecycle signal for `display:none → flex`
and late font/WebGPU metrics. The old path only recomputed the visual scissor
after workspace activation, so the kernel grid was not necessarily fitted.
The deferred PTY FIFO drained one chunk per pane per animation frame, which
made a busy stream fall behind even though input/RPC remained independent.

## Change

- Add a bounded, cancellable cold-fit retry window (`0/16/50/150/400ms`).
- Refit panes when a workspace becomes visible, not only recompute the scissor.
- Cancel fit timers on park/detach; compare measured content-box capacity with
  the local kernel grid before scheduling another attempt.
- Drain up to two deferred chunks within a 6ms frame budget, rotating panes so
  one noisy PTY cannot monopolize the Remote main thread.
- Preserve WebGPU-first rendering, local-grid authority, live-tail ordering,
  and the existing input/RPC queues.

## Verification

- `pnpm check` — 0 errors, 0 warnings.
- Full Vitest — 150 files, 1564 passed, 1 skipped.
- Focused terminal/pane-tree tests — 86 passed.

`v0.1.58` remains an unpublished draft because its assets were built before
this fix. A new version must be tagged after the clean-worktree gate.
