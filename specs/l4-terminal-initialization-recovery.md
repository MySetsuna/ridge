---
id: L4-TERMINAL-INITIALIZATION-RECOVERY
level: L4
title: 终端初始化失败可恢复且不重建运行进程
status: DRAFT
lifecycle: PLANNED
parent: L3-TERMINAL-VISUAL-INPUT-REPAIR
code_targets:
  - src/lib/components/RidgePane.svelte
  - packages/ridge-kernel/src/client.rs
  - src/lib/i18n/messages/workspace.ts
test_targets:
  - scripts/cdp-renderer-retry.mjs
  - packages/ridge-kernel/src/client.rs
---

只读 load_terminal_font_faces 在本地 Kernel 命令通道使用 35 秒有限读时限，普通命令仍使用 5 秒。RidgePane 对渲染初始化做一次自动重试，持续失败显示可操作重试；重试沿原附着流程，保持工作区、pane 和 PTY 身份，不重启核心或终端。重复触发互斥，卸载取消后续附着。真实 native dev:cdp 注入字体请求失败后恢复，核对真实输入回显、同一 PTY 与无重复绑定；并运行真实 socket 超过默认 5 秒的字体回应回归。
