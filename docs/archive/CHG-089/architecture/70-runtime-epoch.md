# 70 — Runtime Epoch 与 Host Identity

> 模块：`packages/ridge-kernel/src/server.rs` + `pty.rs` + `rtp1_session.rs`
> 测试：`packages/ridge-kernel/tests/stability_fault.rs` + `conformance_rtp1.rs`

`runtime_epoch` 与 `host_id` 是 kernel 进程级的**身份锚点**：
* `host_id` 在 `(runtime_epoch)` scope 内稳定。
* `runtime_epoch` 每次 kernel 启动 mint 一次（UUID v7）。
* 客户端必须把两者持久化在本地（attach 时回传），用于：rediscovery /
  stale attach 检测 / 跨 boot 身份重建。

## 生成

```rust
// server.rs:run
let runtime_epoch = Uuid::now_v7().to_string();
let host_id = std::env::var("RIDGE_HOST_ID")
    .ok()
    .filter(|value| !value.trim().is_empty())
    .unwrap_or_else(|| {
        format!(
            "{}@{}",
            std::env::var("COMPUTERNAME")
                .or_else(|_| std::env::var("HOSTNAME"))
                .unwrap_or_else(|_| "unknown".into()),
            &runtime_epoch
        )
    });
```

* `runtime_epoch`：`Uuid::now_v7()`（时间戳 + 随机），保证跨进程唯一性。
* `host_id`：优先 env `RIDGE_HOST_ID`；否则 `<COMPUTERNAME>@<runtime_epoch>`。

## 一次性绑定

```rust
// pty.rs:PtyRegistry
pub fn set_runtime_epoch(&self, epoch: String) {
    let mut guard = self.runtime_epoch.lock();
    if guard.is_some() {
        panic!("runtime_epoch already bound for this registry");
    }
    *guard = Some(epoch);
}
```

重复绑定 → `panic`。这是 Foundation 的"防 silent rebind"闸门：
* 进程内重启 = 不可能（panic）；
* 跨进程 = 新 epoch（新 PtyRegistry 实例）；
* 旧 epoch 持有者 = 旧 PtyRegistry，attach 必 stale。

## Attach 校验（SPEC-L2-PROTO-001 §3.4.3）

```rust
// rtp1_session.rs:Rtp1Session::handle_attach
let current_epoch = self.ptys.runtime_epoch()
    .ok_or_else(|| AttachError::ServerMisconfigured("runtime_epoch not bound".into()))?;
if request.runtime_epoch != current_epoch {
    return Err(AttachError::RuntimeEpochStale {
        expected: current_epoch,
        received: request.runtime_epoch.clone(),
    });
}
```

不匹配 → `error{code:"runtime_epoch_stale", expected, received}`，
**不**进入 `Attached`。

## 生命周期

```
kernel boot
    │
    ├─► Uuid::now_v7() → runtime_epoch
    │
    ├─► PtyRegistry::set_runtime_epoch(epoch)        ← 一次绑定
    │
    ├─► AppState { host_id, runtime_epoch, ... }
    │
    ├─► HTTP /v1/status body { host_id, runtime_epoch }
    │
    └─► WS /v1/rtp1 capability_advertise (含 features / caps)
        ↓
        ↓ 客户端 attach
        ↓ 携带 client.runtime_epoch
        ↓
        服务端校验：相等 → accept；否则 → error{code:"runtime_epoch_stale"}
        ↓
        一旦 accept，attach_ack 回执带 server.runtime_epoch
        后续所有 C→S 帧不必再带 runtime_epoch（已在 session 绑定）
```

## 与 host_id / terminal_id / controller_id 的关系

```text
host_id         ──▶ (host_id, runtime_epoch)        scope 内稳定
                  │
                  ▼
terminal_id     ──▶ (host_id, runtime_epoch, terminal_id)  scope 内唯一
                  │
                  ├─► output_seq (per-terminal)
                  │
                  ▼
controller_id   ──▶ (host_id, runtime_epoch, terminal_id, controller_id)  唯一
                  │
                  ▼
input_seq       ──▶ (host_id, runtime_epoch, terminal_id, controller_id, input_seq)  唯一
```

不同 `runtime_epoch` 的同 `terminal_id` 视为不同对象。客户端必须走：

```text
discover kernel (host_id + runtime_epoch)
└─► call host_list_sessions
    └─► if no matching terminal: terminal was destroyed → rebuild view
    └─► if new terminal_id: bind to new object (silently rebind forbidden)
```

## 持久化（client 端）

```rust
localStorage["ridge.identity"] = JSON.stringify({
    host_id: "...",
    runtime_epoch: "...",
    controller_id: "...",
    terminal_seq: { "term-1": 42, "term-2": 17 }
});
```

attach 时必带：
```json
{
  "host_id": "...",
  "runtime_epoch": "...",
  "controller_id": "...",
  "terminal_id": "...",
  "since_output_seq": 41,
  "mode": "raw",
  "client_min_version": 1,
  "client_max_version": 1
}
```

## Rediscovery（host restart）

```
kernel 重启
    │
    ├─► 新 runtime_epoch
    │
    ├─► terminal_id 在新 epoch 内不保留为同一对象
    │
    ▼
client attach 带旧 runtime_epoch
    │
    ▼ error{code:"runtime_epoch_stale", expected:<new>, received:<old>}
    │
    ▼
client 必须：
    1. GET /v1/status → 取新 (host_id, runtime_epoch)
    2. GET /v1/domain/ptys → 查新 terminal_id
    3. 用新 epoch + 新 terminal_id 重 attach
    4. 若 terminal 已被销毁 → 提示用户重建视图
```

禁止 silently rebind 旧 `terminal_id`。identity mismatch 必显式提示。

## Kernel 端 Host Discovery

```bash
# shell 启动时
$ cat %LOCALAPPDATA%/ridge/kernel.json
{"pid":1234,"port":45678,"token":"uuid","started_at_unix":1700000000}
```

`host_id` + `runtime_epoch` 通过 `GET /v1/status` 拉取：

```rust
// rtp1_kernel_e2e.rs:fetch_host_info
GET /v1/status
  → { "host_id": "...", "runtime_epoch": "...", ... }
```

## 测试

* `stability_fault::runtime_epoch_rebind_panics` (`#[should_panic]`)：
  同一 PtyRegistry 第二次 `set_runtime_epoch` 必 panic。
* `conformance_rtp1::acceptance_runtime_epoch_stale_rejected`：
  attach 带 wrong-epoch → `RuntimeEpochStale`。
* `conformance_rtp1::acceptance_runtime_epoch_independent_of_server_version`：
  epoch 变化 ≠ server_version 变化；`runtime_epoch_rotated` event 不带
  server_version 字段。
* `rtp1_kernel_e2e::rtp1_ws_full_lifecycle`：开真实 kernel 子进程 →
  `fetch_host_info` → attach → 验证 attach_ack.runtime_epoch 与 fetch 一致。

## 详尽规范

* `specs/L2-PROTO-001.md` §3.4.3（runtime_epoch）
* `specs/L2-REMOTE-001.md` §3.5.3（host restart）
* `specs/L2-TERM-001.md` §3.1（identity invariants）
