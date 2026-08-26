---
id: L3-REMOTE-SMOOTHNESS-001
level: L3
title: Bounded Remote smoothness and lifecycle
status: LOCKED
parent: L2-REMOTE-EXPERIENCE-001
code_targets:
  - src/remote/MainApp.svelte
  - src/remote/lib/TerminalCanvas.svelte
  - src/remote/lib/paneFeedScheduler.ts
  - packages/remote/src/shared/terminal/manager.ts
  - packages/remote/src/shared/transport/**
test_targets:
  - src/remote/lib/paneFeedScheduler.test.ts
  - src/remote/lib/TerminalCanvas.test.ts
  - packages/remote/src/shared/**
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
