---
id: L3-AGENT-ACTIVITY-001
level: L3
title: Agent activity and acknowledged attention
status: LOCKED
parent: L2-AGENT-COMMUNE-001
code_targets:
  - src-tauri/src/commands/teammate.rs
  - src/lib/teammate/agentPaneHighlightSync.ts
  - src/lib/teammate/AgentPaneHighlightSync.svelte
  - src/lib/stores/paneTree.ts
test_targets:
  - src/lib/teammate/agentPaneHighlightSync.test.ts
---

# Agent activity and acknowledged attention

Output sequence or OSC title movement keeps an Agent working. Both signals must
remain stable for the idle window before completion attention appears. Clicking
the Agent card or focusing its pane acknowledges the exact status; repeated
snapshots cannot re-arm attention until the Agent changes status.
