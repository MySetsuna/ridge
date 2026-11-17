---
id: L4-REMOTE-PANE-READINESS-UX
level: L4
title: Remote 区分未运行 pane 并复用新工作区默认终端
status: DRAFT
lifecycle: PLANNED
parent: L3-REMOTE-INTERACTION-RELIABILITY
code_targets:
  - src/remote/MainApp.svelte
  - src/remote/lib/WorkspaceTree.svelte
  - src/lib/i18n/messages/mobile.ts
  - packages/ridge-kernel/src/host.rs
  - packages/remote/src/shared/transport/wsRemote.ts
  - src/lib/types.ts
  - src/remote/lib/cloudRemote.ts
  - packages/ridge-kernel/src/settings.rs
test_targets:
  - scripts/remote-cwd-acceptance.mjs
  - scripts/remote-createws-test.mjs
  - src/lib/i18n/messages.test.ts
  - packages/ridge-kernel/src/host.rs
  - src/remote/lib/cloudRemote.test.ts
  - packages/remote/src/shared/transport/wsRemote.behavior.test.ts
  - packages/ridge-kernel/src/settings.rs
  - packages/ridge-kernel/tests/remote_invoke_runtime.rs
---

公网真实 Kernel 重启后，工作区拓扑仍可列出 status=detached 的未运行 pane；它不代表活跃 PTY。Remote 须显示未运行及选择终端类型的启动指引，不挂载输入面、不订阅或请求不存在 PTY 的 resize，不自动重启进程，不降低跨工作区所有权检查。保持未知 cwd 清空侧栏范围及既有显式 shell 选择流程。

Kernel create_workspace 已原子创建一个默认真实终端；手机端工作区树不得再额外调用 createPane，须通过真实 panes 回包选中默认 pane。仍保留用户显式新建终端行为，失败显示错误，不推测成功。

以真实 Kernel、生产手机 bundle 和真实 PTY 验证 detached 状态不再出现归属错误且不冒用前一 pane cwd；新建工作区恰有一个终端，显式新增后恰为两个，切换及刷新保留身份。扩展现有执行脚本，不用源码字符串包含断言替代验收。

未运行状态须来自 Kernel 实际 PTY 清单，经既有 pane layout 投影传递给 Cloud/手机端；新增可选 status 字段，旧 Host 缺字段维持未知，不依 cwd、标题或本地缓存推测进程。外部借用视图不因本机缺 PTY 而误标未运行。只扩展 UI 投影，不改认证、持久化或自动启动契约。

新工作区与新增 pane 的实际 shell cwd 必须遵守已配置的 Runtime 文件根：有明确 RIDGE_KERNEL_FS_ROOT 时默认终端从该根启动，不落回根外的 HOME；分屏沿 Kernel split 回包的 initial_cwd 或已持久化 pane cwd 启动。不放宽读取边界。LAN 空白可选 shell 按未指定处理；Host 明确失败必须透传到调用方而非只返回空 ID。真实新建/新增 E2E 核对 PTY 数量、cwd 与既有进程身份。

真实显式启动又复现：change_pane_shell 将图内未运行 pane 当作外来 pane 拒绝。此写操作必须以当前工作区拓扑验证本地所有权；不存在和外部借用仍拒绝。仅对已有 PTY 销毁替换，未运行 pane 按所选 shell 创建一次实际进程，保留同一 pane ID；只读订阅不因此自动启动。Runtime 未设默认 cwd 时复用已配置文件根，统一覆盖工作区创建与显式 shell 启动，不覆盖有效的用户默认设置。验证真实所选进程与跨工作区写入拒绝。
