# Iteration 82 Contract — Lifecycle, Kernel Git, and MCP Authority

## Scope

- Carry forward the Remote runtime/memory requirement: component listeners and
  delayed pane refreshes must not survive mobile unmount or transport teardown.
- Carry forward the kernel convergence requirements: Git domain work must not
  block the async control plane, and nested paths must resolve to their parent
  repository consistently.
- Carry forward the MCP kernel-authority requirement: default discovery must not
  silently fall back to a stale teammate sidecar after the kernel disappears.

## Implemented

- `src/remote/MainApp.svelte` now retains every transport unsubscribe handle,
  clears the pane refresh debounce on destroy, and tears down listeners through
  an idempotent cleanup.
- `src/remote/lib/listenerCleanup.ts` and its test provide the once-only
  unsubscribe contract; `CloudRemoteConnection` rejects late provider state,
  error, and reconnect callbacks after disposal.
- `src/remote/lib/generationGuard.ts` prevents an older workspace snapshot from
  mutating a newer or already-destroyed mobile view; teardown invalidates the
  guard before transport disposal, and workspace restore is only committed by
  the current generation.
- `packages/remote/src/shared/terminal/workerHostedRenderer.ts` records the
  pane identity of pending worker requests, cancels them before `destroy`, and
  ignores bounded late-reply tombstones; pane teardown no longer waits behind
  stale frame RPCs.
- `packages/ridge-kernel/src/domain.rs` runs repository discovery and SCM status
  inside `spawn_blocking`, and uses ancestor-aware repository detection. A test
  covers a nested path under a repository root.
- `packages/ridge-mcp-bridge` now defaults to the active kernel registry and
  fails closed when no kernel is available. Explicit `--url/--token` remains
  supported; legacy environment/sidecar discovery requires `--legacy-sidecar`.

## Verification

- `pnpm exec vitest run src/remote/lib/listenerCleanup.test.ts src/remote/lib/cloudRemote.test.ts --reporter=dot` — 2 files, 33 passed.
- `pnpm exec vitest run src/remote/lib/generationGuard.test.ts src/remote/lib/listenerCleanup.test.ts src/remote/lib/cloudRemote.test.ts --reporter=dot --silent` — 3 files, 35 passed.
- `pnpm exec vitest run packages/remote/src/shared/terminal/workerHostedRenderer.test.ts packages/remote/src/shared/terminal/workerRendererBridge.test.ts --reporter=dot --silent` — 2 files, 33 passed.
- Full Vitest — 120 files, 1374 passed, 1 skipped.
- `pnpm check` — 0 errors, 0 warnings.
- `cargo test -p ridge-kernel --lib --quiet` — 15 passed.
- `cargo test -p ridge-mcp-bridge --lib --quiet` — 8 passed.
- Focused commits pushed to `main`: `367c053`, `66d51f0`, `1475abc`, `0207319`, `b402f75`.

## Closure status

The code slice is complete and pushed. Requirements remain `Active` until
physical mobile clean-profile/extension A-B attribution, WebView2 heap soak,
public Remote long-run evidence, and real desktop/rdg kernel process E2E are
recorded. No Console filtering or third-party extension code was changed.
