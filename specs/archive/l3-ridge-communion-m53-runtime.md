---
id: L3-RIDGE-COMMUNION-M53-RUNTIME
level: L3
title: Canonical workflow runtime hardening
status: RETIRED
lifecycle: ARCHIVED
parent: L2-RIDGE-COMMUNION-M53
code_targets:
  - docs/archive/CHG-070/retired-communion/packages/ridge-acp/src/worker.rs
  - docs/archive/CHG-070/retired-communion/packages/ridge-acp/src/workflow.rs
  - docs/archive/CHG-070/retired-communion/packages/ridge-acp/src/test_harness.rs
  - docs/archive/CHG-070/retired-communion/packages/ridge-acp/src/communion.rs
test_targets: []
archived_at: 2026-11-06T11:16:19.635Z
archived_by_change: CHG-090
---

Keep one canonical PlanStore and AgentSession. Live approvals continue the same turn without re-prompting. Lost Running/WaitingApproval restore as Interrupted and require explicit fresh-session Resume. Replace timing-flaky worker tests with controlled events/clock and repeat 50 times. Outcomes distinguish success, unsuccessful terminal, recoverable, and active. Aggregate all canonical task results, including restored/interrupted/blocked tasks; never silently discard persistence errors.

## CHG-090 映射处置

CHG-070 及 L3-COMMUNE-PROTOCOL-MEMBERSHIP 已明确移除右侧 Communion 与自动启动 Staff 产品链。此旧 M5.3 契约归档，原文和验收要求保留用于历史追溯。当前 Commune 使用既有终端会话及 Host 全局协议成员。

`packages/ridge-acp/src/worker.rs` → `docs/archive/CHG-070/retired-communion/packages/ridge-acp/src/worker.rs`，Git 904729d46ae35a233e1e9f4d8139f957fd149d58 / blob 89cb6da2c5ba14137d94e88e310ad8987acf7751

`packages/ridge-acp/src/workflow.rs` → `docs/archive/CHG-070/retired-communion/packages/ridge-acp/src/workflow.rs`，Git 904729d46ae35a233e1e9f4d8139f957fd149d58 / blob 3685bcdcafceaa4461f159bab127114e1d4d028d

`packages/ridge-acp/src/test_harness.rs` → `docs/archive/CHG-070/retired-communion/packages/ridge-acp/src/test_harness.rs`，Git 904729d46ae35a233e1e9f4d8139f957fd149d58 / blob 8278b0a706ca90215906ec507114122d98c0ad00

`packages/ridge-acp/src/communion.rs` → `docs/archive/CHG-070/retired-communion/packages/ridge-acp/src/communion.rs`，Git 904729d46ae35a233e1e9f4d8139f957fd149d58 / blob 6e1235bc655010f0474ed7f8ca1b2e55cfba56b4

原始映射保留如下，历史归档仅用于溯源。

```json
{
  "codeTargets": [
    "packages/ridge-acp/src/task.rs",
    "packages/ridge-acp/src/worker.rs",
    "packages/ridge-acp/src/workflow.rs",
    "packages/ridge-acp/src/test_harness.rs",
    "packages/ridge-acp/src/session.rs",
    "packages/ridge-acp/src/communion.rs",
    "packages/ridge-acp/src/bin/fake_acp_agent.rs",
    "packages/ridge-acp/src/prompt_loop.rs",
    "packages/ridge-acp/src/provider.rs"
  ],
  "testTargets": [
    "packages/ridge-acp/tests/m53_acceptance.rs"
  ]
}
```
