---
id: L2-PROTO-001
level: L2
title: Terminal Wire Protocol v1
status: APPROVED
origin: authored
migration_state: PROPOSED
depends_on: [L1-PROJECT-001, L2-TERM-001]
---

## §1 范围

统一 Ridge 终端交互 wire 协议，独立于 WebSocket / WebRTC / HTTP。
覆盖范围：PTY 双向字节流、终端 grid 增量、attach/detach、replay、resync、错误。

## §2 现状事实

### §2.1 已存在的协议片段

| 协议 | 位置 | 形态 | 覆盖范围 |
|---|---|---|---|
| `bounded-seq-v1` | kernel `domain::domain_pty_output_*` | HTTP JSON：lease + frames + Lagged | kernel ↔ any HTTP client |
| `RemotePtyEvent` | `types.rs` | Tauri event + WS frame：`RawBytes{ws, pane, bytes: Arc<Vec<u8>>}` / `SemanticDelta{frame,is_alt}` / `Metadata{title,cwd}` / `Resize` | shell ↔ WS remote |
| `TerminalDeltaFrame` | `ridge_term::remote_v2` | postcard via `subscribe_pane_terminal_v2` | shell ↔ cloud pane |
| `TerminalSnapshot` | 同上 | 替换式 | 同上 |
| `TerminalKernel`（WASM） | `ridge_term::lib.rs` | TS-bridge 单机调用 | 不入 wire |
| tmux `ridge-remote-ws` | ridge-cli `mux.rs` / `rpc.rs` | 字节通道前缀 demux + JSON-RPC 2.0 | rdg controller ↔ host |

### §2.2 缺失

- 没有统一 envelope（4 套协议各自 frame 形态）。
- 无 capability 公告（`domain_meta` 仅服务端单边）。
- 无 desync/replay 在统一层的语义。

## §3 契约（Ridge Terminal Protocol v1，简称 RTP1）

### §3.1 设计原则（冻结前约束）

- **P1**：单一 authoritative negotiated protocol version；普通消息不携带 `protocol_version`。
- **P2**：input / output 序列号语义独立，identity 集合互不重叠。
- **P3**：snapshot / replay 走 bounded chunk + continuation，realtime frame 必须独立完整。
- **P4**：RTP1 要求 transport 提供 reliable + ordered + complete；application-level 完整性校验**不**由 RTP1 承担。
- **P5**：RTP1 核心语义对旧协议无回退义务。legacy 兼容在 adapter/fallback layer 闭环，不污染 RTP1 消息体。
- **P6**：每个 host kernel / runtime 进程启动生成一次 `runtime_epoch`；attach 必须经过 runtime_epoch 校验，stale attach 必拒。

### §3.2 Negotiated Protocol Version

| 字段 | 来源 | 含义 |
|---|---|---|
| `envelope_format_version` | envelope header byte `efv`（见 §3.3） | 仅描述 binary framing 的格式版本；与协议语义版本解耦 |
| `client_min_version` | `attach` 帧（wire field） | 客户端能接受的最低 negotiated protocol version |
| `client_max_version` | `attach` 帧（wire field） | 客户端能支持的最高 negotiated protocol version |
| `server_version` | `attach_ack` 帧（wire field） | 服务端在 `[client_min_version, client_max_version]` 内选定的 negotiated protocol version；本 attachment/session 期间唯一权威 |

- `envelope_format_version` ≠ `server_version`：前者是 binary frame 布局版本，后者是协议语义版本；二者与 `runtime_epoch` **完全独立**。
- 普通消息（`output` / `input` / `delta` / `replay` / `snapshot` / `session_event` / `error` / `ping` / `pong` / `title` / `cwd`）**不**带 `server_version` 字段。
- `server_version` 单调；服务端在 `attach_ack` 中一次性敲定，本 attachment/session 期间不变。
- 若 `client_max_version < server_version` → `error{code:"client_too_old"}`；无交集即协议层不兼容，attach 必拒。
- **server_version 与 runtime_epoch 解耦**：
  - server_version 改变**不等价于** runtime_epoch 改变；
  - runtime_epoch 改变**不等价于** server_version 改变；
  - 协议版本无法继续兼容时，要求 client 重新 attach 并重新协商；**不**通过 `session_event` 隐式重协商。
- `runtime_epoch_rotated`（§3.5 0x16）仅表示 host kernel 重启；**不**承载协议版本变化信号。

### §3.3 Frame Envelope

```
+--------+--------+--------+--------+----------------+
| magic  | efv    |  type  | flags  |   payload_len  |
| 4 B    | 1 B    | 1 B    | 1 B    |   4 B (LE)     |
+--------+--------+--------+--------+----------------+
|                       payload                       |
+------------------------------------------------------+

magic      = 'R' 'T' 'P' '1' (4 bytes; ASCII)
efv        = envelope_format_version；仅描述 binary framing；
            0x01 对应本 spec；envelope 升级时 +1
type       = enum (见 §3.5)
flags      = bit0: continuation (logical message 后续还有 chunk);
            bit1: end-of-stream (server→client lease close)
            bit2-7: reserved (MUST be 0)
payload_len = u32 LE; realtime frame ≤ 64 KiB（见 §3.7）
            snapshot/replay chunk ≤ 256 KiB 且必须 continuation 链
```

> transport 完整性由 WebSocket-over-TLS 或 WebRTC reliable data channel 提供（P4）。RTP1 不定义 application-level 校验字段。

### §3.4 Identity 与 Sequence 语义（P2 强制）

#### §3.4.1 Identity 集合

| 维度 | identity | 来源 |
|---|---|---|
| host | `host_id` (string) | `/info` 公告 |
| runtime_epoch | `runtime_epoch` (string, u64-as-string 或 UUID v7) | `attach_ack` 回执；每次 host kernel/runtime 启动重生 |
| session | `session_id` (string) | `host_list_sessions` 公告；跨 runtime_epoch 不稳定 |
| terminal | `terminal_id` (string) | `host_list_sessions` 内嵌；跨 runtime_epoch **不稳定** |
| controller | `controller_id` (string) | 客户端生成（UUID v4 或 v7），attach 时声明 |

`terminal_id` 在 `(host_id, runtime_epoch)` 内稳定；不同 runtime_epoch 的同 terminal 视为不同对象，客户端必须重发现（§3.6）。

#### §3.4.2 Sequence 空间

| 命名空间 | scope | 语义 | 单调性 |
|---|---|---|---|
| `output_seq` | per `(host_id, runtime_epoch, terminal_id)` | 服务端 PTY output / delta / replay 顺序 | 严格单调；reset 仅在 terminal destroy 后 |
| `input_seq` | per `(host_id, runtime_epoch, terminal_id, controller_id)` | 单 controller 的 input 顺序；不同 controller 互不重叠 | 严格单调；`controller_id` 改变必重置 |

- input 帧**只能**用 `input_seq`；output/delta/replay 帧**只能**用 `output_seq`。两者**绝不**共用。
- input_seq 在 `attach_ack` 中以 `controller_id` 绑定；客户端在 attach 时声明 controller_id，服务端回执确认。
- `input_ack.seq` 与 `input_seq` 同空间；`output` / `delta` / `replay_data` 中的 `seq` 与 `output_seq` 同空间。

#### §3.4.3 runtime_epoch（P6 强制）

- 每次 host kernel / runtime 进程启动生成一次 `runtime_epoch`（UUID v7 或单调计数）；重启 = 新 epoch。
- 客户端在 `attach` 时必须带 `runtime_epoch`（来自上一次成功的 `attach_ack` 或 `host_info`）。
- 服务端校验：
  - 匹配 → 正常 attach。
  - 不匹配（`attach.runtime_epoch != current_runtime_epoch`）→ `error{code:"runtime_epoch_stale", expected: <current>, received: <client>}`，**不**进入 `Attached`。
- 客户端必须走以下任一恢复路径：
  1. 调 `host_list_sessions` 取新 `terminal_id`，用新 `runtime_epoch` 重 attach。
  2. 若 `host_list_sessions` 不再有该 terminal → terminal 已被 host 销毁，提示用户。
- 同一 host 跨 epoch 的 terminal 不可 silently rebind；必须显式 re-attach。

### §3.5 消息类型（type 枚举）

| id | 名称 | 方向 | payload 概要 |
|---|---|---|---|
| 0x01 | `attach` | C→S | `{host_id, runtime_epoch, session_id, terminal_id, controller_id, since_output_seq?, mode, client_min_version, client_max_version}` |
| 0x02 | `attach_ack` | S→C | `{terminal_id, controller_id, server_version, runtime_epoch, mode, oldest_output_seq, next_output_seq, controller_input_seq, snapshot?: SnapshotEnvelope, capability?}` |
| 0x03 | `detach` | C→S | `{terminal_id, controller_id, reason?}` |
| 0x04 | `detach_ack` | S→C | `{terminal_id, controller_id, last_output_seq}` |
| 0x05 | `input` | C→S | `{terminal_id, controller_id, input_seq, data}` (UTF-8 / paste base64) |
| 0x06 | `input_ack` | S→C | `{terminal_id, controller_id, input_seq, status: "applied" | "rejected", reason?}` |
| 0x07 | `output` | S→C | `{terminal_id, output_seq, frames: [{seq_offset, data_b64}]}` (raw) |
| 0x08 | `delta` | S→C | `{terminal_id, output_seq, delta_bytes_b64, alt: bool}` (postcard DeltaFrame) |
| 0x09 | `resize` | C→S | `{terminal_id, controller_id, rows, cols, owner?: "controller" | "observer"}` |
| 0x0A | `resize_ack` | S→C | `{terminal_id, controller_id, rows, cols, next_output_seq}` |
| 0x0B | `replay` | C→S | `{terminal_id, since_output_seq, max_bytes}` |
| 0x0C | `replay_data` | S→C | `{terminal_id, frames: [{output_seq, data_b64}], at_oldest: bool, head_output_seq}` (continuation 链) |
| 0x0D | `snapshot` | S→C | `{terminal_id, revision, snapshot_bytes_b64}` (continuation 链) |
| 0x0E | `title` | S→C | `{terminal_id, title}` |
| 0x0F | `cwd` | S→C | `{terminal_id, cwd}` |
| 0x10 | `desync` | S→C | `{terminal_id, reason: "overflow" | "lagged" | "io_error" | "runtime_epoch_stale" | "controller_id_unknown"}` |
| 0x11 | `resync` | C→S | `{terminal_id, mode, since_output_seq?}` |
| 0x12 | `error` | S→C / C→S | `{terminal_id?, controller_id?, code, message}` |
| 0x13 | `ping` | 双向 | `{nonce}` |
| 0x14 | `pong` | 双向 | `{nonce}` |
| 0x15 | `capability_advertise` | S→C | `{terminal_id?, features: Vec<string>, max_realtime_frame, max_snapshot_chunk, replay_cap_bytes, replay_cap_frames}` |
| 0x16 | `session_event` | S→C | `{terminal_id, event: "exited" | "starting" | "runtime_epoch_rotated", code?}` |

> 所有 C→S 帧必带 `host_id` + `runtime_epoch`（除 `attach` 自身声明 `runtime_epoch` 外，其他帧的 `runtime_epoch` 由 `attach_ack` 绑定，服务端校验）。
> 所有 S→C 帧服务端代填 `runtime_epoch`，客户端不需解析。

### §3.6 模式字段

- `mode: "raw"`：服务端发 `output`；客户端发 `input`；不消费 parser。
- `mode: "delta"`：服务端发 `delta`（postcard-encoded `TerminalDeltaFrame`）。
- `mode: "snapshot"`：服务端先发 `snapshot`（bounded chunks）再发 `delta`；客户端可仅消费 snapshot。
- `since_output_seq`：attach 时回放起点；缺省 = 从 `oldest_output_seq` 起。

### §3.7 Realtime / Snapshot / Replay 大小与 Chunk 重组（P3 强制）

| 类别 | 上限 | 续传 |
|---|---|---|
| Realtime frame（`output` / `input` / `delta` / `title` / `cwd` / `error` / `ping` / `pong`） | `payload_len` ≤ 64 KiB | **禁止** continuation |
| `snapshot` chunk | `payload_len` ≤ 256 KiB | continuation bit 链；唯一边界信号 = `flags.continuation` |
| `replay_data` chunk | `payload_len` ≤ 256 KiB | continuation bit 链 |
| 单次 `replay` 请求的累计回放 | 由 `capability_advertise.replay_cap_bytes` 限制（参考 kernel `OUTPUT_REPLAY_CAP_BYTES`） | 服务端按 oldest → newest 顺序回放，cursor 不连续时返回 `desync{reason:"lagged"}` |

**realtime frame 必为独立完整 message**。PTY 大输出由服务端拆成多个 `output` 帧，每帧 ≤ 64 KiB、拥有连续 `output_seq`。客户端不恢复也不假设 PTY read boundary；realtime 帧禁 continuation。

**Chunked Logical Message 重组规则**：

- 客户端只允许同一 `(terminal_id, message_type)` 同时存在一个 in-flight chunked logical message。
- `flags.continuation = 1`：当前 chunk 后续还有同 logical message 的 chunk。
- `flags.continuation = 0`：当前 chunk 为该 logical message 的末块。
- 若同一 `(terminal_id, message_type)` 的上一个 logical message 尚未结束（`continuation=0` 未到），又收到新首块（`continuation=1`） → `error{code:"protocol_violation"}`，必拒。
- RTP1 **不支持**同 `(terminal_id, message_type)` 多 chunked logical message 并发交错；并发区分由 `(terminal_id, message_type)` 唯一对保证。
- 边界信号唯一为 `flags.continuation`；**不**通过 payload size 推断 message 边界。
- 客户端重组时按到达顺序合并到当前 logical message buffer；`continuation=0` 时一次性 flush。

> 不引入 `message_id`；RTP1 优先保持简单。

### §3.8 完整性（P4 强制）

- RTP1 **不**提供 application-level CRC。
- RTP1 要求 transport 提供 reliable + ordered + complete 的数据通道（WebSocket-over-TLS / WebRTC reliable data channel）。
- 未来如需 application-level 校验，通过新 `envelope_format_version` 引入；不在 RTP1 范围内动态协商。

### §3.9 兼容性策略（P5 强制）

- RTP1 核心语义不假设旧客户端能理解任何 RTP1 消息。
- 旧协议（`bounded-seq-v1` HTTP、LAN 旧 WS、tmux 私协议）通过 **adapter / fallback layer** 兼容：
  - adapter 仅存在于 transport boundary；将旧消息 ↔ RTP1 帧转换；
  - adapter 不修改 RTP1 核心字段语义；不在 RTP1 帧中携带旧协议字段；
  - adapter 失败 → 拆 RTP1 链路，不污染 RTP1。
- 新 client → 旧 server：客户端 connect 后只发 RTP1 `attach`；收不到 `attach_ack` 且无明确 RTP1 错误 → 客户端可选进入"无 RTP1 模式"（直连旧 adapter 端点）；**不**假设旧服务端能理解 `error`。
- 旧 client → 新 server：旧客户端在 transport boundary 拒认 magic → adapter 路径失败 → 客户端回退到旧协议直连（如 `bounded-seq-v1` HTTP）。
- minor 扩展：envelope `efv` 不变；新字段通过 postcard 默认行为（未知字段忽略）实现；新 message type 通过 `capability_advertise` 公告。
- breaking 变更：`efv` +1；旧客户端在 transport boundary 拒认 → adapter 回退到旧路径。

### §3.10 错误码（`error` payload）

| code | 含义 | 触发 |
|---|---|---|
| `unknown_envelope` | magic 或 `efv` 不识别 | transport boundary |
| `unknown_message` | type 字段不识别 | payload 解码后 |
| `unknown_terminal` | terminal_id 不存在或已 detach | 服务端按 host session 状态查表 |
| `runtime_epoch_stale` | attach 时 `runtime_epoch` 不匹配 | §3.4.3 |
| `controller_id_unknown` | input 帧带未 attach 的 controller_id | 服务端 lane 查表 |
| `attach_denied` | 权限或并发限制拒绝 | 鉴权层 |
| `input_too_large` | 单帧 input > `max_realtime_frame` | §3.7 |
| `client_too_old` | `client_max_version < server_version` | §3.2 |
| `server_overloaded` | 服务端背压拒收 | 资源耗尽 |
| `protocol_violation` | 顺序 / 必填字段缺失 / 校验失败 | 解码层 |
| `io_error` | PTY 子进程级错误 | kernel PtyRegistry |
| `replay_unavailable` | replay 数据已被 GC | kernel `PtyOutputHub` 环形丢头 |

> `error` 帧**不**用作跨协议兼容带（与 §3.9 一致）。

### §3.11 复用既有实现

- `TerminalDeltaFrame` 编码/解码复用 `ridge_term::remote_v2`。
- `PtyOutputLease` 的 `next_seq/oldest_seq` 字段语义直接复用，映射到 `output_seq`。
- `runtime_epoch` 生成策略详见 SPEC-REMOTE-001 §3.5（UUID v7，与 authentication token 解耦）。
- `attach_ack.snapshot` 可选字段使用 `TerminalSnapshot` postcard bytes，分块走 §3.7 continuation。
- `pty_input_lanes` 的 `(ws, pane, src)` 索引直接映射 `(terminal_id, controller_id)`。

## §4 端到端 Acceptance

1. **跨 transport 一致**：同一 `printf` 输出经 WebSocket 与 WebRTC（envelope 相同，wire bytes 不同）渲染结果一致。
2. **协议版本协商独立**：
   - `client_min_version` / `client_max_version` 正常协商；
   - 服务端在 `[min, max]` 内回执 `server_version`；
   - 无交集 → `error{code:"client_too_old"}`；
   - **mode negotiation 独立验证**：`attach.mode`（`raw` / `delta` / `snapshot`）由 `attach` / `attach_ack` 显式声明，不被 `server_version` 隐式改变；
   - 协议版本协商不触发 raw/delta/snapshot mode 切换。
3. **无 application-level CRC**：同一 `printf` 在 WS+TLS 与 WebRTC 两种 transport 下均渲染一致；RTP1 frame 无 CRC 字段。
4. **背压**：PTY 突发 1 MiB + 客户端 slow consumer → 服务端拆 ≤ 64 KiB `output` 帧（每帧独立完整）+ 必要时 `desync{reason:"overflow"}`，客户端调 `replay` 后 grid 等价。
5. **input_seq 独立**：客户端连续发 3 帧 `input`，服务端按 `input_seq` 应用；乱序则 `error{code:"protocol_violation"}`；output_seq 与 input_seq 互不干扰。
6. **terminal exited event 必达**：terminal 进入 `Exited` → 所有 attached controller ≤ 1 s 收到 `session_event{event:"exited", terminal_id, code?}`。
7. **runtime_epoch stale 拒绝**：客户端带旧 `runtime_epoch` attach → 立即 `error{code:"runtime_epoch_stale"}`，不进入 `Attached`。
8. **controller_id 隔离**：两个 controller 同 terminal 同时 input → 两 lane `input_seq` 互不干扰；某 controller 单独 `input_ack` 不混淆。
9. **snapshot 续传**：256 KiB snapshot 拆 N 块，唯一边界信号 = `flags.continuation`；客户端按序重组后 grid 完整。
10. **realtime frame 限速**：客户端发送 1 MiB `input` 单帧 → 服务端 `error{code:"input_too_large"}`；客户端拆 ≤ 64 KiB 后成功。
11. **chunked logical message 互斥**：服务端并发发同一 `(terminal_id, message_type)` 多 snapshot logical message → 客户端视为 `protocol_violation`，不重组。
12. **server_version 与 runtime_epoch 解耦**：runtime_epoch 改变**不**伴随 server_version 变化；client 收到的 `runtime_epoch_rotated` 事件必不含 `server_version` 字段。

## §5 与 SPEC-REMOTE-001 关系

- 本 spec 描述 wire 层；SPEC-REMOTE-001 描述状态机层。
- 任何 attach/detach 在 wire 上对应 `attach/attach_ack/detach/detach_ack`；状态机记录在 `(host_id, runtime_epoch, session_id)` 上而非 wire。
- `runtime_epoch` 是 SPEC-REMOTE-001 §3.4.3（Host 重启）场景的权威信号；新 epoch = host 重启；runtime_epoch 生成与 authentication token 解耦（见 SPEC-REMOTE-001 §3.5）。
- input_seq identity `(terminal_id, controller_id)` 与 SPEC-REMOTE-001 §3.4.5 Input Ownership 字段一致。
