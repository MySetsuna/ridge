---
id: L2-REMOTE-RENDERING-PARITY
level: L2
title: Remote mobile terminal rendering and lifecycle parity
status: LOCKED
lifecycle: ACTIVE
parent: L1-PROJECT-001
code_targets:
  - packages/ridge-term/src/**
  - packages/remote/src/shared/terminal/**
  - src/lib/components/RidgePane.svelte
  - src/lib/components/FileEditor.svelte
  - src/lib/components/EditorWindow.svelte
  - src/lib/monaco/**
  - src/remote/**
  - scripts/verify-remote-pwa-build.mjs
  - package.json
  - packages/ridge-kernel/src/pty.rs
  - packages/ridge-kernel/src/host.rs
  - packages/ridge-core/src/process_guard.rs
  - packages/ridge-core/src/process_identity.rs
test_targets:
  - packages/ridge-term/src/**
  - packages/remote/src/**/*.test.*
  - src/lib/**/*.test.*
  - src-tauri/src/**
  - src/remote/**/*.test.*
  - scripts/**/*.test.*
  - packages/ridge-kernel/tests/owned_pty_process_tree.rs
  - packages/ridge-kernel/tests/terminal_ops_runtime.rs
---

# Remote mobile terminal rendering and lifecycle parity

Remote Web shall retain controller-side browser font rendering and shall never request desktop font RPCs. Its glyph rasterizer shall apply the same deterministic terminal-cell post-processing as the desktop path: Unicode Block Elements U+2580-U+259F render with cell-aligned opaque shared boundaries at every device pixel ratio, while Box Drawing connectors meet adjacent cells and ordinary glyphs, emoji, and shade blocks retain native browser antialiasing. TUI keyboard ownership must survive host context-menu interactions and suppress shell-history UI until an explicit prompt, alternate-screen exit, or reset releases the lease. Editor popups shall retain the active Monaco theme. On Windows, PTY teardown shall terminate the complete assigned Job Object process tree with the existing PID-tree fallback only when job assignment or termination fails. Acceptance requires pure raster bitmap tests, Remote browser/device-scale visual seam coverage, workspace/pane identity switching coverage, TUI context-menu regression coverage, editor theme mount coverage, a real parent-child-grandchild Job Object teardown test on Windows, PWA artifact verification that excludes desktop font RPCs, and the repository test, lint, typecheck, SpecTree verification, and completion gates. Publishing is permitted only after the existing artifact domains pass ordinary TLS validation; no insecure certificate bypass is allowed.

## CHG-090 映射处置

原需求正文保留。CHG-069/070 后终端生产及 Host 分发归 Kernel，旧桌面命令/parser/PTY 映射退出现役。进程树销毁归 PTY 与 ridge-core process_guard/process_identity，owned_pty_process_tree 验收；输入/终端操作、快照历史、事件及背压分别由所列测试核验。现有纯前端渲染、输入与主题测试继续适用。

`src-tauri/src/commands/terminal.rs` → `docs/archive/CHG-090/source/src-tauri/src/commands/terminal.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 15029d6855a7c6d1aba44e8a33a5d3d82961a7bd

`src-tauri/src/teammate/job_object.rs` → `docs/archive/CHG-070/desktop-retired-tree/teammate/job_object.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob cda64112ca16d80bddcf567545390e931db606b3

`src-tauri/src/remote_host_impl.rs` → `docs/archive/CHG-090/source/src-tauri/src/remote_host_impl.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 4494aeb7dec320fa924d5ddcb45ff6e29705efb1

原始映射保留如下，历史归档仅用于溯源。

```json
{
  "codeTargets": [
    "packages/ridge-term/src/**",
    "packages/remote/src/shared/terminal/**",
    "src/lib/components/RidgePane.svelte",
    "src/lib/components/FileEditor.svelte",
    "src/lib/components/EditorWindow.svelte",
    "src/lib/monaco/**",
    "src-tauri/src/commands/terminal.rs",
    "src-tauri/src/teammate/job_object.rs",
    "src-tauri/src/remote_host_impl.rs",
    "src/remote/**",
    "scripts/verify-remote-pwa-build.mjs",
    "package.json"
  ],
  "testTargets": [
    "packages/ridge-term/src/**",
    "packages/remote/src/**/*.test.*",
    "src/lib/**/*.test.*",
    "src-tauri/src/**",
    "src/remote/**/*.test.*",
    "scripts/**/*.test.*"
  ]
}
```


### 保留的实现差异

旧正文要求 Windows Job Object kill-on-close。当前 PtyRegistry 采用 process_identity 与 process_guard 的已验证进程身份/后代回收，现役调用链未见 Job Object 分配。owned_pty_process_tree 测试验证回收结果，不能证明 Job Object 机制满足。此差异按原要求保留，本轮不改产品行为，也不宣称该机制验收通过。
