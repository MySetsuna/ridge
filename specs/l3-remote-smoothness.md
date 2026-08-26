---
id: L3-REMOTE-SMOOTHNESS-001
level: L3
title: Bounded Remote smoothness and lifecycle
status: LOCKED
parent: L2-REMOTE-EXPERIENCE-001
code_targets:
  - src/remote/App.svelte
  - src/remote/MainApp.svelte
  - src/remote/lib/TerminalCanvas.svelte
  - src/remote/lib/paneFeedScheduler.ts
  - packages/remote/src/shared/terminal/manager.ts
  - packages/remote/src/shared/transport/**
  - Cargo.lock
  - packages/ridge-core/Cargo.toml
  - packages/ridge-core/src/lib.rs
  - packages/ridge-core/src/terminal_font.rs
  - packages/ridge-core/src/dispatch.rs
  - packages/ridge-cli/src/tui/lan_host_impl.rs
  - packages/ridge-cli/src/kernel_host_impl.rs
  - src-tauri/Cargo.toml
  - src-tauri/src/commands/terminal_font.rs
test_targets:
  - src/remote/lib/paneFeedScheduler.test.ts
  - src/remote/lib/TerminalCanvas.test.ts
  - packages/remote/src/shared/**
  - scripts/mobile-keyboard-e2e.mjs
---

# Bounded Remote smoothness and lifecycle

Active input and control outrank history/render work. Output queues, replay,
timers, listeners, transports, and terminal instances remain bounded and are
disposed exactly once. Lab evidence cannot substitute for public-network and
physical-device soak evidence.

On mobile attach, the pane-box grid must be claimed once after its Host resize
callback is bound, even when an earlier local renderer fit already chose the
same dimensions. Remote-owned claims size the browser kernel before resizing
the Host PTY, so TUI redraw bytes are parsed against one canonical grid and do
not wrap into scrollback.

Every resize acknowledgement preserves the initiating endpoint as `owner`.
Missing or invalid owner data defaults to `host`; receivers apply the canonical
grid without echoing it. Production mobile evidence must exercise a real Host
PTY and prove cursor-addressed primary-screen redraws leave local scrollback and
viewport offset unchanged after the Remote-owned grid claim settles.

The LAN mobile entry attaches its authenticated RPC bridge before rendering the
terminal surface, so Host font discovery cannot race terminal initialization.
Desktop, Cloud, and both CLI Host paths serve the same bounded system-font
resolver and chunk cache from `ridge-core`.
