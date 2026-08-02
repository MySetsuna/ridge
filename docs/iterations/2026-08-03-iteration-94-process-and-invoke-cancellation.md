# Iteration 94 - Process-tree and invoke cancellation closure

Date: 2026-08-03  
Status: runtime slice complete; versioned publication required

## Scope

This slice closes two cancellation gaps left after the Remote `data-request`
work: detached Unix descendants could outlive a timed-out Git child, and the
browser desktop bridge could time out locally while the Host continued an
`invoke-request` or native JSON-RPC call.

## Implemented contract

- Unix guarded commands create a dedicated process group before `exec`.
- Timeout and cancellation signal the whole group (TERM then KILL), with a
  PID fallback only when group signalling is unavailable. Windows keeps its
  `taskkill /T` tree path.
- Remote Host owns legacy `invoke-request` and native JSON-RPC tasks in one
  bounded per-connection registry (32 entries), with duplicate rejection,
  bounded pre-cancel tombstones, exact-once completion, and disconnect cleanup.
- `$/cancel` and legacy `invoke-cancel` remove task ownership before aborting;
  raced results are dropped. Browser timeout, AbortSignal, pane-scope teardown,
  and disposal send one wire cancellation when the transport is open.
- Native synchronous core Git dispatch installs the request's existing
  `(slot, generation)` on its blocking thread, so cancellation cannot reopen a
  fresh generation after the request has already been canceled.

## Files

- `packages/ridge-core/Cargo.toml`
- `Cargo.lock`
- `packages/ridge-core/src/process_guard.rs`
- `packages/ridge-core/src/commands/git.rs`
- `src-tauri/src/remote_host_impl.rs`
- `packages/remote/src/shared/transport/wsRemote.ts`
- `packages/remote/src/shared/transport/wsRemoteRpcScheduler.test.ts`

## Verification

- Windows `cargo test -p ridge-core process_guard::tests --lib`: 2 passed.
- WSL Ubuntu `cargo test -p ridge-core process_guard::tests --lib`: 3 passed,
  including real shell + `sleep` descendant reclamation.
- `cargo test -p ridge-core commands::git --lib --quiet`: 37 passed.
- Host JSON-RPC tests: 12 passed.
- Remote scheduler/adapter tests: 26 passed.
- `pnpm check`: 0 errors, 0 warnings.
- `git diff --check`: clean.

## Limits and follow-up

The registry covers the LAN/WebSocket Host path. Public Cloud/WebRTC and
physical phone/PWA/WebView2 soak evidence still requires the external devices
and environments. Dual-window workspace singleton, production branch identity,
and full Kernel-authority evidence remain open gates.
