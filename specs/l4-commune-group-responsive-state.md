---
id: L4-COMMUNE-GROUP-RESPONSIVE-STATE
level: L4
title: Commune 编组后台刷新与写入状态隔离
status: LOCKED
lifecycle: ACTIVE
parent: L3-COMMUNE-PROTOCOL-MEMBERSHIP
code_targets:
  - src/lib/teammate/teammateGroups.svelte.ts
  - src/lib/teammate/HostCommuneGroups.svelte
  - src/lib/i18n/messages/commune.ts
test_targets:
  - src/lib/teammate/teammateGroups.test.ts
  - scripts/commune-global-ui-acceptance.mjs
acceptance_checks:
  - id: group-refresh-races
    command:
      - pnpm
      - exec
      - vitest
      - run
      - src/lib/teammate/teammateGroups.test.ts
    description: 覆盖初次加载、后台读取中写入、过期读取与错误返回、写入并发拒绝、作用域切换、失败后显式重试及 legacy 迁移互斥
  - id: group-refresh-ui
    command:
      - node
      - scripts/commune-global-ui-acceptance.mjs
    description: 生产 Commune 组件在后台读取挂起时仍可编辑与提交，读取旧回包不能回退新投影，窄屏状态可读且既有编组、派发、审批和接入回归成立
depends_on:
  - L4-COMMUNE-ORGANIZATION-MEMBERS
---

# Commune 编组响应与状态反馈

用户要求实施后续迭代计划，包含 Commune 与手机端交互可用性。本变更修复当前后台读取与写入共用 busy 所致的操作间歇禁用，保持既有组织成员、协议身份、CAS、派发恢复和历史契约。

## 可观察行为
初次取得有效投影前，写操作保持禁用并显示加载状态。失败时提供明确错误和显式刷新入口，未知 revision 不得当成有效 revision 0 提交。已有有效投影后，后台刷新显示独立、低干扰状态，不清空表单、选择和最后确认内容，不禁用编辑、取消或正常提交。正在提交时只串行化写操作，仍可取消本地确认。所有实际写入仍使用最后确认 revision 经过 Runtime CAS。

## 并发与恢复
读取中开始写入后，先前读取及其失败不得覆盖写入结果、错误、revision 或状态。实际写入进行中至多一条，同一作用域重复刷新合并为一次后续读取。作用域或 Remote 连接身份切换使旧读取和写入回包失效，不能清除新作用域的加载状态。刷新失败保留已确认投影并展示可重试状态。写入失败继续按既有权威重读恢复，不自动重放写入。历史 workspace 的首次显式 legacy 迁移仍视作写入，在迁移结束前禁止竞争写入，不改变归档和数据保留语义。

## 界面
Native 与 Remote 共用 HostCommuneGroups，用现有设计 token、字体与组件密度呈现独立的初次加载、后台刷新、保存和错误反馈，提供中英文文案与 aria-live 状态。窄屏保持可读、键盘焦点可见及触摸操作尺寸，成员身份与来源、组长和编组能力继续可辨。

## 验收边界
单测须实际控制读取和写入 Promise 顺序，覆盖旧读取成功及失败、写入并发、作用域切换和 legacy 迁移。浏览器验收挂起生产组件的背景读取，执行真实 Kernel CAS 写入，再释放旧快照核对新组保持。使用隔离 Kernel 与协议 fixture，真实外部 Agent、手机和公网证据继续独立记录。
