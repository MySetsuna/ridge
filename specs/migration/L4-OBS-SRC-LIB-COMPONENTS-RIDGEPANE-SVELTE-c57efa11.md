---
id: L4-OBS-SRC-LIB-COMPONENTS-RIDGEPANE-SVELTE-c57efa11
level: L4
parent: L3-OBS-SRC-LIB-COMPONENTS-28f39e09
title: RidgePane 生命周期与启动恢复
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/components/RidgePane.svelte
  - src/lib/terminal/paneStartup.ts
test_targets:
  - src/lib/terminal/paneStartup.test.ts
  - src/lib/components/customTheme.test.ts
  - src/lib/components/explorerPaste.test.ts
  - src/lib/components/GitGraph.test.ts
  - src/lib/components/inputBufferTracker.test.ts
  - src/lib/components/interactionContracts.test.ts
  - src/lib/components/SettingsPanel.test.ts
  - src/lib/components/SidebarLazyMount.test.ts
  - src/lib/components/SplitContainer.test.ts
verified_by:
  - TEST-OBS-SRC-LIB-COMPONENTS-CUSTOMTHEME-TEST-TS-0b73284d
  - TEST-OBS-SRC-LIB-COMPONENTS-EXPLORERPASTE-TEST-TS-e29ef2f9
  - TEST-OBS-SRC-LIB-COMPONENTS-GITGRAPH-TEST-TS-69aabbd2
  - TEST-OBS-SRC-LIB-COMPONENTS-INPUTBUFFERTRACKER-TEST-TS-a37220a2
  - TEST-OBS-SRC-LIB-COMPONENTS-INTERACTIONCONTRACTS-TEST-TS-6ff9d90a
  - TEST-OBS-SRC-LIB-COMPONENTS-SETTINGSPANEL-TEST-TS-a77c4772
  - TEST-OBS-SRC-LIB-COMPONENTS-SIDEBARLAZYMOUNT-TEST-TS-a0bebf5d
  - TEST-OBS-SRC-LIB-COMPONENTS-SPLITCONTAINER-TEST-TS-1932765d
---

# RidgePane 生命周期与启动恢复

A desktop context menu opened while an inline TUI signal is live grants that pane a bounded host-interaction lease. Choosing a non-terminal-replacing menu action renews the lease and restores pane focus, so a visible TUI cursor or heuristic decay during menu use cannot route subsequent input through shell mode. Plain shell context menus never create the lease.

本地及 Remote 桌面窗格按明确 workspaceId/paneId 启动，依次确保 PTY 存在、注册快照并同步尺寸、激活；组件失效后不得继续激活。任何启动失败须呈现可操作错误，重试沿用同一身份与既有 PTY，不关闭或重复创建已运行终端，不重放键盘输入。启动中重复重试必须合并或禁用。历史预取不先于关键终端启动；失败不阻止终端使用。

验收以启动顺序、创建与订阅失败阻断后续激活、组件卸载隔离和相同身份重试的测试为准；真实公网补验终端输入、刷新、主机重连及原 child PID 保留。原有上下文菜单租约测试继续约束 TUI 输入归属。


本机前台进程查询保持 native-only 权限。浏览器 Remote 和原生界面中的外部主机窗格，不得启动本机进程轮询，也不得为消除拒绝错误扩大 Remote allowlist。原生本机活动窗格继续使用现有轮询与提示标记；Remote 沿用终端模式和协议提示状态。验收：公网 Remote 稳定交互期间不再每秒发送 get_pane_foreground_process，本机进程权限拒绝测试保持有效，原生本机调用仍可工作。
