# Iteration 136 — asynchronous kernel bootstrap

## Problem

The Tauri `setup` callback synchronously performed kernel detect-or-spawn,
health polling, and remote-host topology restoration. A stale or cold kernel
could therefore occupy the setup thread for the bounded eight-second startup
window, making the first page and Settings feel frozen.

## Change

`src-tauri/src/lib.rs` now schedules kernel bootstrap through
`tauri::async_runtime::spawn_blocking`. The WebView remains interactive while
filesystem/process/HTTP probes run. The host topology is restored only after a
successful result, and the kernel death watcher is installed after the endpoint
is healthy. Failure remains visible in structured logs and does not abort the
shell startup.

## Verification

- `cargo check --manifest-path src-tauri/Cargo.toml --lib --quiet` — exit 0;
  existing warnings only.
- `git diff --check` — exit 0.
- No release or publication made; daily release allowance remains unchanged.

## Remaining gates

This removes setup-thread blocking; it does not claim the separate external
WebView2 heap soak, physical phone/public Remote, dual-window, or full Tauri
AppState/PTY Kernel-authority evidence.
