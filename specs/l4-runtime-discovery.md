---
id: L4-RUNTIME-DISCOVERY-001
level: L4
title: Cache and consume one host process snapshot
status: DRAFT
parent: L3-RUNTIME-DISCOVERY-001
artifact: PSEUDOCODE
code_targets:
  - src-tauri/src/teammate/autodiscover.rs
test_targets:
  - src-tauri/src/teammate/autodiscover.rs
---

# Cache and consume one host process snapshot

```text
if cached host snapshot is absent or older than 500 ms: enumerate processes once
for each workspace query: match its pane roots against that shared snapshot
when Agent profile process names change: invalidate the snapshot immediately
test two different workspace consumers inside one TTL observe one enumeration
```
