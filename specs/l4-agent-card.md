---
id: L4-AGENT-CARD-001
level: L4
title: Render one Agent member card
status: DRAFT
parent: L3-AGENT-CARD-001
artifact: PSEUDOCODE
code_targets:
  - src/lib/teammate/AgentMemberRow.svelte
test_targets:
  - src/lib/components/SplitContainer.test.ts
---

# Render one Agent member card

```text
render large identity and live state header
render latest reply as the card's persistent primary body
render task and approval only when present
render message composer and accessible action controls
acknowledge attention on card focus/pointer interaction
```
