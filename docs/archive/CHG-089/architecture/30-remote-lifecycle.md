# 30 — Remote Lifecycle

> 模块：`packages/ridge-kernel/src/rtp1_session.rs` + `pty.rs`（lifecycle）
> 测试：`packages/ridge-kernel/tests/stability_fault.rs` + `conformance_rtp1.rs`
> 详尽规范：`specs/L2-REMOTE-001.md`

Remote lifecycle 包含**两个独立状态机**：

1. **Terminal lifecycle**：`Starting → Running → Exited → Reaped`（PTY 进程）
2. **Connection state machine**（per-controller）：
   `Detached → Connecting → Attached → Reconnecting → Desynced → Closing/Failed`

## Terminal lifecycle（SPEC §3.1）

```
        spawn_command_for_with_env
                  ↓
            ┌──────────┐
            │ Starting │  ← reader task 未收到首字节
            └────┬─────┘
                 │ first byte
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

转换不变量：
* `Starting → Running` ≤ 5 s；超时按 `Exited` 处理（带 reason=`start_timeout`）。
* `Exited` 不可回退；`session_event{event:"exited"}` 必发。
* `Reaped` 后 attach 必返 `error{code:"unknown_terminal"}`。
* **v1 不**支持 `Idle`（基于"无输出"推导）；suspend/idle 留待后续 spec。
* terminal 进入 `Exited` **不**驱动 attachment 状态机；attachment 维持
  `Attached` 直至显式 `detach` / client destroy / auth revoke / host policy。
  已 Exited 的 terminal 在 attached 期间：input 必拒（`session_closed`），
  replay / snapshot 仍允许。

实现细节：
* Reader task 在第一次 publish 时把 lifecycle 从 `Starting` 转到 `Running`。
* Reader task 结束（PTY EOF）时把 lifecycle 转到 `Exited`，并通过
  `PtyExitNotification` broadcast 派发。
* `finish_destroy` 把 lifecycle 强制设为 `Reaped`（内核回收）。
* `subscribe_exit(pty_id)` 返回 broadcast receiver；attachment state machine
  通过该 receiver 在 output pump 中派生 `session_event{event:"exited"}` 帧。

## Connection state machine（SPEC §3.2）

```
              ┌──────────┐
              │ Detached │  ← 初始 / 显式 detach / server refused
              └────┬─────┘
                   │ C→S attach
                   ↓
              ┌──────────┐
              │Connecting│  ← 等待 attach_ack；握手超时 ≤ 5 s
              └────┬─────┘
                   │ attach_ack received             │ 超时 / error / reset
                   ↓                                ↓
              ┌───────────←───────────────────────────┘
              │  Attached │  ← 已 attach；live stream 推流中
              └────┬─────┘
                   │ network failure / heartbeat timeout
                   ↓
              ┌────────────┐
              │Reconnecting│  ← 自动重连尝试；指数退避（1s → 30s 上限）
              └────┬───────┘
                   │ 重连时重新进入 attach 握手
                   ↓ (overlap: detect seq gap → Desynced)
              ┌──────────┐
              │ Attached │  ← attach_ack 携带 since_output_seq 续流
              └────┬─────┘
                   │ consumer cursor 落后 oldest_seq / version mismatch
                   ↓
              ┌──────────┐
              │ Desynced │  ← 强制 full resync（snapshot 或 RIS + replay）
              └────┬─────┘
                   │ resync_ack
                   ↓
              ┌──────────┐
              │ Attached │
              └────┬─────┘
                   │ client destroy
                   ↓
              ┌──────────┐
              │ Closing  │  ← 主动 disconnect（客户端 tear down）
              └────┬─────┘
                   │ server detach_ack
                   ↓
              ┌──────────┐
              │ Detached │
              └──────────┘

终态：
              ┌──────────┐
              │  Failed  │  ← auth fail / revoked / 401 / server explicit deny
              └──────────┘
```

### 转换矩阵

| from | to | 触发 | 必发消息 |
|---|---|---|---|
| Detached | Connecting | client attach | `attach` |
| Connecting | Attached | server attach_ack | `attach_ack` |
| Connecting | Failed | auth/perm/refuse | `error` |
| Connecting | Failed | `attach.runtime_epoch` 不匹配 | `error{code:"runtime_epoch_stale"}` |
| Connecting | Detached | client cancel | — |
| Attached | Reconnecting | heartbeat 3 次未回 / WS close | （可能不发） |
| Reconnecting | Connecting | 重连时重新进入 attach 握手 | `attach` |
| Reconnecting | Attached | attach_ack | `attach_ack` |
| Reconnecting | Failed | retry 超限（30 s 上限 5 次） | `error{code:"server_overloaded"}` |
| Attached | Desynced | `output_seq` 落后 / `runtime_epoch` mismatch | `desync` |
| Desynced | Attached | resync_ack | `resync` + `snapshot`/`replay_data` |
| Attached | Closing | client destroy | `detach` |
| Closing | Detached | server detach_ack | `detach_ack` |
| Any | Failed | auth revoke / blacklist | `error{code:"auth_revoked"}` |

## 关键场景行为（SPEC §3.5）

### §3.5.1 — 网络瞬断（≤ 30 s）

1. 检测：WS heartbeat 3 次未回（默认 5 s 间隔）。
2. 客户端进入 `Reconnecting`，**不断开 terminal**。
3. 重连成功后自动发 `attach`，带 `since_output_seq = last_received_output_seq`。
4. 服务端比对 seq：
   * 连续（last+1..head） → 仅发 `output` 增量。
   * 中间有洞 → 服务端发 `desync{reason:"lagged"}`，客户端进入 `Desynced`，自动 `replay` → `snapshot`。

### §3.5.2 — 客户端重启

1. 客户端本地的 `(host_id, runtime_epoch, terminal_id, controller_id) → output_seq` 持久化。
2. 重连后 `attach` 带 `since_output_seq`；服务端走 §3.5.1 第 4 步。

### §3.5.3 — Host 重启（kernel 重启 = 新 runtime_epoch）

1. kernel 重启 → 生成新 `runtime_epoch`；旧 `terminal_id` 在新 epoch 内**不**保留为同一对象。
2. 客户端 attach 带旧 `runtime_epoch` → 服务端 `error{code:"runtime_epoch_stale", expected, received}`，**不**进入 `Attached`。
3. 客户端必须走以下恢复：
   1. 调 `host_list_sessions`（在新 epoch 下）取新 `terminal_id`。
   2. 用新 `runtime_epoch` 重新 attach；若同 `host_id` + 新 `terminal_id` 仍指向同 pane 内容，绑定新对象。
   3. 若 `host_list_sessions` 不再有该 terminal → 终端已被销毁，客户端必须 detach 并提示用户重建视图。
4. **不**允许 silently rebind；identity mismatch 必显式提示。
5. **不**承诺"空屏闪一次即恢复"；旧 `terminal_id` 视为不存在。

### §3.5.4 — Terminal Exited 与 Session 关系

* terminal 进入 `Exited` 时，**所有**当前 `Attached` controller **必**收到
  `session_event{event:"exited", code, terminal_id}`。
* terminal exit **不**等价于 session exit（v1 单 terminal session binding 未声明）。
* terminal `Exited` 后仍允许 client 端发 `replay` / `snapshot`；`input` 必拒。
* 重连后：服务端在 `attach_ack` 中按需回放 `session_event`。

### §3.5.5 — Input Ownership 冲突

* 同一 host terminal 被多个 controller attach 时，`input` 帧必携带 `controller_id`。
* 服务端按 `(host_id, runtime_epoch, terminal_id, controller_id)` 维护 input lane。
* resize 同理带 `owner: "controller" | "observer"`；observer 不能 resize。
* `controller_id` 是 **attachment / controller identity**（UUID v4 或 v7 均可），**不**携带顺序语义；input 顺序由 `input_seq` 保证。
* `controller_id` 在 `(host_id, runtime_epoch, terminal_id)` scope 内**唯一**。

## 实现映射

| 模块 | 角色 |
|---|---|
| `packages/ridge-kernel/src/pty.rs` | terminal lifecycle + exit broadcast + per-pty controllers |
| `packages/ridge-kernel/src/rtp1_session.rs` | attachment state machine + per-controller `AttachmentRegistry` |
| `packages/ridge-kernel/src/rtp1_ws.rs` | WS ↔ RTP1 适配，attach/detach 路由 + output pump + session_event 派发 |

## 测试

* `stability_fault::repeated_attach_detach_does_not_leak_state` — 200 attach/detach 周期
* `stability_fault::resize_storm_does_not_panic_or_leak` — 1000 resize
* `stability_fault::multi_terminal_stress_isolates_per_pty_state` — 16 PTY × 64 cycle
* `stability_fault::exit_notification_delivered_to_subscribers` — broadcast 多订阅
* `conformance_rtp1::stability_reconnect_resume_from_since_output_seq` — 重连续流
* `conformance_rtp1::stability_host_restart_rejects_stale_epoch` — host 重启路径
* `stability_fault::input_seq_wire_validation_*` — per-controller 输入所有权

## 详尽规范

完整 wire + state machine 规范见 `specs/L2-REMOTE-001.md`。
