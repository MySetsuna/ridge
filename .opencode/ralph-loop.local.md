---
active: true
iteration: 2
maxIterations: 50
sessionId: ses_pty_layering
---

PTY 数据流分层实现：

## 迭代 1 (Rust 后端)
- state.rs: 新增 PaneRegistry/PaneOutputSender/RemotePaneSub；删 broadcast；迁移 pty_delta_channels → registry
- lib.rs: 两处 fan-out 替换 broadcast.send → registry 遍历
- server.rs: subscribe → mpsc channel + subscribe-pane handler + force delta + full-reframe
- commands/terminal.rs: 迁移 register/unregister/get 到新 registry

## 迭代 2 (Frontend)
- wsRemote.ts: subscribePane()
- MainApp.svelte: 移除轮询，添加 activePaneId 订阅
- TerminalScreen.svelte: 移除 pane 过滤
- ptyBridge.ts: 新增 outputChannel

Completion promise: DONE

