---
id: L3-OBS-OPENCODE-5e2dd251
level: L3
parent: L2-OBS-OPENCODE-5e2dd251
title: .opencode module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - .opencode/opencode.json
test_targets: []
---

# .opencode module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.

## CHG-131 映射处置

保留 Git 已跟踪的 .opencode/opencode.json 配置源及其活子节点；移除本机 package.json 映射。原节点完整内容按 Git blob 保存在 `docs/archive/CHG-131/observations/L3-OBS-OPENCODE-5e2dd251.md`。Tauri 权限及 MCP 打包仍由现行源码与正式 Runtime 契约承接。未迁入本机环境文件、凭据或工具私有配置。
