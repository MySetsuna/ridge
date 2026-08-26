---
id: L2-HOST-INTEGRATION-001
level: L2
title: Host integration surfaces
status: LOCKED
parent: L1-PROJECT-001
depends_on:
  - L2-AGENT-COMMUNE-001
  - L3-AGENT-HITL-001
  - L4-REMOTE-PERFORMANCE-GATE-001
code_targets:
  - src/lib/hosts/**
  - src/lib/remote/cloud/cloudControllerBoot*
  - src/lib/remote/cloud/cloudHostTopologyLink*
  - src/lib/remote/cloud/sharedWorkspaceProjection*
  - src/lib/stores/gitGuardStats*
  - src/lib/stores/hostReconnect*
  - src/lib/stores/hosts*
  - src/lib/stores/processGuardPolicy*
  - src-tauri/src/commands/terminal_font.rs
test_targets:
  - src/lib/hosts/**/*.test.ts
  - src/lib/remote/cloud/cloudControllerBoot.test.ts
  - src/lib/remote/cloud/cloudHostTopologyLink.test.ts
  - src/lib/remote/cloud/sharedWorkspaceProjection.test.ts
  - src/lib/stores/gitGuardStats.test.ts
  - src/lib/stores/hostReconnect.test.ts
  - src/lib/stores/hosts*.test.ts
  - src/lib/stores/processGuardPolicy.test.ts
---

# Host integration surfaces

Desktop host composition, reconnect, and outbound lifecycle bridges depend on
Agent coordination and the shared Remote transport contracts.
Host font resolution supplies a bounded, authenticated set of installed font
bytes to the shared terminal renderer. Color-emoji families are retained ahead
of lower-priority fallback faces; Remote and Cloud controllers need neither
local font access nor font installation.
