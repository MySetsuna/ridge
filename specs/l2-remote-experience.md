---
id: L2-REMOTE-EXPERIENCE-001
level: L2
title: Remote terminal continuity and performance
status: LOCKED
parent: L1-PROJECT-001
depends_on:
  - L2-RUNTIME-QUALITY-001
code_targets:
  - src/remote/**
  - packages/remote/src/**
  - src-tauri/src/remote_host_impl.rs
test_targets:
  - src/remote/**
  - packages/remote/src/**
---

# Remote terminal continuity and performance

Remote input, pane switching, output delivery, rendering, and reconnection stay
responsive under bounded queues and retained resources. Desktop, host, relay,
and physical-device costs are measured separately before further optimization.
