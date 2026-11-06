# 40 — CLI 与独立终端创建

[main.rs](../../packages/ridge-cli/src/main.rs)提供 TUI、默认仪表盘、login、remote、connect、tmux、host、mcp 与 kernel 入口。CLI 启动或连接独立 Runtime，LAN listener 与 Cloud 会话由 Runtime 持有。TUI 消费语义并在本地展示。

## 四入口

各入口共用 [lan_session.rs](../../packages/ridge-cli/src/tui/lan_session.rs)构造的 create_independent_terminal 请求，在 [business.rs](../../packages/ridge-kernel/src/business.rs)鉴权分发，在 [domain.rs](../../packages/ridge-kernel/src/domain.rs)提交。权威行为见[CLI 独立终端契约](../../specs/l3-cli-independent-terminal-creation.md)。

| 入口 | 创建前置条件 |
| --- | --- |
| TUI，含多会话 | 取得 origin-bound local grant，打开共同 Host WS，再创建和订阅 |
| 默认仪表盘 | 连接 Runtime，经受保护的 native 业务请求创建 |
| 仅 LAN | Runtime LAN listener 确认可用后才创建 |
| Cloud，显式 shell 或 CWD | 完成本地请求授权，Cloud 收到认证 Host welcome 后才创建 |
| Cloud，无显式 shell/CWD | 启动/连接 Host，创建数量为零 |

[tui/mod.rs](../../packages/ridge-cli/src/tui/mod.rs)、[dashboard.rs](../../packages/ridge-cli/src/tui/dashboard.rs)与 [daemon.rs](../../packages/ridge-cli/src/daemon.rs)接各入口。每次显式创建独立工作区、pane 与 PTY，避免读取 snapshot 隐式创建或合并到其他客户端所选工作区。

## 失败与重试

- 本地授权无效、过期、Origin 不符或 WS 握手失败时，TUI 不提交创建。
- LAN 上线失败、Cloud 认证/上线失败或配置冲突时拒绝创建。已在线 Cloud 配置冲突须显式处理，错误保留现存服务。
- 一次逻辑请求固定 workspaceId、paneId、runtimeEpoch。未知结果重试同一身份，重复/并发提交返回同一活终端，过期代际、已关闭或身份冲突明确报错。
- 保存拓扑失败回滚新 PTY，用户可用原身份重试。错误携带可核对身份，超时不能宣称未执行。

[cli_terminal_create_e2e](../../packages/ridge-cli/tests/cli_terminal_create_e2e.rs)覆盖四入口、鉴权与上线失败、回滚、丢失响应及并发重试。[ridge-remote auth](../../packages/ridge-remote/src/auth.rs)覆盖授权到期和重放。公网及平台实测范围须查实际执行证据。
