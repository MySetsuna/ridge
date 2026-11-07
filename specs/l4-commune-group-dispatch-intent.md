---
id: L4-COMMUNE-GROUP-DISPATCH-INTENT
level: L4
title: Commune 组派发意图与回执恢复
status: LOCKED
lifecycle: ACTIVE
parent: L3-COMMUNE-PROTOCOL-MEMBERSHIP
code_targets:
  - src/lib/teammate/communeGroupDispatch.ts
  - src/lib/teammate/CommuneGroupTasks.svelte
  - src/lib/teammate/HostCommuneGroups.svelte
  - src/remote/lib/CommuneWorkspaceRoster.svelte
  - src/lib/teammate/communeUiText.ts
test_targets:
  - src/lib/teammate/communeGroupDispatch.test.ts
  - scripts/commune-permission-ui-acceptance.mjs
acceptance_checks:
  - id: group-intent-kernel-build
    command:
      - cargo
      - build
      - --locked
      - -p
      - ridge-kernel
      - --bin
      - ridge-kernel
    description: 构建现有组派发幂等与冻结目标的真实 Runtime
  - id: group-intent-fixture-build
    command:
      - cargo
      - test
      - --locked
      - -p
      - ridge-kernel
      - --test
      - commune_registration
      - --no-run
    description: 构建既有 ACP 会话 fixture
  - id: group-intent-ui
    command:
      - node
      - scripts/commune-permission-ui-acceptance.mjs
    description: 真实组任务在提交后回执丢失、输入新草稿和组件重挂后，确认原派发不重复执行并保留新输入；回归审批、取消和产物
depends_on: []
---

# Commune 组派发确认

## 契约

同一 Host 客户端、组作用域和组 ID 的未确认派发拥有一个内存请求编号。RPC 失败不等于尚未接收。主按钮再次确认时沿用未确认编号，不产生第二个逻辑派发。显式修订后的审定请求仍沿用该编号，若 Runtime 已接收另一正文或目标则按既有幂等冲突拒绝，禁止绕过冻结记录另建编号；原请求保留供明确重试确认。尚未接收的无效目标可修正后确认，由同一个既有 Runtime 幂等边界保证至多一份已接收请求。

原派发确认按钮重试完整原请求及原编号。连接恢复、重新渲染、返回页面均不自动派发。只有获得合法且匹配组、作用域和修订的回执后，才结束此编号并允许后续新任务使用新编号。防止并发点击产生两个请求。错误、输入和请求均不写 localStorage 或磁盘。

## 状态与界面

草稿、选择的精确成员代际与 lease、组修订、原请求和未确认状态按 Host 客户端隔离保留，组件重挂及普通链路重连后仍可确认原派发。后续输入不得被迟到 ACK 或原请求确认清除，只有当前草稿仍是该次已确认提交快照时才清空。切换组或 Host 不混用意图，旧作用域迟到结果只更新原作用域。组任务回执确认和后续刷新错误分开，刷新失败不允许重新生成编号。单 Runtime 的既有组幂等与冻结规则保持为执行保证。

## 验收

隔离单测验证双击、回执丢失、同编号修改被拒后仍可确认原请求、修订尚未接收的错误、原回执确认后的新输入保留、作用域/Host 隔离与组件使用同一状态恢复。真实 Chromium 拦截实际 Runtime 已提交后的回执，组件重挂后仍可重试，两个原 ACP 会话各执行一次，改写的新草稿保留。保留已有组修订变化、逐员分工、真实审批、取消和协议产物回归。fixture 与注入断线不冒充外部 Agent、真机或 WAN 验收。

显式修订保留对应请求快照和前一候选。修订回执丢失时确认最近候选；Runtime 明确返回既有幂等冲突时，恢复前一候选供用户再次确认，不自动派发。相同请求不叠加候选，所有候选沿用同一编号。覆盖先拒绝、再修订已接收但回执丢失的情况，避免始终重试已被修订的旧正文。
