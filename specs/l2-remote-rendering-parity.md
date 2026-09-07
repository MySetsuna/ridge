---
id: L2-REMOTE-RENDERING-PARITY
level: L2
title: Remote mobile terminal rendering and lifecycle parity
status: LOCKED
lifecycle: ACTIVE
parent: L1-PROJECT-001
code_targets:
  - packages/ridge-term/src/**
  - packages/remote/src/shared/terminal/**
  - src/lib/components/RidgePane.svelte
  - src/lib/components/FileEditor.svelte
  - src/lib/components/EditorWindow.svelte
  - src/lib/monaco/**
  - src-tauri/src/commands/terminal.rs
  - src-tauri/src/teammate/job_object.rs
  - src-tauri/src/remote_host_impl.rs
  - src/remote/**
  - scripts/verify-remote-pwa-build.mjs
  - package.json
test_targets:
  - packages/ridge-term/src/**
  - packages/remote/src/**/*.test.*
  - src/lib/**/*.test.*
  - src-tauri/src/**
  - src/remote/**/*.test.*
  - scripts/**/*.test.*
---

# Remote mobile terminal rendering and lifecycle parity

Remote Web shall retain controller-side browser font rendering and shall never request desktop font RPCs. Its glyph rasterizer shall apply the same deterministic terminal-cell post-processing as the desktop path: Unicode Block Elements U+2580-U+259F render with cell-aligned opaque shared boundaries at every device pixel ratio, while Box Drawing connectors meet adjacent cells and ordinary glyphs, emoji, and shade blocks retain native browser antialiasing. TUI keyboard ownership must survive host context-menu interactions and suppress shell-history UI until an explicit prompt, alternate-screen exit, or reset releases the lease. Editor popups shall retain the active Monaco theme. On Windows, PTY teardown shall terminate the complete assigned Job Object process tree with the existing PID-tree fallback only when job assignment or termination fails. Acceptance requires pure raster bitmap tests, Remote browser/device-scale visual seam coverage, workspace/pane identity switching coverage, TUI context-menu regression coverage, editor theme mount coverage, a real parent-child-grandchild Job Object teardown test on Windows, PWA artifact verification that excludes desktop font RPCs, and the repository test, lint, typecheck, SpecTree verification, and completion gates. Publishing is permitted only after the existing artifact domains pass ordinary TLS validation; no insecure certificate bypass is allowed.
