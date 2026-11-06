# 40 — ridge CLI（headless host）

> 包：`packages/ridge-cli/`
> 二进制：`ridge`（`Cargo.toml` 的 `[[bin]] name = "ridge"`）
> 旧 `rdg` 已下线（commit ea0a3afa + 后续）

ridge CLI 是 Ridge 架构的**headless 入口**：在没有图形界面的 Linux /
VPS 上常驻，承担 PTY 桥接 + 设备配对 + WebRTC 信令 + LAN remote 等
职责。它不渲染终端，只把内核产生的字节通过对应通道送往 controller。

## 二进制与子命令

```text
ridge [--version]
ridge tui          # 交互式 TUI（默认）
ridge login        # 设备码 / 浏览器登录
ridge remote       # 信令 + WebRTC + E2EE + PTY bridge
ridge connect      # 作为控制端连接桌面 LAN host
ridge tmux         # 无头 tmux 引擎
ridge host         # LAN remote host 子进程
ridge mcp          # MCP stdio 适配（兼容路径）
ridge kernel ...   # 内核生命周期（status / stop / ensure / agents / fs-list / git-status / remote-hosts / mcp-smoke）
```

## 模块清单（`packages/ridge-cli/src/`）

| 模块 | 角色 |
|---|---|
| `main.rs` | clap CLI 入口 + 子命令路由 |
| `kernel_host_impl.rs` | LAN remote host：PTY 订阅 + RPC + 信令 |
| `rtp1_kernel_client.rs` | RTP1-over-WS 客户端（migration target） |
| `mux.rs` | 通道 demux（0x10 PANE_RAW / 0x11 JSON-RPC / 0x12 TOTP） |
| `protocol.rs` | SessionControl 帧（TOTP 信道） |
| `rpc.rs` | JSON-RPC 2.0 路由（PTY / fs） |
| `session.rs` | E2EE + 16ms 攒批 + 信令 |
| `host.rs` | 控制端连接（LAN host） |
| `ice.rs` / `rtc.rs` / `signaling.rs` | WebRTC ICE / DataChannel / 信令 |
| `e2ee.rs` | X25519 + ChaCha20Poly1305 + HKDF（与桌面 noble 栈字节一致） |
| `kernel_ctl.rs` / `daemon_ctl.rs` | 内核 / 守护进程控制 |
| `tui/dashboard.rs` / `tui/lan_host*.rs` / `tui/scrollback.rs` | TUI 渲染 |
| `pty.rs` / `batching.rs` | PTY 与批处理 |
| `login_flow.rs` / `totp.rs` | 设备配对 + TOTP |
| `config.rs` / `fs_reuse.rs` / `core_host.rs` | 配置 + 路径 + host abstraction |
| `daemon.rs` / `key_binding.rs` / `envelope.rs` | daemon 模式 + 键绑定 + envelope |
| `kernel_host_impl.rs::start_subscription_rtp1` | 新增的 RTP1 订阅路径 |

## RTP1 WS 客户端（`rtp1_kernel_client.rs`）

`Rtp1KernelClient` 是 shell 端的 RTP1-over-WebSocket 客户端，对应 kernel
端的 `Rtp1Session` + `rtp1_ws::drive`。13 个 wire-round-trip 测试 + 1
个 live kernel + WS e2e 测试覆盖。

```rust
pub struct Rtp1KernelClient { /* ... */ }

impl Rtp1KernelClient {
    pub fn new(endpoint: KernelEndpoint, host_id: String, runtime_epoch: String) -> Self;
    pub async fn connect(
        &self,
        pty_id: Uuid,
        session_id: String,
        since_output_seq: Option<u64>,
    ) -> Result<(Rtp1Sink, mpsc::Receiver<OutputFrame>)>;
    pub fn state(&self) -> impl Future<Output = Rtp1ClientState>;
    // ...
}

pub struct Rtp1Sink { /* 拥有 WS sink 半部 */ }
impl Rtp1Sink {
    pub async fn send_input(&mut self, terminal_id: Uuid, input_seq: u64, data: &[u8]);
    pub async fn send_resize(&mut self, terminal_id: Uuid, rows: u16, cols: u16);
    pub async fn send_detach(&mut self, terminal_id: Uuid, reason: Option<String>);
    pub async fn send_ping(&mut self, nonce: u64);
    pub async fn close(self);
}
```

### 切换：HTTP ↔ RTP1（env flag）

```rust
fn rtp1_kernel_enabled() -> bool {
    matches!(
        std::env::var("RIDGE_RTP1_KERNEL").ok().as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}
```

`KernelHost::dispatch_method` 在 `subscribe-pane` / `subscribe_pane_raw` 时
根据此 flag 选 `start_subscription` (HTTP) 或 `start_subscription_rtp1` (WS)。

```rust
if id.is_none() {
    if matches!(method, "subscribe-pane" | "subscribe_pane_raw") {
        if rtp1_kernel_enabled() {
            start_subscription_rtp1(&params, host, &snapshot, out_tx, subscriptions);
        } else {
            start_subscription(&params, host, &snapshot, out_tx, subscriptions);
        }
    }
    return None;
}
```

### Legacy mux ↔ RTP1 适配（surface 已就位）

rdg 时代的私协议 mux 通道：
* `0x10 PANE_RAW` — host→controller 裸 PTY 字节
* `0x11 JSON-RPC` — RPC 请求 / 响应
* `0x12 CONTROL` — TOTP 控制帧

`rtp1_kernel_client::tests::legacy_pane_raw_to_rtp1_output_round_trip`
证明 `[0x10 PANE_RAW, u32 LE paneId, bytes…]` ↔ RTP1 `output` 帧
**双向无损**。实接到 `mux.rs::channel::PANE_RAW` 出站路径是后续工作。

## 命令行二进制

```
$ ridge --help
Ridge headless remote host for Linux/VPS

Usage: ridge <COMMAND>

Commands:
  tui      Interactive TUI (default if stdin/stdout is a TTY)
  login    Login with device code / browser
  remote   Run headless remote daemon
  connect  Connect as controller to a desktop LAN host
  tmux     Headless tmux session engine
  host     Run LAN remote host subprocess
  mcp      MCP stdio adapter (compatibility)
  kernel   Kernel lifecycle
```

## 系统服务模板

* `packages/ridge-cli/ridge-cli.service` — `ExecStart=/usr/local/bin/ridge remote --daemon`
* `packages/ridge-cli/ridge-tmux.service` — `ExecStart=/usr/local/bin/ridge tmux`

两者均带 `NoNewPrivileges` / `PrivateTmp` / `ProtectSystem=strict` 等沙箱
收敛。

## 配置目录

```
~/.config/ridge/
├── auth.json             # device JWT 凭据
├── ridge.log             # TUI 模式 tracing 文件 writer 输出
└── ...

/var/lib/ridge/.config/ridge/  # 系统级用户 ridge 安装时
```

## 测试

```
cargo test -p ridge-cli --bin ridge           175 lib
cargo test -p ridge-cli --test rtp1_kernel_e2e 1  ← live WS e2e
cargo test -p ridge-cli --test kernel_lifecycle_e2e 4/5 (1 pre-existing harness timeout)
```

## 详尽规范

* CLI 完整命令：`ridge --help`
* 服务模板：`packages/ridge-cli/ridge-cli.service`、`ridge-tmux.service`
* 端到端测试：`packages/ridge-cli/tests/rtp1_kernel_e2e.rs`
* Foundation 终报告：`docs/architecture/RIDGE-RUNTIME-FOUNDATION-FINAL.md`
