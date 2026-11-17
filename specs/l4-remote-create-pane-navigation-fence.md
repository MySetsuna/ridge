---
id: L4-REMOTE-CREATE-PANE-NAVIGATION-FENCE
level: L4
title: Remote 创建终端归属与迟到回包隔离
status: DRAFT
lifecycle: PLANNED
parent: L3-REMOTE-INTERACTION-RELIABILITY
code_targets:
  - src/remote/MainApp.svelte
  - src/remote/lib/WorkspaceTree.svelte
  - src/remote/lib/mobileRemoteUiState.svelte.ts
  - src/remote/lib/cloudRemote.ts
  - packages/remote/src/shared/transport/wsRemote.ts
test_targets:
  - src/remote/lib/mobileRemoteUiState.test.ts
  - src/remote/lib/cloudRemote.test.ts
  - packages/remote/src/shared/transport/wsRemote.behavior.test.ts
  - scripts/remote-create-navigation-acceptance.mjs
acceptance_checks:
  - id: remote-create-navigation
    command:
      - node
      - scripts/remote-create-navigation-acceptance.mjs
    description: 真实生产组件与隔离 LAN、Cloud：延迟回包、归属、后续选择、A→B→A、连接替换、失败及单次创建
  - id: remote-mobile-build
    command:
      - npm
      - run
      - build:remote:mobile
    description: 移动生产包与 PWA 验证
  - id: lint
    command:
      - npm
      - run
      - lint
    description: 仓库 lint
  - id: typecheck
    command:
      - npm
      - run
      - typecheck
    description: 仓库类型检查
depends_on:
  - L2-REMOTE-MOBILE-SEMANTIC-V2
---

# Remote 创建终端的工作区归属与迟到回包隔离

落实 L2-REMOTE-MOBILE-SEMANTIC-V2 已批准的完整 PaneRef 与导航 generation fence。当前 MainApp.handleCreatePane 与 WorkspaceTree.newPane 在 await createPane 后读取可变 activeWorkspaceId；精确原函数复现已证明 A 请求在切至 B 后将 paneA 与 workspaceB 配对。该复现是受控函数执行证据，不冒充真实浏览器、Kernel 或公网验收。

创建意图在请求发出前冻结连接身份、workspaceId 和选择代次。LAN 使用宿主现有 create-pane.workspaceId 参数，Cloud 同样传递冻结的 workspaceId；不得由请求完成时的宿主或浏览器活动工作区推断归属。成功结果须属于原工作区；缺少可确认目标、工作区消失或归属不一致时不得把新 pane 填入其他工作区，不创建替代工作区。保持既有 shell、cwd、独立终端、组与进程身份语义。

MainApp 空态创建按钮、工作区树新增终端入口共用同一隔离规则。无导航变化时成功选择原工作区新 pane；请求期间切换工作区、选择其他 pane、A→B→A 或更换连接后，迟到成功与失败都不得覆盖新选择或向新上下文显示旧错误。已成功创建的终端仍通过原工作区权威列表可见，不把丢弃导航等同于创建失败；不删除、重建或自动重复提交。无关元数据刷新不应伪装成用户导航。显式失败保留宿主原因，结束等待状态；重复点击最多提交一次。

验收须先保留修复前失败，再对真实生产组件和真实 LAN／Cloud 连接分别延迟创建回包，验证正常成功、切工作区、同工作区换 pane、A→B→A、连接替换、目标消失、业务拒绝和重复点击。检查完整 workspaceId+paneId、源工作区布局与实际 PTY 数量；成功仅创建一次，原 PTY 的 PID／标识不变，迟到错误不污染新目标。若浏览器自动化使用本机隔离网络，应明确其不证明物理手机或公网弱网。纯函数或 mock 测试只能覆盖隔离逻辑，不替代实际生产入口与两种传输验收。

执行 npm test、lint、typecheck、移动生产构建和上述隔离验收，经 stc verify 与 stc complete 闭环。CHG-133 的默认 pane 复用、cwd 与显式启动修复仍由原会话验收；本变更待其完成后实施，保留该轮实现，不并行覆盖同一文件。
