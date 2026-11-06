---
id: L3-RIDGE-COMMUNION-M53-PRODUCT
level: L3
title: Communion sidebar and review product path
status: RETIRED
lifecycle: ARCHIVED
parent: L2-RIDGE-COMMUNION-M53
code_targets:
  - docs/archive/CHG-070/retired-communion/src/lib/agent-chat/AgentChatPanel.svelte
  - docs/archive/CHG-070/retired-communion/src/lib/agent-chat/DefaultWiredAgentChat.svelte
  - docs/archive/CHG-070/retired-communion/src/lib/agent-chat/AgentSessionPicker.svelte
  - docs/archive/CHG-070/retired-communion/src/lib/agent-chat/agentEvents.ts
  - docs/archive/CHG-070/retired-communion/src-tauri/src/bridge/acp_bridge.rs
  - docs/archive/CHG-070/retired-communion/src/lib/agent-chat/providerRestore.ts
test_targets: []
archived_at: 2026-11-06T11:16:19.635Z
archived_by_change: CHG-090
---

Wire existing chat into Ridge sidebar/popup. Simple Chat opens one AgentSession without Plan/Staff. Communion Lead proposes canonical PlanStore tasks for inline title/point/dependency/assignee review. Staff start only after explicit Confirm; Cancel review runs no Staff. Project canonical task/staff/tool/approval/handoff/artifact/diff/structured LeadFinal into the same continuous chat. Resume keeps Task identity, uses a fresh session/transport, and does not rerun completed tasks. Preserve Kernel/PTY/Remote/RTP1 boundaries.

## CHG-090 映射处置

CHG-070 及 L3-COMMUNE-PROTOCOL-MEMBERSHIP 已明确移除右侧 Communion 与自动启动 Staff 产品链。此旧 M5.3 契约归档，原文和验收要求保留用于历史追溯。当前 Commune 使用既有终端会话及 Host 全局协议成员。

`src/lib/agent-chat/AgentChatPanel.svelte` → `docs/archive/CHG-070/retired-communion/src/lib/agent-chat/AgentChatPanel.svelte`，Git 904729d46ae35a233e1e9f4d8139f957fd149d58 / blob cc38995c604e3aa877318447adaeb6397401356c

`src/lib/agent-chat/DefaultWiredAgentChat.svelte` → `docs/archive/CHG-070/retired-communion/src/lib/agent-chat/DefaultWiredAgentChat.svelte`，Git 904729d46ae35a233e1e9f4d8139f957fd149d58 / blob 439b4f1d30298213b873a26436e7ab4115914eab

`src/lib/agent-chat/AgentSessionPicker.svelte` → `docs/archive/CHG-070/retired-communion/src/lib/agent-chat/AgentSessionPicker.svelte`，Git 904729d46ae35a233e1e9f4d8139f957fd149d58 / blob 212a31aab9577a30833a1aca26b78a8db21d633e

`src/lib/agent-chat/agentEvents.ts` → `docs/archive/CHG-070/retired-communion/src/lib/agent-chat/agentEvents.ts`，Git 904729d46ae35a233e1e9f4d8139f957fd149d58 / blob 8e9c7feafc805d5407f6a29dff0c492b43b2ea57

`src-tauri/src/bridge/acp_bridge.rs` → `docs/archive/CHG-070/retired-communion/src-tauri/src/bridge/acp_bridge.rs`，Git 904729d46ae35a233e1e9f4d8139f957fd149d58 / blob 9e282dff344f162fa4d31626770f16cb8fdc56f1

`src/lib/agent-chat/providerRestore.ts` → `docs/archive/CHG-070/retired-communion/src/lib/agent-chat/providerRestore.ts`，Git 904729d46ae35a233e1e9f4d8139f957fd149d58 / blob 858b04e4a91fb0d9a0f3dd39b87d7386d46539e9

原始映射保留如下，历史归档仅用于溯源。

```json
{
  "codeTargets": [
    "src/lib/agent-chat/AgentChatPanel.svelte",
    "src/lib/agent-chat/DefaultWiredAgentChat.svelte",
    "src/lib/agent-chat/AgentSessionPicker.svelte",
    "src/lib/agent-chat/agentEvents.ts",
    "src-tauri/src/bridge/acp_bridge.rs",
    "src-tauri/src/lib.rs",
    "src/routes/+layout.svelte",
    "src/lib/agent-chat/providerRestore.ts",
    "src-tauri/Cargo.toml"
  ],
  "testTargets": [
    "src/lib/agent-chat/productAcceptance.test.ts",
    "src/lib/agent-chat/listenerLifecycle.test.ts",
    "src/lib/agent-chat/providerRestore.test.ts"
  ]
}
```
