---
id: L2-TUI-MOBILE-PTY-LIFECYCLE
level: L2
title: TUI mobile terminal and PTY lifecycle hardening
status: LOCKED
lifecycle: ACTIVE
parent: L1-PROJECT-001
code_targets:
  - packages/ridge-term/src/**
  - packages/remote/src/**
  - src/lib/components/RidgePane.svelte
  - src/lib/components/FileEditor.svelte
  - src/lib/components/EditorWindow.svelte
  - src/lib/monaco/**
  - src/lib/stores/themes.ts
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

# TUI mobile terminal and PTY lifecycle hardening

Terminal UI keyboard ownership shall remain active through host context-menu interactions and shall not release based on an elapsed timer. A terminal that has emitted a durable inline-TUI signal suppresses Ridge shell-history UI until an explicit terminal prompt, alternate-screen exit, or reset signal releases the lease. Mouse-reporting terminals receive pointer input without browser context-menu leakage. Monaco editor theme selection shall use the active theme catalogue metadata and initialize only after the popup theme system is ready. On Windows, each PTY job uses kill-on-close semantics and explicit job termination so pane teardown reclaims its complete process tree, with the existing process-tree fallback retained when a job cannot be used. Remote/mobile artifacts must not request desktop-only terminal font RPCs; legacy requests fail immediately with a refresh-required error, and the PWA exposes an available update while applying it automatically only after backgrounding. Acceptance requires terminal gate and parser tests, remote input and PWA artifact checks, Monaco custom-theme coverage, PTY teardown coverage where platform support is available, and repository test, lint, typecheck, SpecTree verification, and completion gates.
