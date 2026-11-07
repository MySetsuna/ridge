---
id: L4-COMMUNE-GROUP-ONLINE-ELIGIBILITY
level: L4
title: Commune 组派发的在线资格与离线确认
status: LOCKED
lifecycle: ACTIVE
parent: L3-COMMUNE-PROTOCOL-MEMBERSHIP
code_targets:
  - src/lib/teammate/CommuneGroupTasks.svelte
test_targets:
  - scripts/commune-permission-ui-acceptance.mjs
acceptance_checks:
  - id: online-kernel-build
    command:
      - cargo
      - build
      - --locked
      - -p
      - ridge-kernel
      - --bin
      - ridge-kernel
    description: 构建当前真实 Runtime
  - id: online-fixture-build
    command:
      - cargo
      - test
      - --locked
      - -p
      - ridge-kernel
      - --test
      - commune_registration
      - --no-run
    description: 构建连接可断开但原进程保活的已有 ACP fixture
  - id: online-group-ui
    command:
      - node
      - scripts/commune-permission-ui-acceptance.mjs
    description: 真实连接断开后离线成员不能新选；已提交丢失回执的组派发仍能确认原请求，不重复执行；回归审批、组分工和产物
depends_on: []
---

# Commune 组派发在线资格

新派发的成员选择与既有单成员发送入口一致，须 online 为 true、调用者 canSend 为 true，且存在精确 generation 和 lease。缺省在线状态按不可用处理。离线成员继续在编组中呈现，保持所属组、历史、名称及离线说明，选择控件禁用。原已选成员离线使当前草稿选择失效，禁止新确认，仍保留草稿供刷新后重新审定。

此条件只约束新派发资格。CHG-098 已提交但回执丢失的原请求保留同一编号和冻结目标，原会话随后断开时仍允许用户明确确认原请求，不能因离线隐藏或清除其未确认状态。Runtime 沿既有组幂等记录返回原回执，不建立新编号或向 Agent 重新发送。原进程保活、历史与未知结果不改变。

验收使用实际 Kernel 与原已有 ACP 连接。主动使一个 fixture 断开连接，核对其 canSend 方法权限仍 true、generation/lease 仍在而 online false，组选择器必须禁用。再用另一成员产生真实已提交组任务，注入丢失回执并使其原连接断开，重新挂载后原请求确认仍可达，两个确认保持同一编号，原进程日志仅有一次执行。fixture 不证明用户实际 Agent 或手机网络表现。
