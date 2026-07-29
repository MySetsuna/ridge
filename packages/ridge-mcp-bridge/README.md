# ridge-mcp

`ridge-mcp` 是 Ridge 桌面内置 MCP 的 stdio companion；它不是 `rdg`，也不托管无头终端。

Ridge 的 HTTP MCP 端点和 token 会随桌面后端重启变化。该程序每次从 Ridge pane 的
`RIDGE_TEAMMATE_URL` / `RIDGE_TEAMMATE_TOKEN` 或 endpoint sidecar 发现当前值，再把 stdio
JSON-RPC 转发到 `POST /api/v1/mcp`；首次连接失败会重新发现一次。

当前源码安装：

```bash
cargo install --path packages/ridge-mcp-bridge --bin ridge-mcp
codex mcp add ridge -- ridge-mcp
```

须从 Ridge pane 启动 Codex，或已有打开的 Ridge 工作区供 sidecar 发现。不要把端点 token
写入静态配置。详细接入说明见 `docs/mcp-integration.md`。
