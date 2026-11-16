---
id: L4-COMMUNE-UI-PERMISSION-ACCEPTANCE
level: L4
title: Commune 共用权限与派发 UI 验收
status: LOCKED
lifecycle: ACTIVE
parent: L3-SPEC-MAPPING-GOVERNANCE
code_targets:
  - scripts/commune-permission-ui-acceptance.mjs
test_targets:
  - scripts/commune-permission-ui-acceptance.mjs
acceptance_checks:
  - id: commune-permission-complete-ui
    command:
      - node
      - scripts/commune-permission-ui-acceptance.mjs
    description: 完整执行原脚本及全部断言；覆盖原有 group-intent-ui, online-group-ui,
      organization-dispatch-regression
depends_on:
  - L4-COMMUNE-GROUP-DISPATCH-INTENT
  - L4-COMMUNE-GROUP-ONLINE-ELIGIBILITY
  - L4-COMMUNE-ORGANIZATION-MEMBERS
---

# Commune 共用权限与派发 UI 验收

一轮实际执行一次现有完整脚本，不复用历史通过记录。任一依赖需求变化均须经 SpecTree 影响传播触发本单元。原功能脚本和断言保持。

- L4-COMMUNE-GROUP-DISPATCH-INTENT（原 group-intent-ui）：真实组任务在提交后回执丢失、输入新草稿和组件重挂后，确认原派发不重复执行并保留新输入；回归审批、取消和产物
- L4-COMMUNE-GROUP-ONLINE-ELIGIBILITY（原 online-group-ui）：真实连接断开后离线成员不能新选；已提交丢失回执的组派发仍能确认原请求，不重复执行；回归审批、组分工和产物
- L4-COMMUNE-ORGANIZATION-MEMBERS（原 organization-dispatch-regression）：审批、协议历史、旧工作区组兼容及 CHG-098/099 的失联回执确认保护
