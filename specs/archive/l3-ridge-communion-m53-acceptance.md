---
id: L3-RIDGE-COMMUNION-M53-ACCEPTANCE
level: L3
title: M5.3 acceptance evidence
status: RETIRED
lifecycle: ARCHIVED
parent: L2-RIDGE-COMMUNION-M53
code_targets:
  - docs/archive/CHG-070/retired-communion/packages/ridge-acp/tests/m53_acceptance.rs
  - docs/archive/CHG-070/retired-communion/src/lib/agent-chat/productAcceptance.test.ts
test_targets: []
archived_at: 2026-11-06T11:16:19.635Z
archived_by_change: CHG-090
---

Execute scenarios A simple chat, B reviewed A→(B,C)→D with same-turn approval and structured LeadFinal, and C restart/resume without repeating completed tasks. Run headed Ridge Desktop when available and credentialed ACP providers when available; otherwise explicitly NOT_RUN. Require npm test, npm run lint, npm run typecheck, controlled Worker tests repeated 50 times with zero flakes, and SpecTree verify/complete. Report every final M5.3 verdict; never claim COMMUNION_PRODUCT_READY=YES with known semantic bugs or timing flakes.

## CHG-090 映射处置

CHG-070 及 L3-COMMUNE-PROTOCOL-MEMBERSHIP 已明确移除右侧 Communion 与自动启动 Staff 产品链。此旧 M5.3 契约归档，原文和验收要求保留用于历史追溯。当前 Commune 使用既有终端会话及 Host 全局协议成员。

`packages/ridge-acp/tests/m53_acceptance.rs` → `docs/archive/CHG-070/retired-communion/packages/ridge-acp/tests/m53_acceptance.rs`，Git 904729d46ae35a233e1e9f4d8139f957fd149d58 / blob de84b876a7b035e9ccc2ce8124fd28ce452fd44a

`src/lib/agent-chat/productAcceptance.test.ts` → `docs/archive/CHG-070/retired-communion/src/lib/agent-chat/productAcceptance.test.ts`，Git 904729d46ae35a233e1e9f4d8139f957fd149d58 / blob ad9c2fbd83978c5c1e095c9a66601fe769889b44

原始映射保留如下，历史归档仅用于溯源。

```json
{
  "codeTargets": [
    "packages/ridge-acp/tests/m53_acceptance.rs",
    "src/lib/agent-chat/productAcceptance.test.ts",
    "scripts/m53-product-acceptance.mjs",
    "docs/architecture/notes/M5.3-ACCEPTANCE.md",
    "vitest.config.ts",
    "packages/ridge-acp/src/bin/live_acp_agent.rs"
  ],
  "testTargets": [
    "packages/ridge-acp/tests/m53_acceptance.rs",
    "src/lib/agent-chat/productAcceptance.test.ts",
    "packages/ridge-acp/tests/e2e_acp_lifecycle.rs",
    "packages/ridge-acp/tests/e2e_communion_coordination.rs",
    "packages/ridge-acp/tests/e2e_session_snapshot_json.rs",
    "packages/ridge-acp/tests/m21_lifecycle_acceptance.rs",
    "packages/ridge-acp/tests/mobile_consumer_protocol.rs",
    "packages/ridge-acp/tests/real_acp_e2e.rs"
  ]
}
```
