---
id: L3-AGENT-CARD-001
level: L3
title: Reply-first Agent card experience
status: DRAFT
parent: L2-AGENT-COMMUNE-001
depends_on:
  - L3-AGENT-ACTIVITY-001
code_targets:
  - src/lib/teammate/AgentCenterPanel.svelte
  - src/lib/teammate/AgentMemberRow.svelte
  - src/routes/+page.svelte
test_targets:
  - src/lib/components/SplitContainer.test.ts
  - tests/e2e/commune.spec.ts
---

# Reply-first Agent card experience

Agent cards favor identity, current state, and the latest native JSONL reply.
Cards and type remain readable in a wider Commune sidebar; secondary metadata
and controls stay compact, keyboard-visible, and available without hiding the
latest reply behind disclosure.
