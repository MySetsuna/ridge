---
id: L2-REMOTE-001
level: L2
title: Remote Session Lifecycle
status: APPROVED
origin: authored
migration_state: PROPOSED
depends_on: [L1-PROJECT-001, L2-TERM-001, L2-PROTO-001]
---

## §1 范围

定义 host kernel 上的 **terminal lifecycle** 与 **connection attachment** 两个独立状态机；并明确 **session** 是 terminal 之上的一个轻量聚合。
关键不变量：
- **网络断开 ≠ terminal 消失**。
- **host 重启 = 新 `runtime_epoch` ≠ 旧 terminal 自动续命**（identity 必须显式重发现）。
- **terminal exit 不自动等价于 session exit**（除非该 session 内只有一个 terminal，由 host 端显式记录并在本 spec 范围内声明）。

## §2 现状事实

### §2.1 三类 Terminal / Session 抽象（按代码）

| 抽象 | owner | 持久化 | 进程归属 |
|---|---|---|---|
| **Terminal**（`kernel PtyRegistry` 中的 PTY 进程） | kernel | 否（进程级内存） | kernel 进程 |
| **tmux session**（`ridge-tmux::NativeServer`） | shell（ridge-tmux 全局 Mutex） | 否 | shell 进程 |
| **Host session 列表**（`HostRecord.sessions[]`） | kernel（attached flag）/ shell（live_sink + outbound client + history） | remote-hosts.json（kernel 投影） | 进程分离 |

> 本 spec 中 "Terminal" 与 PTY 进程同义（SPEC-TERM-001 §3.1）。
> "Session" 指 host 端聚合多个 terminal 的轻量记录；v1 仅给最小定义（§3.3），不强制绑定具体实现。

### §2.2 已存在的连接管理

| 模块 | 行为 |
|---|---|
| `OutboundClient`（hosts/outbound.rs） | 状态机：`Init → HelloSent → Listed → Subscribed → Disconnected`；带 hello/list/subscribe/write/resize/unsubscribe/detach；reconnect 由 `ReconnectSupervisor` 调度 |
| `ReconnectSupervisor` | 4 阶段：`Init → Backoff → Probing → Succeeded/Idle`；可 cancel；health 检测 |
| `RemoteClientRegistry`（state.rs） | 仅记录 connected 远程客户端的元数据（id/addr/UA/device_id/token/kill_flag）；无 input ownership 角色 |
| `LiveInputSink` | 单 host session 单 sink；写入语义是 "register one closure" |
| `cloud_pane_raw_subs` | refcount；activation-scoped envelope（多 controller 共享 live bytes） |
| `ForeignHistoryStore` | attach seed；保留历史 tail 给重连 |

### §2.3 缺失

- 显式 attachment 状态机（当前仅靠 `subscribed` boolean + sink 存在性推断）。
- 客户端重连时无"基于 seq resume"的明确协议（部分字段存在但未贯通）。
- 无 host session 退出后的客户端通知。

## §3 契约

### §3.1 Terminal Runtime State

```
+-------------+
|  Starting   |  ← PtyRegistry.spawn_command 提交成功，等待 PTY 子进程首字节
+------+------+
       |  first byte / setup complete
       v
+-------------+
|  Running    |  ← PTY 子进程活动，可读写
+------+------+
       |  PTY 子进程退出 (EOF / signaled)
       v
+-------------+
|   Exited    |  ← exit_code 已知；scrollback 与 lease cursor 仍可读
+------+------+
       |  lease 全部 detach + GC timeout
       v
+-------------+
|   Reaped    |  ← 内核回收（lease、scrollback、PtyRegistry entry）
+-------------+
```

转换不变量：
- `Starting → Running` 必在 ≤ 5 s 内完成；超时按 `Exited` 处理（带 reason=`start_timeout`）。
- `Exited` 不可回退；`session_event{event:"exited"}` 必发。
- `Reaped` 后任何 `attach` 必返 `error{code:"unknown_terminal"}`。
- **v1 不**支持 `Idle`（基于"无输出"推导）；不提供 fake-echo 唤醒语义。suspend/idle 留待后续 spec。

### §3.2 Connection / Attachment State（per-controller）

```
+-------------+
|   Detached  |  ← 初始 / 显式 detach / server refused
+------+------+
       |  C→S attach (with host_id, session_id, terminal_id, mode)
       v
+-------------+
|  Connecting |  ← 等待 attach_ack；握手超时 ≤ 5 s
+------+------+                ↓ 超时 / error / reset
       |  attach_ack received                |
       v                                     |
+-------------<------------------------------+
|   Attached  |  ← 已 attach；live stream 推流中
+------+------+
       |  network failure / heartbeat timeout
       v
+-------------+
|Reconnecting |  ← 自动重连尝试；指数退避（1s → 30s 上限）
+------+------+
       |  attach_ack on reconnect
       v   ↘ (overlap: detect seq gap → Desynced)
+-------------+
|  Attached   |
+------+------+
       |  consumer cursor 落后 oldest_seq / version mismatch
       v
+-------------+
|  Desynced   |  ← 强制 full resync（snapshot 或 RIS + replay）
+------+------+
       |  resync_ack
       v
+-------------+
|  Attached   |
+------+------+

终态：
+-------------+
|   Failed    |  ← auth fail / revoked / 401 / server explicit deny
+-------------+

+-------------+
|  Closing    |  ← 主动 disconnect（客户端 tear down）
+-------------+
```

### §3.3 状态转换矩阵

| from | to | 触发 | 必发消息 |
|---|---|---|---|
| Detached | Connecting | client attach | `attach` |
| Connecting | Attached | server attach_ack | `attach_ack` |
| Connecting | Failed | auth/perm/refuse | `error` |
| Connecting | Failed | `attach.runtime_epoch` 与 current 不匹配 | `error{code:"runtime_epoch_stale"}` |
| Connecting | Detached | client cancel | — |
| Attached | Reconnecting | heartbeat 3 次未回 / WS close | （可能不发） |
| Reconnecting | Connecting | 重连时重新进入 attach 握手 | `attach` |
| Reconnecting | Attached | attach_ack | `attach_ack` |
| Reconnecting | Failed | retry 超限（30 s 上限 5 次） | `error{code:"server_overloaded"}` |
| Attached | Desynced | `output_seq` 落后 / `runtime_epoch` mismatch（仅当 layer 漏掉 attach 期校验时） | `desync` |
| Desynced | Attached | resync_ack | `resync` + `snapshot`/`replay_data` |
| Attached | Closing | client destroy | `detach` |
| Closing | Detached | server detach_ack | `detach_ack` |
| Any | Failed | auth revoke / blacklist | `error{code:"auth_revoked"}` |

> **terminal 进入 `Exited` 不驱动 attachment 状态机**。attachment 维持 `Attached` 状态直至显式 `detach` / client destroy / auth revoke / host policy。已 Exited 的 terminal 在 attached 期间：input 必拒（`error{code:"session_closed"}`），replay / snapshot 仍允许。

### §3.4 Session Runtime State（最小定义）

```
+----------+
|  Active  |  ← session 内仍有至少一个 terminal，且未显式 close
+---+------+
    |  host 显式 close / 最后 terminal 退出（仅限 host 声明 single-terminal session）
    v
+----------+
|  Closed  |  ← 不再接受 attach / 任何 input
+----------+
```

> v1 最小定义：仅 `Active` / `Closed`。
> **terminal exit 不自动等价于 session exit**。仅当 host 显式声明该 session 内只有一个 terminal 且该 terminal 进入 `Exited`，session 才允许随之进入 `Closed`；当前代码未实现该声明，本 spec 不强制该绑定。
> session 列表来源：`HostRecord.sessions[]`（见 §2.1）。

### §3.5 关键场景行为

#### §3.5.1 网络瞬断（≤ 30 s）

1. 检测：WS heartbeat 3 次未回（默认 5 s 间隔）。
2. 客户端进入 `Reconnecting`，**不断开 terminal**。
3. 重连成功后自动发 `attach`，带 `since_output_seq = last_received_output_seq`。
4. 服务端比对 seq：
   - 连续（last+1..head） → 仅发 `output` 增量；
   - 中间有洞 → 服务端发 `desync{reason:"lagged"}`，客户端进入 `Desynced`，自动 `replay` → `snapshot`。

#### §3.5.2 客户端重启

1. 客户端本地的 `(host_id, runtime_epoch, terminal_id, controller_id) → output_seq` 记录持久化（localStorage）。
2. 重连后 `attach` 带 `since_output_seq`；服务端走 §3.5.1 第 4 步。

#### §3.5.3 Host 重启（kernel 重启 = 新 runtime_epoch）

1. kernel 重启 → 生成新 `runtime_epoch`（与 authentication token **解耦**；详见 §3.6）；旧 `terminal_id` 在新 epoch 内**不**保留为同一对象。
2. 客户端 attach 带旧 `runtime_epoch` → 服务端 `error{code:"runtime_epoch_stale", expected, received}`，**不**进入 `Attached`。
3. 客户端必须走以下恢复：
   1. 调 `host_list_sessions`（在新 epoch 下）取新 `terminal_id`；
   2. 用新 `runtime_epoch` 重新 attach；若同 `host_id` + 新 `terminal_id` 仍指向同 pane 内容（用户可识别），绑定新对象；
   3. 若 `host_list_sessions` 不再有该 terminal → 终端已被销毁，客户端必须 detach 并提示用户重建视图。
4. **不**允许 silently rebind 旧 terminal_id；identity mismatch 必显式提示。
5. 客户端可保留 `host_id` 与 pane 元数据（cwd、title）做 best-effort 提示，但不参与 wire identity。
6. **不**承诺"空屏闪一次即恢复"——旧 terminal_id 视为不存在；用户可能看到"session 已结束"提示，需重发现。

#### §3.5.4 Terminal Exited 与 Session 关系

- terminal 进入 `Exited` 时，**所有**当前 `Attached` controller **必**收到 `session_event{event:"exited", code, terminal_id}`（SPEC-PROTO-001 §3.5 0x16）。
- terminal exit **不**等价于 session exit（见 §3.4）。
- terminal `Exited` 后仍允许 client 端发 `replay` / `snapshot`（用于查看最终输出 / scrollback）；`input` 必拒（`error{code:"session_closed"}`）。
- 若 host 端已声明 single-terminal session 且该唯一 terminal `Exited` → session 转入 `Closed`，后续 `attach` 必拒。
- 若 v1 未实现 single-terminal session binding（当前实现未声明），session 保持 `Active`；client 仅收到 `terminal exited` 事件，不收额外 `session Closed` 事件。
- 重连后：服务端在 `attach_ack` 中按需回放 `session_event`。
- terminal `Exited` **不**触发 attachment `Closing` 状态（见 §3.3）；attachment 维持 `Attached` 直到显式 `detach` / client destroy / auth revoke / host policy。

#### §3.5.5 Input Ownership 冲突

- 同一 host terminal 被多个 controller attach 时，`input` 帧必携带 `controller_id`。
- 服务端按 `(host_id, runtime_epoch, terminal_id, controller_id)` 维护 input lane（参考 `pty_input_lanes` 既有实现 `(ws, pane, src)` 索引）。
- resize 同理带 `owner: "controller" | "observer"`；observer 不能 resize。
- `controller_id` 是 **attachment / controller identity**（UUID v4 或 v7 均可），**不**携带顺序语义；input 顺序由 `input_seq` 保证（SPEC-PROTO-001 §3.4.2）。
- `controller_id` 在 `(host_id, runtime_epoch, terminal_id)` scope 内**唯一**；runtime_epoch 改变必重新 attach 并生成新的 controller_id binding（scope 改变）。
- 详细角色化授权不在本 spec 范围（仅定义 wire 字段）。

### §3.6 复用既有实现

- kernel `PtyRegistry.begin_destroy/destroy/cancel_destroy/finish_destroy` 状态机等价于 `Starting/Running/Exited/Reaped`。
- `ReconnectSupervisor` 的阶段映射到 `Reconnecting`。
- `ForeignHistoryStore` 用作 `snapshot` 后备。
- `OutboundClient` 状态可对外暴露为 `attach_state`。
- `runtime_epoch` 由 host kernel 在每次启动时生成 UUID v7（与 authentication token **完全解耦**）；kernel `KernelBootGuard` 跨进程唯一性保障 epoch 不冲突。
- `pty_input_lanes: (ws, pane, src)` 索引直接映射 `(host_id, runtime_epoch, terminal_id, controller_id)`，scope 由 attach 绑定。

### §3.7 Acceptance

1. **断开恢复**：断网 10 s 后恢复（runtime_epoch 未变） → 用户无感（grid 完整、scrollback 一致）。
2. **大量丢包**：连续 1 MiB 输出 + 网络丢包 5%（runtime_epoch 未变） → 客户端进入 Desynced → 重建后 grid 与 baseline 等价。
3. **host 重启（kernel kill -9）**：新 `runtime_epoch` 生效 → 旧 attach 立即 `error{code:"runtime_epoch_stale"}` → 客户端走 rediscovery；不承诺 silent rebind、不承诺旧 terminal snapshot resync。
4. **terminal 退出通知 + only-read attached**：terminal 进程被 kill → 所有 attached controller ≤ 2 s 收到 `session_event{event:"exited", terminal_id}`；attachment 维持 `Attached`，client 仍可发 `replay` / `snapshot`；input 必拒（`error{code:"session_closed"}`）。
5. **input 唯一性**：两台 controller 同按 → 字符不交叉；input_seq 与 controller_id 解耦。
6. **resize 冲突**：observer resize → `error{code:"permission_denied"}`。
7. **session 状态最小性**：v1 内单个 terminal `Exited` 不会强制 session `Closed`；除非 host 显式声明 single-terminal session。
8. **attachment 不因 terminal exit 自动 closing**：terminal `Exited` 后 attachment 仍 `Attached`，直至显式 detach / client destroy / auth revoke / host policy。
9. **controller_id 非顺序**：controller_id 不承担单调/递增语义；input 顺序仅由 `input_seq` 表达。

## §4 与 SPEC-PROTO-001 关系

本 spec 描述状态机层；SPEC-PROTO-001 描述 wire 层。
任何 attach/detach 在 wire 上对应 `attach/attach_ack/detach/detach_ack`；状态机记录在 `(host_id, runtime_epoch, terminal_id)` 上而非 wire。
