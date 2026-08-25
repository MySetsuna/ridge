---
id: L3-AGENT-HITL-001
level: L3
title: Agent human-in-the-loop audit bridge
status: LOCKED
parent: L2-AGENT-COMMUNE-001
depends_on:
  - L4-AGENT-ACTIVITY-001
  - L4-REMOTE-PERFORMANCE-GATE-001
code_targets:
  - src/lib/teammate/hitlAudit*
test_targets:
  - src/lib/teammate/hitlAudit*.test.ts
---

# Agent human-in-the-loop audit bridge

The HITL audit surface projects shared Remote audit records through Commune
models while leaving unrelated Agent cards and activity independent.
