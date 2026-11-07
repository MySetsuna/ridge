---
id: L4-REMOTE-CONDITIONAL-FILE-SAVE
level: L4
title: Remote 编辑器条件保存与冲突保护
status: LOCKED
lifecycle: ACTIVE
parent: L3-REMOTE-INTERACTION-RELIABILITY
code_targets:
  - packages/ridge-core/src/fs/commands.rs
  - packages/ridge-core/src/dispatch.rs
  - packages/ridge-core/src/capability.rs
  - packages/remote/src/shared/cloud/remoteAllowlist.ts
  - src/lib/transport/types.ts
  - src/lib/transport/ws.ts
  - src/lib/transport/tauri.ts
  - src/shared/sidebar/types.ts
  - src/remote/lib/sidebarProvider.ts
  - src/remote/lib/FileViewer.svelte
  - src/lib/i18n/messages/mobile.ts
  - src/lib/transport/fileContentHash.ts
test_targets:
  - packages/ridge-core/src/dispatch.rs
  - packages/remote/src/shared/cloud/remoteAllowlist.test.ts
  - src/lib/transport/ws.test.ts
  - src/lib/transport/tauri.test.ts
  - src/remote/lib/sidebarProvider.test.ts
  - scripts/remote-tabs-acceptance.mjs
  - src/lib/stores/hosts.connect.test.ts
acceptance_checks:
  - id: conditional-save-core
    command:
      - cargo
      - test
      - --locked
      - -p
      - ridge-core
      - conditional_save
    description: 从 Core 正常分发入口验证条件保存、并发竞争、已提交重试、无效参数、根目录和只读权限
  - command:
      - cargo
      - build
      - --locked
      - -p
      - ridge-kernel
      - --bin
      - ridge-kernel
    id: conditional-save-kernel-build
    description: 以当前 Core 和 Kernel 源码构建实际 UI 验收进程，禁止沿用不含新命令的旧二进制
  - id: conditional-save-remote-ui
    command:
      - node
      - scripts/remote-tabs-acceptance.mjs
    description: 真实 Remote 编辑组件和隔离 Kernel 上验证外部修改后的冲突、草稿保留、重新读取和明确保存，以及已有保存/断线/PWA/Git 回归
depends_on: []
---

# Remote 编辑器条件保存

## 契约

新增写命令 `write_file_if_unchanged`，参数 `path`、`content`、`expectedHash` 均为必填字符串。expectedHash 为编辑器最后确认读取或成功保存的完整 UTF-8 原文字节的 SHA-256，小写十六进制 64 字符。空文件使用空字节串摘要。前置条件不重复传输整份旧文件。文件不存在或不可读直接失败，不隐式重建文件。

当前文件与 content 完全相同时直接确认，支持提交已成功但回执丢失后的幂等重试。否则只有当前文件的 SHA-256 与 expectedHash 完全相同才沿现有文件写入实现提交。冲突错误包含稳定标记 `FILE_CONFLICT`，错误不回传文件内容。写入失败按既有 IO 错误上报，客户端保留完整草稿和最后确认快照，不报告已保存。

同一 Runtime 的 Core 文件写、局部编辑、创建、改名、删除、复制和移动命令与条件保存使用同一文件变更互斥边界，在取得锁后复核根目录。只读和授权检查在副作用之前拒绝。进程外编辑在条件检查前已发生的变化可检测，此接口不声明对不参与 Runtime 互斥的进程外写入提供跨进程原子 CAS。

## 客户端

DataProvider 和 SidebarProvider 保留原两参数 writeFile，并新增可选能力方法 writeFileIfUnchanged(path, content, expectedContent)。传输实现通过同一摘要函数生成 wire expectedHash。Remote FileViewer 必须使用独立条件保存方法，旧适配器缺少该方法时直接拒绝保存并保留草稿。LAN 与 Cloud 同源，Tauri 共享调用者支持相同参数。旧 Host 不认识新命令时保留草稿并显示失败，禁止退回无条件保存。

冲突提示明确说明其他端修改、草稿保留，以及复制草稿、重新读取、合并后保存的路径。显式重新读取沿用未保存确认和强制刷新。发送中继续编辑、目标切换、PWA 保存后更新保持现有 generation fence，迟到回执只能确认原目标与原提交快照。成功后才更新已保存快照和失效文件/Git 缓存。

## 验收

覆盖外部编辑已先提交、同一基线的两个 Runtime 保存竞争、成功后重试、内容相同、空文件、缺失文件、参数缺失、只读、越权和路径遍历。传输分别证明旧调用与条件调用的命令字段、错误透传和无静默回退。真实 UI 在冲突时保留草稿与磁盘新内容，明确重新读取后才能以新快照保存。保留断线不重放、迟到 ACK、新输入、目标切换、PWA 及 Git 验收。

全量门禁的主机接入测试隔离收到与发出的共享查询，禁止因残留真实 Cloud HTTP 请求产生外网依赖和超时。
