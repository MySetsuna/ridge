# 80 — Security

> 模块：kernel `server.rs` + `domain.rs` + `pty.rs`；CLI `kernel_host_impl.rs`
> 详尽规范：`specs/L2-PROTO-001.md` §3.4 + `specs/L2-REMOTE-001.md` §3.5.5

Ridge 的安全模型围绕四件事：
1. **Token gating** — kernel 的 HTTP/WS 入口都要 token 校验。
2. **Controller_id ownership** — input 与 resize 必带已注册的 controller_id。
3. **Transport integrity** — RTP1 不提供 application-level CRC（P4），
   transport 必须保证 reliable + ordered + complete。
4. **Replay windowing** — bounded cap 防止 unbounded memory；cursor 落后
   → 必须 resync（不静默续流）。

## 1. Token gating

kernel 启动时：
```rust
let token = Uuid::new_v4().to_string();
// 写入 %LOCALAPPDATA%/ridge/kernel.{pid,json}
write_registry(&KernelEndpoint { pid, port, token, started_at_unix })?;
```

所有路由走 `auth_ok`：

```rust
fn auth_ok(headers: &HeaderMap, token: &str) -> bool {
    headers
        .get("x-ridge-kernel-token")
        .or_else(|| headers.get("x-ridge-token"))
        .and_then(|v| v.to_str().ok())
        == Some(token)
}
```

WebSocket upgrade 也走同一校验（`rtp1_ws_handler`）。

## 2. Controller_id ownership（输入所有权）

### 强制点

| 入口 | 校验 |
|---|---|
| RTP1 WS `input` 帧 | `rtp1_ws::handle_input` 校验 controller_id 与 active_controller 一致 |
| RTP1 WS `resize` 帧 | 同上 |
| 旧 HTTP `/v1/domain/ptys/:id/write` | `domain::domain_pty_write` 校验 controller_id（缺省时注册 `legacy-http:<pty_id>` 合成 id） |
| 旧 HTTP `/v1/domain/ptys/:id/resize` | 同上 |

### 数据结构

```rust
// pty.rs:PtyRegistry
attached_controllers: Arc<Mutex<HashMap<Uuid, HashSet<String>>>>
```

每 PTY 一个 HashSet，元素为 controller_id 字符串。

### API

```rust
pub fn write_with_controller(&self, id: Uuid, controller_id: &str, data: &[u8])
    -> Result<(), PtyInputError>;
pub fn attach_controller(&self, id: Uuid, controller_id: String);
pub fn detach_controller(&self, id: Uuid, controller_id: &str);
```

### 错误码映射

| 错误 | HTTP / RTP1 code |
|---|---|
| `PtyInputError::UnknownTerminal` | `unknown_terminal` |
| `PtyInputError::ControllerIdUnknown` | `controller_id_unknown` |
| `PtyInputError::NotReady` | `server_overloaded` |

### 错误码汇总（RTP1 §3.10）

| code | 含义 | 触发 |
|---|---|---|
| `unknown_envelope` | magic / efv 不识别 | transport boundary |
| `unknown_message` | type 字段不识别 | payload 解码后 |
| `unknown_terminal` | terminal_id 不存在或已 detach | 服务端按 host session 状态查表 |
| `runtime_epoch_stale` | attach 时 runtime_epoch 不匹配 | §3.4.3 |
| `controller_id_unknown` | input 帧带未 attach 的 controller_id | 服务端 lane 查表 |
| `attach_denied` | 权限或并发限制拒绝 | 鉴权层 |
| `input_too_large` | 单帧 input > `max_realtime_frame` | §3.7 |
| `client_too_old` | `client_max_version < server_version` | §3.2 |
| `server_overloaded` | 服务端背压拒收 | 资源耗尽 |
| `protocol_violation` | 顺序 / 必填字段缺失 / 校验失败 | 解码层 |
| `io_error` | PTY 子进程级错误 | kernel PtyRegistry |
| `replay_unavailable` | replay 数据已被 GC | kernel PtyOutputHub |

## 3. Transport integrity

RTP1 自身**不**提供 application-level CRC（P4）。这意味着：

* WebSocket 之前必走 TLS（HTTPS / WSS）。
* 生产部署通过 reverse proxy / ridge-remote bridge 提供 TLS。
* localhost 上 plain WS 是 dev-only（127.0.0.1 受 kernel token 保护）。

```rust
// rtp1_kernel_client.rs:ws_url
pub fn ws_url(&self) -> String {
    format!("ws://127.0.0.1:{}/v1/rtp1", self.endpoint.port)
}
```

> ⚠️ 当前 ridge-cli / rdg / 桌面端本地调用都是 plain WS（受 kernel
> token + 127.0.0.1 边界保护）。**生产部署必须通过 reverse proxy
> 提供 TLS**。

## 4. Replay windowing

PtyOutputHub 是 bounded 环形 buffer：

```rust
OUTPUT_REPLAY_CAP_BYTES: usize = 256 * 1024
OUTPUT_REPLAY_CAP_FRAMES: usize = 256
```

任何 publish 超 cap → FIFO 丢头；lease cursor 落后 → `Lagged{requested, oldest, latest}`。

客户端收到 `Lagged` 后必须：
1. 调 `replay(since_output_seq=oldest)` 重放剩余 bytes；或
2. 调 `resync(mode=snapshot)` + `snapshot` 全帧替换 mirror。

禁止 silent rebind。

## 5. E2EE（仅 ridge CLI ↔ controller，**非 RTP1**）

ridge CLI 的 WebRTC DataChannel 上叠 X25519 + ChaCha20Poly1305：
```rust
// packages/ridge-cli/src/e2ee.rs
x25519-dalek = { version = "2", features = ["static_secrets"] }
chacha20poly1305 = "0.10"
hkdf = "0.12"
sha2 = "0.10"
```

字节级兼容桌面端 `e2ee.ts`（noble 栈）。TOTP HMAC bind 走 `0x12`
通道。

RTP1 不再叠加 E2EE（信道本身由 WebRTC DataChannel 已加密）；ridge-kernel
看到的是明文 PTY bytes。

## 6. Resize 权限

```rust
// rtp1_session.rs:Rtp1Session::handle_resize
if matches!(request.owner, Some(ResizeOwner::Observer)) {
    return Err(AttachError::PermissionDenied("observer cannot resize".into()));
}
```

Observer 角色不能 resize；只有 Controller 角色可。

## 7. 测试覆盖

* `stability_fault::input_seq_wire_validation_unknown_controller_rejected`
* `stability_fault::input_seq_wire_validation_multi_controller_isolation`
* `stability_fault::runtime_epoch_rebind_panics`
* `conformance_rtp1::acceptance_runtime_epoch_stale_rejected`
* `conformance_rtp1::acceptance_protocol_version_no_overlap_rejected`
* `conformance_rtp1::acceptance_input_too_large_rejected`
* `stability_fault::attach_error_code_canonical`（8 个 AttachError 变体 → canonical code 映射）

## 8. 不在 Foundation 范围内

* 双向 TLS 证书 pinning（依赖 reverse proxy / ridge-remote bridge）
* 跨 host_id 的访问控制列表（v1 假定同组织内部使用）
* rate limiting per controller（v1 依赖 kernel backpressure 即可）

## 详尽规范

* `specs/L2-PROTO-001.md` §3.4 + §3.9 + §3.10
* `specs/L2-REMOTE-001.md` §3.4 + §3.5.5
