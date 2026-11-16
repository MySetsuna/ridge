---
id: L4-COMMUNE-UI-ACCEPTANCE
level: L4
title: Commune 共用全量 UI 验收
status: LOCKED
lifecycle: ACTIVE
parent: L3-SPEC-MAPPING-GOVERNANCE
code_targets:
  - scripts/commune-global-ui-acceptance.mjs
test_targets:
  - scripts/commune-global-ui-acceptance.mjs
acceptance_checks:
  - id: commune-complete-ui
    command:
      - node
      - scripts/commune-global-ui-acceptance.mjs
    description: 完整执行现有生产组件与真实隔离 Kernel 的 Native/Remote、编组、历史、审批、重试、会话选择、共享服务入口及触控断言
depends_on:
  - L4-COMMUNE-GROUP-RESPONSIVE-STATE
  - L4-COMMUNE-GROUP-USABILITY
  - L4-COMMUNE-MOBILE-GROUP-NAVIGATION
  - L4-COMMUNE-ORGANIZATION-MEMBERS
  - L4-COMMUNE-REMOTE-DELIVERY
  - L4-COMMUNE-SHARED-DAEMON-BINDING
---

# Commune 共用全量 UI 验收

复用现有 scripts/commune-global-ui-acceptance.mjs，一轮验证完整执行一次。原脚本、全部断言和隔离 Runtime 保持原实现，不缓存验证结果。

## 影响关系

本单元依赖下列六个需求节点；任意节点变化须经 SpecTree 影响传播触发本单元。各需求保留原 test_targets 与产品验收正文，其他独立命令不合并。

- L4-COMMUNE-GROUP-RESPONSIVE-STATE（原 group-refresh-ui）：生产 Commune 组件在后台读取挂起时仍可编辑与提交，读取旧回包不能回退新投影，窄屏状态可读且既有编组、派发、审批和接入回归成立
- L4-COMMUNE-GROUP-USABILITY（原 commune-group-usability-ui）：真实 Kernel 和受控既有 ACP 会话上验证协作入口可见、跨来源成员选择、候选接入解释、已归组成员可见与原子移动、派发记录保留和窄屏可用性
- L4-COMMUNE-MOBILE-GROUP-NAVIGATION（原 commune-mobile-group-navigation）：真实隔离 Kernel 上的生产组件，验证首屏到编组与派发的路径、同名成员区分、触控区、状态和既有权限
- L4-COMMUNE-ORGANIZATION-MEMBERS（原 organization-global-ui）：Native/Remote 统一成员与候选先入组后接入、同名来源、成员移动与能力反馈
- L4-COMMUNE-REMOTE-DELIVERY（原 commune-remote-delivery-ui）：真实受控 ACP 进程及 Kernel 上验证手机单成员消息、丢回执同键重试、连点防重和新草稿保留，同时保持跨来源编组、历史与 Remote 权限边界
- L4-COMMUNE-SHARED-DAEMON-BINDING（原 shared-native-ui）：实际组件中显式共享服务启动、原有手机编组和会话接入回归

治理检查验证依赖、单一完整命令及节点关联，实施时再用 stc impact 核对六个入口均能到达此单元。通过声明只能来自本轮实际命令退出状态。
