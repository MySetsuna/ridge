---
id: L4-COMMUNE-REMOTE-DELIVERY
level: L4
title: Remote 单成员消息确认与安全重试
status: LOCKED
lifecycle: ACTIVE
parent: L3-COMMUNE-PROTOCOL-MEMBERSHIP
code_targets:
  - packages/remote/src/shared/transport/wsRemote.ts
  - src/remote/lib/cloudRemote.ts
  - src/remote/lib/CommuneWorkspaceRoster.svelte
  - src/remote/lib/CommuneMemberMessage.svelte
  - src/remote/lib/communeMemberDelivery.ts
  - src/lib/teammate/communeUiText.ts
test_targets:
  - src/remote/lib/communeMemberDelivery.test.ts
  - src/remote/lib/cloudRemote.test.ts
  - packages/remote/src/shared/transport/wsRemote.behavior.test.ts
  - scripts/commune-global-ui-acceptance.mjs
  - src/lib/stores/hosts.connect.test.ts
acceptance_checks:
  - id: commune-remote-delivery-ui
    command:
      - node
      - scripts/commune-global-ui-acceptance.mjs
    description: 真实受控 ACP 进程及 Kernel 上验证手机单成员消息、丢回执同键重试、连点防重和新草稿保留，同时保持跨来源编组、历史与 Remote 权限边界
depends_on: []
---

# Remote 单成员消息确认与安全重试

## 批准依据

用户要求实现审核后的 Commune 与 Remote 迭代。本单元落实既有精确会话、幂等投递、未知结果与协议取消契约。

## 行为

一次发送意图冻结连接、workspace、pane、member、generation、lease、正文和请求键。提交期间重复点击或 Enter 不产生第二次提交。回执丢失保留原意图，用户重试复用原键及原正文，不自动重发。新草稿独立保留，迟到确认只清除仍与所提交快照相同的草稿。

消息状态在当前 RemoteLink 生命周期内按精确目标隔离，切换来源或组件重挂载保留未确认意图，代际变化不向新会话重放旧请求。页面重载后的跨会话恢复不由本单元新增持久化。

LAN 和 Cloud 使用相同调用契约，保留原有两参数发送调用。能力缺失、离线或无精确绑定时禁止发起新请求。展示真实回执并允许只读刷新，执行状态与投递受理分开。取消携带原 deliveryId 和精确身份，取消请求成功后等待真实终态。读取失败保留最后回执并提示错误。

## 验收

覆盖双击/Enter、已提交后丢回执、重试固定键、发送中继续编辑、同文的新发送使用新键、未确认状态重挂载、来源和运行代际隔离、查询失败与取消结果未知。传输测试分别证明 LAN/Cloud 请求字段、状态查询和取消透传。受控真实 ACP 进程验证逻辑消息仅执行一次，不把本机窄屏证据扩充为真实手机或 Cloud/WAN 验收。

## 门禁隔离

Host 连接单测必须使用受控共享列表，模拟鉴权不能触发外部 Cloud HTTP。保持原 5 秒超时及业务断言。
