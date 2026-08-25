---
id: L3-RUNTIME-DISCOVERY-001
level: L3
title: Shared Agent process discovery snapshot
status: LOCKED
parent: L2-RUNTIME-QUALITY-001
code_targets:
  - src-tauri/src/teammate/autodiscover.rs
test_targets:
  - src-tauri/src/teammate/autodiscover.rs
---

# Shared Agent process discovery snapshot

One host-wide process table is reused across workspace queries for the bounded
TTL. Each workspace rematches its pane roots in memory; alternating workspace
activity cannot trigger one full system scan per workspace.
