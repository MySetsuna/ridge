---
id: L3-RIDGE-COMMUNION-M53-PRODUCT
level: L3
title: Communion sidebar and review product path
status: LOCKED
lifecycle: ACTIVE
parent: L2-RIDGE-COMMUNION-M53
code_targets:
  - src/lib/agent-chat/AgentChatPanel.svelte
  - src/lib/agent-chat/DefaultWiredAgentChat.svelte
  - src/lib/agent-chat/AgentSessionPicker.svelte
  - src/lib/agent-chat/agentEvents.ts
  - src-tauri/src/bridge/acp_bridge.rs
  - src-tauri/src/lib.rs
  - src/routes/+layout.svelte
  - src/lib/agent-chat/providerRestore.ts
  - src-tauri/Cargo.toml
test_targets:
  - src/lib/agent-chat/productAcceptance.test.ts
  - src/lib/agent-chat/listenerLifecycle.test.ts
  - src/lib/agent-chat/providerRestore.test.ts
---

Wire existing chat into Ridge sidebar/popup. Simple Chat opens one AgentSession without Plan/Staff. Communion Lead proposes canonical PlanStore tasks for inline title/point/dependency/assignee review. Staff start only after explicit Confirm; Cancel review runs no Staff. Project canonical task/staff/tool/approval/handoff/artifact/diff/structured LeadFinal into the same continuous chat. Resume keeps Task identity, uses a fresh session/transport, and does not rerun completed tasks. Preserve Kernel/PTY/Remote/RTP1 boundaries.
