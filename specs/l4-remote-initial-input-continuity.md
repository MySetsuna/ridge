---
id: L4-REMOTE-INITIAL-INPUT-CONTINUITY
level: L4
title: Remote 初始化键盘输入连续性与终端生命周期隔离
status: LOCKED
lifecycle: ACTIVE
parent: L3-TERMINAL-VISUAL-INPUT-REPAIR
code_targets:
  - src/remote/lib/TerminalCanvas.svelte
  - src/remote/lib/remoteInput.ts
test_targets:
  - src/remote/lib/TerminalCanvas.test.ts
  - src/remote/lib/remoteInput.test.ts
  - scripts/remote-startup-input-e2e.mjs
acceptance_checks:
  - id: initial-input-real-pty
    command:
      - node
      - scripts/remote-startup-input-e2e.mjs
    description: 构建真实移动 Remote 和 CLI，以隔离 Kernel/PTY 验证附着前后延迟输入完整到达且执行一次
---

Remote 初始化或恢复呈现时，已捕获的物理键盘与 IME 输入在附着、规范终端上下文或输入锚点未就绪期间保持有界、有序缓冲；文本按原字节保存，特殊键按意图保存并在就绪后的真实终端模式下编码。规范快照或锚点就绪主动推进缓冲，不依赖下一次用户按键或光标坐标改变。虚拟键、粘贴和鼠标的既有未就绪保护保持。

缓冲只属于该组件的明确 pane。已观察到终端 epoch 时，缓冲须绑定该生命周期；同 epoch 的新激活可继续，换代必须丢弃旧缓冲并显示未送达状态。超过 64 KiB、换代或卸载整批清空，不发送命令前缀或自动释放剩余尾部；错误须经用户显式关闭后允许重新输入。组件卸载后异步回调不得发送旧输入。

单元测试使用实际终端编码器覆盖先首字后剩余文本与回车、上下文缺省、同生命周期新激活、换代、溢出、重试及取消。真实浏览器经 LAN 正常鉴权使用真实 Kernel/PTY，以客户端测试替身延迟附着与上下文暴露，核对就绪前零实际命令字节、就绪后完整命令与精确一次落盘。该 LAN 测试不作为公网、真机或真实 Agent 验收。运行 npm test、lint、typecheck 与 SpecTree verify。
