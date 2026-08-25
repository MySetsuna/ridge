---
id: L2-RUNTIME-QUALITY-001
level: L2
title: Runtime performance and resource ownership
status: LOCKED
parent: L1-PROJECT-001
code_targets:
  - src-tauri/src/**
  - packages/ridge-term/src/**
test_targets:
  - src-tauri/src/**
  - packages/ridge-term/src/**
---

# Runtime performance and resource ownership

Host scans, child processes, listeners, timers, queues, kernels, and renderers
have bounded concurrency, explicit ownership, cancellation, and deterministic
cleanup. Performance claims require matching runtime or test evidence.
