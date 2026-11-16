---
id: L4-COMMUNE-UI-MEMBERSHIP-ACCEPTANCE
level: L4
title: Commune 共用接入 UI 验收
status: LOCKED
lifecycle: ACTIVE
parent: L3-SPEC-MAPPING-GOVERNANCE
code_targets:
  - scripts/commune-permission-ui-acceptance.mjs
test_targets:
  - scripts/commune-permission-ui-acceptance.mjs
acceptance_checks:
  - id: commune-membership-complete-ui
    command:
      - node
      - scripts/commune-permission-ui-acceptance.mjs
      - --membership
    description: 完整执行原脚本及全部断言；覆盖原有 connection-ui, organization-membership-ui
depends_on:
  - L4-COMMUNE-CONNECTION-WIZARD
  - L4-COMMUNE-ORGANIZATION-MEMBERS
---

# Commune 共用接入 UI 验收

一轮实际执行一次现有完整脚本，不复用历史通过记录。任一依赖需求变化均须经 SpecTree 影响传播触发本单元。原功能脚本和断言保持。

- L4-COMMUNE-CONNECTION-WIZARD（原 connection-ui）：真实 Chromium 文件选择、预览、错误重试、取消与既有会话注册及移出，不新建替身会话
- L4-COMMUNE-ORGANIZATION-MEMBERS（原 organization-membership-ui）：真实 Runtime 的已有会话连接文件向导、鉴权与进程校验拒绝、修正重试和断开保留历史
