# ridge-mcp

Default discovery is kernel-only: the bridge uses the active `ridge-kernel`
registry and fails closed when the kernel is unavailable. It never silently
falls back to a stale teammate endpoint. Explicit `--url` and `--token` remain
supported; legacy `RIDGE_TEAMMATE_*` / sidecar discovery requires
`--legacy-sidecar`.

`ridge-mcp` 是 Ridge 桌面内置 MCP 的 stdio companion；它不是 `rdg`，也不托管无头终端。

默认从当前 `ridge-kernel` 登记发现 HTTP MCP 端点和 token；kernel 不可用时 fail closed，不会静默读取
旧 teammate 环境变量或 sidecar。显式 `--url` 与 `--token` 可覆盖发现；仅兼容旧宿主时才传
`--legacy-sidecar`，启用 `RIDGE_TEAMMATE_URL` / `RIDGE_TEAMMATE_TOKEN` 和受限 loopback sidecar
后备发现。端点或 token 轮换后，首次请求失败会重新发现一次。

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

默认启动 companion 无须从 Ridge pane 复制端点或 token；确保 Ridge / `ridge-kernel` 正在运行即可。
不要把端点或 token 写入静态配置。旧 `rdg tmux` / teammate host 仅在显式传入
`--legacy-sidecar` 时接入。详细接入说明见 `docs/mcp-integration.md`。
