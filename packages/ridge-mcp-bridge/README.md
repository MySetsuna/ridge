# ridge-mcp

Default discovery is kernel-only: the bridge uses the active `ridge-kernel`
registry and fails closed when the kernel is unavailable. It never silently
falls back to a stale teammate endpoint. Explicit `--url` and `--token` remain
supported; legacy `RIDGE_TEAMMATE_*` / sidecar discovery requires
`--legacy-sidecar`.

`ridge-mcp` 是 Ridge 桌面内置 MCP 的 stdio companion；它不是 `rdg`，也不托管无头终端。

Ridge 的 HTTP MCP 端点和 token 会随桌面后端重启变化。该程序每次从 Ridge pane 的
`RIDGE_TEAMMATE_URL` / `RIDGE_TEAMMATE_TOKEN` 或受限的 loopback runtime sidecar 发现当前值，再把 stdio
JSON-RPC 转发到 `POST /api/v1/mcp`；首次连接失败会重新发现一次。

桌面安装包会携带同版本 `ridge-mcp`。在安装目录运行：

```bash
ridge-mcp --print-config
```

即可取得只含 companion 绝对命令、无 endpoint/token 的可粘贴配置。

源码开发安装：

```bash
cargo install --path packages/ridge-mcp-bridge --bin ridge-mcp
codex mcp add ridge -- ridge-mcp
```

须从 Ridge pane 启动 Codex，或已有打开的 Ridge 工作区供 sidecar 发现。不要把端点 token
写入静态配置。详细接入说明见 `docs/mcp-integration.md`。
