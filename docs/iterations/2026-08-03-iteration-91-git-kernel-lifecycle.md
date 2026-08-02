# Iteration 91 — Git panel and kernel watcher lifecycle

Date: 2026-08-03  
Baseline: `v0.1.44` (`24420a4` runtime release commit; `6de14a3` archive head)

## Scope

The previous sidebar fence covered directory, search, and file/diff reads, but
Git status panels still accepted late results after a refresh, pane switch, or
component teardown. The desktop kernel watcher also discarded thread-spawn
errors, making a missing lifecycle guard look healthy.

## Landed

- Shared `SidebarGitPanel` now aborts its previous status observer, fences
  success/error/finally with a generation, and cancels on destroy.
- Remote `RemoteGitPanel` uses the same status fence and cancels an active Git
  action when the panel is destroyed. Its busy guard still prevents an action
  from overlapping a refresh.
- `SidebarProvider.gitStatus` and the Tauri/WS adapters carry an optional
  `AbortSignal`. QueryClient-backed Remote status remains a shared request;
  the signal stops only the leaving observer, while the no-client path reaches
  the WebSocket request.
- `spawn_kernel_death_watcher` now returns a `Result<JoinHandle, String>`;
  the Tauri setup logs and propagates a thread-spawn failure instead of
  silently dropping it. A real dead-PID test joins the watcher and verifies the
  death callback.

## Deterministic evidence

- `pnpm exec vitest run src/shared/sidebar/gitRequestLifecycle.test.ts src/remote/lib/sidebarProvider.test.ts src/lib/transport/ws.test.ts --reporter=dot` — 3 files, 16 passed.
- `pnpm test -- --reporter=dot` — 141 files, 1470 passed, 1 skipped.
- `pnpm check` — 0 errors, 0 warnings.
- `cargo test --manifest-path src-tauri/Cargo.toml kernel_lifecycle --lib` — 5 passed.
- `git diff --check` — clean before versioning.

The tests cover stale-result fences and a real watcher thread with a dead PID;
they do not claim protocol-level cancellation of a remote host task.

## Audited residuals

- Remote `PaneInfo` has no branch identity. LAN and Cloud pane lists do not
  provide one; precomputing branch per pane would add Git processes and defeat
  the SCM polling budget. A future protocol extension or cached branch-identity
  RPC is required before branch-aware Query keys can be authoritative.
- WS `AbortSignal` currently cancels the browser's pending promise only. The
  legacy host `data-request` loop awaits dispatch serially, and `$/cancel` is
  implemented only for the JSON-RPC leg. True host cancellation requires a
  task table, cancellation token, receive/dispatch separation, and kill-tree
  propagation for external commands; this slice intentionally does not fake it.
- Full Kernel authority/standalone-host migration, physical mobile
  `runtime.lastError` attribution, WebView2 heap soak, dual-window/Remote
  workspace singleton E2E, public WebRTC/authenticated Git, and other external
  gates remain open.

## Publication plan

Runtime changes are in the current worktree after `v0.1.44`; they must be
versioned and published as `v0.1.45` only after the clean-worktree gate,
version contract, full Desktop matrix, Remote artifact activation, and Cloud
health verification pass.
