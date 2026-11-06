# 10 — ridge-kernel 架构

> 模块：`packages/ridge-kernel/src/`
> binary：`packages/ridge-kernel/src/main.rs`（`ridge-kernel.exe`）

ridge-kernel 是一个独立进程，承载 RIDGE Runtime Foundation
的全部不变量：**唯一 PTY 进程权威 / 唯一 output_seq 产生者 / 唯一
runtime_epoch mint 器 / 唯一 controller_id 校验器**。

## 进程模型

```text
main.rs → server::run(host, port)
        ↓
  ┌──────────────────────────────────────────┐
  │ axum::serve()                            │
  │   /v1/health    GET                      │
  │   /v1/status    GET    (host_id + epoch) │
  │   /v1/shutdown  POST                     │
  │   /v1/domain/*  GET/POST (FS, git, ptys) │
  │   /v1/rtp1      GET WS  (RTP1 canonical) │
  │   /api/v1/mcp   POST/GET WS              │
  └──────────────────────────────────────────┘
```

* `AppState` 持有：token / pid / port / `host_id` / `runtime_epoch` /
  `PtyRegistry` / `output_leases` / mcp_state / remote_hosts / etc.
* 进程级锁：`KernelInstanceGuard`（`registry.rs`）防多实例。

## 模块清单

| 模块 | 行数 | 角色 |
|---|---|---|
| `lib.rs` | ~20 | 模块声明 |
| `main.rs` | ~30 | 二进制入口 |
| `server.rs` | ~430 | axum 路由 + AppState + `/v1/rtp1` 处理器 |
| `domain.rs` | ~2900 | `/v1/domain/*` HTTP handler（FS / git / ptys / workspaces） |
| `pty.rs` | ~1245 | **PTY 进程权威** + lifecycle + bounded replay |
| `registry.rs` | ~370 | 进程发现 / `KernelEndpoint` / lock file / 远程 host topology |
| `client.rs` | ~1841 | shell 端的 discovery + HTTP client（bounded-seq-v1 adapter） |
| `kernel_mcp.rs` | ~800 | MCP 协议路由 |
| `kernel_backed_handle.rs` | ~145 | shell-side mirror type（per-(host_id, runtime_epoch, terminal, controller)） |
| `agent_profiles.rs` | ~100 | Agent profile domain |
| `rtp1.rs` | ~700 | **RTP1 envelope + 22 消息类型** |
| `rtp1_session.rs` | ~600 | **Attachment state machine + per-PTY session** |
| `rtp1_ws.rs` | ~530 | **WebSocket ↔ RTP1 适配** |

## PtyRegistry 内部（PTY 进程权威）

```text
PtyRegistry
├── ptys: Mutex<HashMap<Uuid, ManagedPty>>
│       ManagedPty {
│           bridge: Arc<PtyBridge>,         // portable_pty master/writer/child
│           scrollback: Arc<Mutex<Vec<u8>>>,
│           renderer: Arc<Mutex<Terminal>>,  // ridge_term::Terminal instance
│           output: Option<Arc<PtyOutputHub>>,
│           closing: AtomicBool,
│           info: PtyInfo,
│       }
├── runtime_epoch: Mutex<Option<String>>   // 一次绑定，不可重绑（panic）
├── lifecycle: Arc<Mutex<HashMap<Uuid, LifecycleEntry>>>
│       LifecycleEntry { state, exit_code }
├── exit_subs: Arc<Mutex<HashMap<Uuid, broadcast::Sender<PtyExitNotification>>>>
└── attached_controllers: Arc<Mutex<HashMap<Uuid, HashSet<String>>>>
```

### Lifecycle state machine

```
          spawn_command_for_with_env
                    ↓
              ┌──────────┐
              │ Starting │  ← reader task 未收到首字节
              └────┬─────┘
                   │ first byte (在 reader task 里)
                   ↓
              ┌──────────┐
              │ Running  │  ← PTY 子进程活动，可读写
              └────┬─────┘
                   │ PTY 子进程退出 (EOF / signaled)
                   ↓
              ┌──────────┐
              │  Exited  │  ← exit_code 已知；scrollback 与 lease cursor 仍可读
              └────┬─────┘
                   │ lease 全部 detach + GC timeout
                   ↓
              ┌──────────┐
              │  Reaped  │  ← 内核回收（registry entry 已移除）
              └──────────┘
```

转换保证：
* `Starting → Running` ≤ 5s；超时按 `Exited` 处理（带 reason=`start_timeout`）。
* `Exited` 不可回退；session_event{event:"exited"} 必发。
* `Reaped` 后 attach 必返 `error{code:"unknown_terminal"}`。

### PtyOutputHub（bounded replay）

* 环形 buffer：256 KiB 字节 + 256 帧双 cap（`OUTPUT_REPLAY_CAP_*`）。
* 任何 publish 超 cap → FIFO 丢头。
* 多 lease 独立 cursor，互不干扰。
* cursor 落后 oldest_seq → `Lagged{requested, oldest, latest}`。

### Reader task（与 PtyBridge 同寿命）

```text
tokio::spawn(async move {
    while let Some(bytes) = output.recv().await {
        screen.lock().feed(&bytes);     // 同步 ridge_term::Terminal
        retained.extend_from_slice(&bytes);
        if retained.len() > SCROLLBACK_CAP { drain prefix }
        hub.publish(&bytes);            // ← producer to PtyOutputHub
        // 第一次 publish：lifecycle = Running
    }
    hub.close();
    // output 结束：lifecycle = Exited，广播 PtyExitNotification
});
```

## RTP1 server 侧

```
WebSocket /v1/rtp1
  → rtp1_ws::drive(socket, WsContext { session, attachments })
    → Rtp1Session::handle_attach(req) → (AttachAck, PtyOutputLease, AttachmentState::Attached)
    → ctx.ptys.attach_controller(terminal_id, controller_id)
    → read_loop spawn: hub.publish → build_output_frames → sink
                       subscribe_exit(pty_id) → on Exited → SessionEvent{exited}
                       DetachAck on detach
```

详见 `20-rtp1-wire.md` 与 `30-remote-lifecycle.md`。

## Per-controller 输入所有权

* RTP1 WS adapter 在 `attach` / `detach` 时调
  `PtyRegistry::attach_controller` / `detach_controller`。
* 旧 HTTP 适配（`/v1/domain/ptys/:id/write`）现在也接受可选
  `controller_id` 字段；缺失时自动注册 `legacy-http:<pty_id>` 合成 id。
* `PtyRegistry::write_with_controller(id, controller_id, data)` 是
  受控入口；未注册的 controller → `PtyInputError::ControllerIdUnknown` →
  RTP1 `error{code:"controller_id_unknown"}`。
* 详见 `80-security.md`。

## Test surface（kernel）

```
cargo test -p ridge-kernel
  --lib              78 passed
  conformance_*      14 passed
  terminal_live      16 passed
  stability_fault    13 passed
  conformance_rtp1   26 passed
  performance_baseline (--ignored)  5 passed
```

## 关键文件位置

* `packages/ridge-kernel/src/server.rs`：`/v1/rtp1` 路由 + `AppState`
* `packages/ridge-kernel/src/pty.rs`：PTY 进程 + lifecycle + replay
* `packages/ridge-kernel/src/rtp1.rs`：22 消息类型定义
* `packages/ridge-kernel/src/rtp1_session.rs`：Attachment state machine
* `packages/ridge-kernel/src/rtp1_ws.rs`：WebSocket 适配
* `packages/ridge-kernel/tests/`：conformance + terminal_live + stability_fault
