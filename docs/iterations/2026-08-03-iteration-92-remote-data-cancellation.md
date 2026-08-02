# Iteration 92 — Remote data-request cancellation and bounded dispatch

Date: 2026-08-03  
Status: runtime slice complete; versioned publication required

## Scope

This slice addresses the high-priority Remote Host failure mode where a
`WsDataProvider` observer aborted or timed out locally while the host continued
executing the `data-request`. The old host loop awaited each request inline,
which serialized file/Git work with control traffic and left stale work in the
queue.

## Implemented contract

- `WsDataProvider` sends one `data-cancel` frame on AbortSignal, timeout, or
  provider disposal. Closed transports remain fail-closed and are never retried.
- Host `data-request` dispatch is concurrent but bounded at 32 in-flight tasks.
- `DataRequestRegistry` owns each task handle and its Git slot. Cancellation
  aborts the task, invalidates the slotted Git generation, and suppresses any
  result that raced into the result channel.
- Cancellation may arrive before the request frame; the bounded pre-cancel set
  consumes that ID exactly once.
- Socket teardown cancels every remaining request before pane/client cleanup.
- `git_status` uses the existing process guard's latest-win slot, so a canceled
  live Git child is tree-killed and its semaphore admission is reclaimed.

## Files

- `src/lib/transport/ws.ts`
- `src/lib/transport/ws.test.ts`
- `src-tauri/src/remote_host_impl.rs`
- `packages/ridge-core/src/commands/git.rs`

## Verification

- `pnpm exec vitest run src/lib/transport/ws.test.ts --reporter=dot`: 6 passed.
- `pnpm test -- --reporter=dot`: 141 files, 1472 passed, 1 skipped.
- `pnpm check`: 0 errors, 0 warnings.
- `cargo test --manifest-path src-tauri/Cargo.toml remote_host_impl::jsonrpc_tests --lib -- --nocapture`: 11 passed.
- `cargo test -p ridge-core commands::git --lib --quiet`: 33 passed.
- `cargo check --manifest-path src-tauri/Cargo.toml --lib`: exit 0; 39 pre-existing warnings.

## Limits and follow-up

This closes the Remote legacy `data-request` cancellation path and the
`git_status` process-kill path. Git mutation methods that still use unslotted
core APIs retain their bounded timeout/tree-kill guard; they are not claimed as
instant cancellation. JSON-RPC `invoke-request` remains serial and is a
separate protocol slice. Physical phone/public soak, WebView2 heap evidence,
dual-window/Host singleton, branch identity, and full Kernel authority remain
external or open requirements as recorded in `docs/REQUIREMENTS-SPEC.md`.

