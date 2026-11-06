# 30 — 连接、订阅与恢复

职责依据见[统一 Runtime](../../specs/l2-unified-client-runtime.md)与[Remote 交互可靠性](../../specs/l3-remote-interaction-reliability.md)。

| 对象 | 所有者与退出效果 |
| --- | --- |
| Runtime | 独立 Kernel，重启产生新 runtime epoch |
| PTY child | Kernel，展示连接关闭只 detach，显式关闭或进程退出另行处理 |
| 展示连接及订阅 | 客户端与 Host，断线撤销该连接的订阅，重连重新绑定 |

[KernelHost](../../packages/ridge-kernel/src/host.rs)将 workspace 选择及终端订阅绑定到连接。[RemoteConnection](../../packages/remote/src/shared/transport/wsRemote.ts)处理浏览器连接，[CLI session](../../packages/ridge-cli/src/tui/lan_session.rs)处理 TUI 会话。

## 恢复顺序

1. 重新确认 Host、Runtime 代际与凭据。
2. 绑定本客户端所选工作区/pane，建立活动订阅。
3. 校验 terminal lifetime、activation、revision 与 history generation，按服务提供的快照或恢复流同步。
4. 失配或落后于有界窗口时显式重同步，旧帧不能改写当前 pane 或历史窗口。

网络恢复、终端重建与布局恢复各有身份和副作用。Runtime 重启后的已死进程须显示实际状态，显式保存文件仅恢复布局与元数据。

创建结果未知时，保留原 workspaceId、paneId、runtimeEpoch 重试同一逻辑请求，服务返回同一活终端或明确拒绝。再次显式创建生成新身份。见[创建契约](../../specs/l3-cli-independent-terminal-creation.md)及[真实进程验收](../../packages/ridge-cli/tests/cli_terminal_create_e2e.rs)。

回归入口包括 [remote_backpressure](../../packages/ridge-kernel/tests/remote_backpressure.rs)、[terminal_lifecycle_runtime](../../packages/ridge-kernel/tests/terminal_lifecycle_runtime.rs)与 [runtime_workspace_continuity](../../packages/ridge-kernel/tests/runtime_workspace_continuity.rs)。
