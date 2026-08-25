---
id: L2-AGENT-COMMUNE-001
level: L2
title: Agent coordination and Commune
status: LOCKED
parent: L1-PROJECT-001
depends_on:
  - L4-AGENT-ACTIVITY-001
code_targets:
  - src/lib/teammate/**
  - src-tauri/src/commands/teammate.rs
test_targets:
  - src/lib/teammate/**
---

# Agent coordination and Commune

Projects live Agent state into pane chrome and Agent's Commune while keeping
attention acknowledgement, intervention, and recent replies coherent.
