# 40 — CLI 与独立终端创建

[main.rs](../../packages/ridge-cli/src/main.rs)提供 TUI、默认仪表盘、login、remote、connect、tmux、host、mcp、agent 与 kernel 入口。CLI 启动或连接独立 Runtime，LAN listener 与 Cloud 会话由 Runtime 持有。TUI 消费语义并在本地展示。

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


## Agent 协作终端

[agent 入口](../../packages/ridge-cli/src/managed_agent.rs)复用共同 Host 与独立终端身份；[参数定义和控制台](../../packages/ridge-kernel/src/managed_console.rs)由 Kernel 管理。用户显式启动时，才创建所选 Agent 的真实协议会话。例如：

```sh
ridge agent codex --cwd /absolute/project
ridge agent --command /absolute/path/to/acp-agent --protocol acp --cwd /absolute/project -- --agent-option value
```

内置清单包含 Codex、Claude、Grok Build、OpenCode；自定义命令使用兼容的 ACP 或 app-server。程序参数放在 `--` 之后，保持其原始 argv。程序、适配器、登录和协议能力须实际可用，名称识别不代表会话已接入。

Kernel 的 [managed_agent.rs](../../packages/ridge-kernel/src/managed_agent.rs)持有协议进程、通道和准确会话；控制台把用户明确输入路由到此会话，并展示权限请求。打开创建表单、发现或预览均不启动 Agent。关闭展示连接沿用独立终端的 detach 规则；显式退出协作控制台与移出 Commune 各按自己的生命周期处理。

已有原生终端继续走原进程与原会话的接入路径，不隐式改为协作终端。正常接入会从 Runtime 已发布或已声明的描述自动准备并校验，手工连接文件保留为高级入口。鉴权失败先在相应工具原生 CLI 完成登录，再显式重试；未知提交结果核对原身份，避免重复创建。详见[权威接入契约](../../specs/l4-commune-automatic-agent-connection.md)。
