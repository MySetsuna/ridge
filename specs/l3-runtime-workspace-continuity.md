---
id: L3-RUNTIME-WORKSPACE-CONTINUITY
level: L3
title: Runtime file access and workspace pane continuity
status: LOCKED
lifecycle: ACTIVE
parent: L2-UNIFIED-CLIENT-RUNTIME
code_targets:
  - packages/ridge-kernel/src/host.rs
  - packages/ridge-kernel/src/client.rs
  - packages/ridge-kernel/src/domain.rs
  - packages/ridge-kernel/src/kernel_mcp.rs
  - packages/ridge-kernel/src/workspace_commands.rs
  - packages/ridge-kernel/src/workspace_files.rs
  - packages/ridge-kernel/src/workspace_autosave.rs
  - packages/ridge-core/src/fs/commands.rs
  - src/lib/stores/fileExplorer.ts
  - src/lib/components/FileTree.svelte
  - src/lib/components/Explorer.svelte
  - src/lib/stores/paneTree.ts
  - packages/ridge-core/src/workspace/file.rs
test_targets:
  - packages/ridge-kernel/tests/runtime_workspace_continuity.rs
  - packages/ridge-kernel/tests/common/runtime_fixture.rs
  - packages/ridge-kernel/tests/cwd_runtime.rs
  - packages/ridge-kernel/tests/terminal_lifecycle_runtime.rs
  - src/lib/stores/fileExplorer.test.ts
  - src/lib/stores/paneTree.test.ts
  - src/lib/stores/paneTree.coverage.test.ts
  - packages/ridge-kernel/tests/workspace_file_owner.rs
---

# Runtime 工作区连续性

本契约约束内核化后既有文件访问、命名工作区保存、pane cwd 与工作区视图连续性，不改变临时会话保活或恢复策略。

同一已认证桌面客户端的文件树首层、子目录、搜索与打开文件须使用一致权限上下文；始终保留操作者文件范围及远程授权边界。目录读取失败必须传至界面错误/重试状态，不得伪装成功空目录；复制移动的目标枚举失败必须终止变更。编辑器允许范围内文本必须能通过本地 Kernel 响应边界，超限仍有界拒绝。

桌面与 MCP 分屏均继承锚定 pane 当前真实 shell cwd，并应用到子 PTY。命名保存工作区发生拓扑变更须沿现有自动保存机制更新 .ridge；分屏后立即观测 cwd 不得因文件旧拓扑而失败。合并须保留未知扩展字段、原子写入、归档及失败回滚；不得将未命名临时项变成命名保存项。已提交分屏不能因后续自动保存排队失败而丢失 PTY 创建。

异步布局查询必须绑定请求时工作区；A 请求在切至 B 后返回，只能更新 A 缓存，不能覆盖 B 树、焦点或 pane 身份。

验收：真实隔离 Kernel 的 native grant/WebSocket 覆盖授权目录展开与文本打开、权限拒绝；本地 HTTP 桥覆盖编辑器响应容量；真实 shell cd 到带空格目录后分屏，检查子 PTY cwd，再覆盖保存、连续分屏及文件重读。前端用受控异步响应覆盖切工作区竞态，用拒绝响应覆盖错误传播。运行相关 Rust 集成、npm test/lint/typecheck 与 SpecTree verify。

命名保存路径须与现有保存对话框预览一致：显式 .ridge 路径（扩展名忽略大小写）保持不变；其他路径视为目录，含尚不存在目录，创建父目录后追加经既有清理规则生成的名称。失败不得写入另一个推测文件路径。真实 Kernel 验收须覆盖已存在目录、新目录和显式大小写扩展名路径。
已保存文件枚举与目录自动发现须识别同一大小写不敏感扩展名，不得遗漏已成功保存的 .RIDGE 文件；权限和非递归发现边界保持。

工作区列表刷新亦须受同一选择时序约束：刷新开始前已排队的本地切换完成后，方读取调用方选择；刷新期间发生更新选择，则旧响应不得更改当前工作区、树、焦点或监听器。并发刷新只接受最近一次列表快照，过期失败不弹出已无效的错误。列表及保存信息仍正常更新，关闭活动工作区后可采纳后端替代项。验收须控制真实 store 的异步调用顺序：A 布局迟到且 B 切换已完成、切换未完成时触发刷新、两个列表刷新逆序、显式新建/打开及空选择回归；同时以 CDP 触发列表事件并交错切换验证键鼠输入。

Windows cwd 验收按文件系统规范化后的目录身份比较，兼容同一目录的 8.3 短路径和完整路径；实际 shell cd、父子 PTY、OSC 7 观测、连续分屏与保存字段断言仍须运行。不存在或不同目录不得通过。验收须在真实短路径 TEMP/TMP 下运行 PowerShell 集成测试，避免只按开发机路径文本断言。测试与版本更新随 0.1.95 发布，保留失败版本记录。
Windows 目录别名亦须贯通 Runtime 权限校验：授权根使用短路径而 shell 观测长路径，或反向使用时，同一已解析物理目录不得误报越界。修复共享 RootScope，仍同时执行操作者根与调用方根约束；不得通过改 fixture 根、删除权限校验或开放空根掩盖问题。
