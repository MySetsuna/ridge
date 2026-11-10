---
id: L4-REMOTE-WORKSPACE-PANE-CONTINUITY
level: L4
title: Remote 工作区元数据与终端选择连续性
status: LOCKED
lifecycle: ACTIVE
parent: L3-TERMINAL-VISUAL-INPUT-REPAIR
code_targets:
  - src/remote/MainApp.svelte
  - src/remote/lib/cloudRemote.ts
  - src/remote/lib/mobileRemoteUiState.svelte.ts
test_targets:
  - src/remote/MainApp.test.ts
  - src/remote/lib/cloudRemote.test.ts
  - src/remote/lib/mobileRemoteUiState.test.ts
---

工作区元数据刷新和推送须保持当前终端选择连续。Cloud 工作区适配器保留主机已有的 panes 快照。缺省 panes 表示未提供终端成员信息，消费方使用该工作区已确认的 pane 投影；尚无投影时，同工作区保留当前选择。显式空数组表示该工作区已无终端，须清空选择。不同工作区之间不得沿用旧 pane。有效成员投影到来时保留仍存在的当前 pane，否则选择首项或清空。

启动恢复、手动刷新和工作区推送使用同一选择规则。单元回归覆盖缺省与空列表的区别、先 pane 后工作区元数据的回包顺序、已关闭 pane 及跨工作区隔离；Cloud 适配器回归覆盖原快照字段保留。入口接线测试检查三个路径调用同一规则。公网真实浏览器以原有 Kernel 与两个终端验收刷新后仍可输入、尺寸切换后实际画面更新，运行全量 npm test、lint、typecheck 与 SpecTree verify。
