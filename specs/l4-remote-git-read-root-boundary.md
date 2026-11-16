---
id: L4-REMOTE-GIT-READ-ROOT-BOUNDARY
level: L4
title: Git 读取与发现遵守实际仓库根授权
status: DRAFT
lifecycle: PLANNED
parent: L3-REMOTE-INTERACTION-RELIABILITY
code_targets:
  - packages/ridge-core/src/dispatch.rs
  - .github/workflows/rtp1-kernel-e2e.yml
test_targets:
  - packages/ridge-core/src/dispatch.rs
  - packages/ridge-kernel/tests/git_commands_runtime.rs
  - .github/workflows/rtp1-kernel-e2e.yml
---

有界 RootScope 下，Git 读、状态和仓库发现必须核对实际向上找到的仓库根；参数虽在 roots 内，但实际仓库位于 roots 外，返回 outside_sandbox，不能借子目录读父仓库。共同 admission 约束 Desktop scoped、LAN 与 Cloud 等调用方；无界本机行为及授权根内真实仓库的子目录访问保持不变。回归覆盖真实父仓库拒绝、根内仓库允许和非 Git 空目录，不执行越界写入。 CI 必须执行共同 admission 回归族和真实 Git Runtime 回归，不能只把测试写在仓库而未接入现有 Kernel 工作流。
