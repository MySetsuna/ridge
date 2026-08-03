# Iteration 135 — Settings stall and cross-surface blocking offload

Date: 2026-08-03  
Status: code-green; native WebView2/GPU and long-run heap soak remain external gates.

## Root cause

Opening Settings was not one isolated slow call. The Terminal tab eagerly
started shell/WSL/Visual Studio discovery, while the discovery path could spawn
child processes synchronously. The overlay also applied a full-window backdrop
blur over live terminal surfaces. In parallel, unrelated setting writes could
re-submit default-CWD and terminal-theme work.

The audit found the same blocking shape outside Settings: several Tauri
commands were declared `async` but performed PTY resize, parser reframe,
scrollback copying, clear-frame encoding, shell history, or shell replacement
before their first real suspension. Migrated `ridge_core::dispatch` calls on
the remote WebSocket executor had the same risk. Native-size wallpaper decode
could additionally duplicate a large image in WebView2 memory.

## Changes

- `src/lib/components/SettingsPanel.svelte`: defer shell and Agent discovery to
  idle time and the selected tab; add generation/lifecycle guards; use lazy
  preview images; remove full-window blur; keep preview derivation linear.
- `src/routes/+page.svelte`, `src/lib/terminal/hostPorts.ts`,
  `packages/remote/src/shared/terminal/{ports,themeBridge}.ts`: latest-value
  default-CWD sync, unrelated-setting dedupe, theme-aware snapshots, and one
  pending theme bridge frame per animation frame.
- `packages/ridge-core/src/commands/shell.rs` and
  `src-tauri/src/commands/terminal.rs`: shell probes use the shared 2-second
  process-tree timeout; PTY creation/replacement, resize, clear, delta-mode
  reframe, shell history, scrollback paging, and resync-frame construction run
  in `spawn_blocking`; scrollback IPC responses are capped at 512 KiB.
- `src-tauri/src/remote_host_impl.rs`: all migrated core dispatches, not only
  Git, leave the remote WebSocket executor through `spawn_blocking` while
  preserving legacy and JSON-RPC error envelopes.
- `src/lib/stores/themes.ts`: wallpaper decode waits for an idle window,
  suppresses stale generations, and clamps decoded images to 4096 px per edge
  and 16 MP. `themes.slug.test.ts` covers the size bound.

## Verification

- `pnpm check`: 0 errors, 0 warnings.
- Full Vitest: 147 files, 1518 passed, 1 skipped.
- Tauri library tests: 251 passed, 0 failed.
- Tauri library `cargo check`: passed; remaining output is pre-existing
  unused/dead-code warnings.
- Iteration preflight, requirements gate, and write-scope iteration gate:
  passed; no pending requirement IDs.
- Existing LAN desktop/mobile and mobile keyboard/selection E2E evidence stays
  green; clean-profile `runtime.lastError` attribution remains external and is
  not masked in product code.

## Remaining gates

This iteration does not claim physical WebView2 first-contentful timing, GPU
adapter stability, multi-hour heap/scrollback soak, or public Remote/Cloud
deployment. No version bump or release was made because `v0.1.54` consumed the
publication allowance for 2026-08-03.

