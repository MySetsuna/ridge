# Iteration 93 - Git request-slot propagation

Date: 2026-08-03  
Status: runtime slice complete; versioned publication required

## Scope

Iteration 92 stopped a canceled Remote `data-request` task and reclaimed the
`git_status` child. This slice closes the remaining gap for Git reads and
mutations dispatched by the same legacy Remote path. Public Git function
signatures remain unchanged; cancellation context is carried internally.

## Implemented contract

- `with_git_request_slot` starts one generation for the whole async request.
- The task-local context carries `(slot, generation)` through every existing
  async Git helper that uses `spawn_git_blocking`.
- Nested Git operations reuse that generation. A cancellation therefore makes
  every later step stale instead of opening a new generation after cancel.
- Explicit slotted calls (including `get_scm_status`) keep latest-win behavior
  outside a request scope and reuse the ambient generation inside one.
- Generation leases are released on normal completion, cancellation, and task
  unwind; idle slot registry entries are removed, preventing one-entry-per-
  request growth on a long-lived Remote connection.
- The existing process guard still owns wall-clock timeout, semaphore release,
  and process-tree kill for every real Git child.

## Files

- `packages/ridge-core/src/commands/git.rs`
- `src-tauri/src/remote_host_impl.rs`

## Verification

- `cargo test -p ridge-core commands::git --lib --quiet`: 36 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml remote_host_impl::jsonrpc_tests --lib -- --nocapture`: 11 passed.
- New deterministic tests cover a real hanging child, registry cleanup after
  completion/cancel, and cancellation between two sequential Git steps.

## Limits and follow-up

This slice covers the legacy Remote `data-request` protocol only. The separate
JSON-RPC `invoke-request` path still needs protocol-level task ownership and
host-side cancellation. Physical phone/PWA/WebView2 heap, public WebRTC and
authenticated Git, dual-window workspace singleton, production branch identity,
and full Kernel-authority evidence remain open external gates.
