---
id: L4-AGENT-ACTIVITY-001
level: L4
title: Refresh and latch Agent activity
status: LOCKED
parent: L3-AGENT-ACTIVITY-001
artifact: PSEUDOCODE
code_targets:
  - src-tauri/src/commands/teammate.rs
  - src/lib/teammate/agentPaneHighlightSync.ts
  - src/lib/teammate/AgentPaneHighlightSync.svelte
  - src/lib/teammate/agentCommuneModel*
  - src/lib/teammate/teammateModel*
test_targets:
  - src/lib/teammate/agentPaneHighlightSync.test.ts
  - src/lib/components/SplitContainer.test.ts
---

# Refresh and latch Agent activity

```text
on output/title/layout event: refresh only the owning workspace
if output sequence or OSC title changed: status = working; restart expiry timer
if both stayed stable for 12 seconds: status = idle
if working -> idle/stopped/waiting: latch attention once
if card click or pane focus: clear attention and remember acknowledged status
while status equals acknowledged status: never re-arm attention
on any status transition: clear acknowledgement and permit a future latch
```
