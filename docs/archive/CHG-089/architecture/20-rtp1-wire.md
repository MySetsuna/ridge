# 20 — RTP1 wire 协议

> 模块：`packages/ridge-kernel/src/rtp1.rs`
> 测试：`packages/ridge-kernel/tests/conformance_rtp1.rs`（12 acceptance）+ `packages/ridge-cli/tests/rtp1_kernel_e2e.rs`（live）

RTP1（**R**idge **T**erminal **P**rotocol v1）是 kernel ↔ remote 间的
**canonical wire 协议**。所有 22 消息类型 + envelope 编码由
`SPEC-L2-PROTO-001` 冻结。

## 设计原则（SPEC §3.1）

| # | 原则 |
|---|---|
| P1 | 单一 authoritative negotiated protocol version；普通消息不携带 `protocol_version` |
| P2 | input / output 序列号命名空间独立，identity 集合互不重叠 |
| P3 | snapshot / replay 走 bounded chunk + continuation；realtime 帧独立完整 |
| P4 | RTP1 要求 transport 提供 reliable + ordered + complete；不定义 application-level CRC |
| P5 | 旧协议（bounded-seq-v1 / LAN 旧 WS / rdg 私协议）只作 adapter，不污染 RTP1 |
| P6 | host kernel 启动生成一次 `runtime_epoch`；stale attach 必拒 |

## Frame envelope（SPEC §3.3）

```
+--------+--------+--------+--------+----------------+
| magic  | efv    |  type  | flags  |   payload_len  |
| 4 B    | 1 B    | 1 B    | 1 B    |   4 B (LE)     |
+--------+--------+--------+--------+----------------+
|                       payload                       |
+------------------------------------------------------+

magic      = 'R','T','P','1' (ASCII)
efv        = 0x01 (envelope format version)
type       = enum (见 §3.5 / 22 类型)
flags      = bit0: continuation
             bit1: end-of-stream (server→client lease close)
             bit2-7: reserved (MUST be 0)
payload_len = u32 LE; realtime frame ≤ 64 KiB (见 §3.7)
```

## 22 消息类型（SPEC §3.5）

| id | 名称 | 方向 | 简述 |
|---|---|---|---|
| 0x01 | `attach` | C→S | attach 请求，含 `host_id` / `runtime_epoch` / `controller_id` / `mode` / version |
| 0x02 | `attach_ack` | S→C | attach 回执，含 `server_version` / `oldest_output_seq` / `next_output_seq` / `controller_input_seq` |
| 0x03 | `detach` | C→S | 显式 detach，可选 `reason` |
| 0x04 | `detach_ack` | S→C | detach 回执，含 `last_output_seq` |
| 0x05 | `input` | C→S | 输入字节（base64），含 `input_seq` / `controller_id` |
| 0x06 | `input_ack` | S→C | `applied` / `rejected` |
| 0x07 | `output` | S→C | 原始 PTY 字节（base64），frames 数组每项含 `seq_offset` |
| 0x08 | `delta` | S→C | postcard `DeltaFrame` 字节（base64），含 `alt` flag |
| 0x09 | `resize` | C→S | resize 请求，`owner: controller\|observer` |
| 0x0A | `resize_ack` | S→C | resize 回执，含 `next_output_seq` |
| 0x0B | `replay` | C→S | 请求从 `since_output_seq` 回放 |
| 0x0C | `replay_data` | S→C | 回放数据 frames + `at_oldest` + `head_output_seq` |
| 0x0D | `snapshot` | S→C | snapshot chunk（continuation 链） |
| 0x0E | `title` | S→C | OSC 0/1/2 标题 |
| 0x0F | `cwd` | S→C | OSC 7 cwd |
| 0x10 | `desync` | S→C | resync 触发：overflow / lagged / io_error / runtime_epoch_stale / controller_id_unknown |
| 0x11 | `resync` | C→S | 客户端请求 resync（指定 `mode`） |
| 0x12 | `error` | 双向 | 错误码 + message |
| 0x13 | `ping` | 双向 | nonce |
| 0x14 | `pong` | 双向 | nonce |
| 0x15 | `capability_advertise` | S→C | 服务端 capability：`features` / `max_realtime_frame` / `max_snapshot_chunk` / `replay_cap_*` |
| 0x16 | `session_event` | S→C | `exited` / `starting` / `runtime_epoch_rotated` |

## Identity 与 Sequence 命名空间（SPEC §3.4）

### Identity

| 维度 | identity | 来源 |
|---|---|---|
| host | `host_id` | `/info` 公告 |
| runtime_epoch | `runtime_epoch` | `attach_ack` 回执 |
| session | `session_id` | `host_list_sessions` 公告 |
| terminal | `terminal_id` | per-host `(host_id, runtime_epoch)` 内稳定 |
| controller | `controller_id` | 客户端生成 UUID v4/v7，attach 时声明 |

### Sequence 空间

| 命名空间 | scope | 单调性 |
|---|---|---|
| `output_seq` | per `(host_id, runtime_epoch, terminal_id)` | 严格单调；reset 仅在 terminal destroy 后 |
| `input_seq` | per `(host_id, runtime_epoch, terminal_id, controller_id)` | 严格单调；controller_id 改变必重置 |

input 帧**只能**用 `input_seq`；output/delta/replay 帧**只能**用 `output_seq`。两者**绝不**共用。

## Realtime / Snapshot / Replay 大小（SPEC §3.7）

| 类别 | 上限 | 续传 |
|---|---|---|
| Realtime 帧（output/input/delta/title/cwd/error/ping/pong） | payload_len ≤ 64 KiB | **禁止** continuation |
| `snapshot` chunk | payload_len ≤ 256 KiB | continuation bit 链 |
| `replay_data` chunk | payload_len ≤ 256 KiB | continuation bit 链 |
| 单次 `replay` 累计 | 受 `capability_advertise.replay_cap_bytes` 限制 | 服务端按 oldest→newest 顺序回放 |

Realtime 帧必为独立完整 message；PTY 大输出由服务端拆成多个 `output` 帧，
每帧 ≤ 64 KiB、拥有连续 `output_seq`。

## 错误码（SPEC §3.10）

| code | 触发 |
|---|---|
| `unknown_envelope` | magic / efv 不识别 |
| `unknown_message` | type 字段不识别 |
| `unknown_terminal` | terminal_id 不存在或已 detach |
| `runtime_epoch_stale` | attach 时 runtime_epoch 不匹配 |
| `controller_id_unknown` | input 帧带未 attach 的 controller_id |
| `attach_denied` | 权限或并发限制拒绝 |
| `input_too_large` | 单帧 input > `max_realtime_frame` |
| `client_too_old` | `client_max_version < server_version` |
| `server_overloaded` | 服务端背压拒收 |
| `protocol_violation` | 顺序 / 必填字段缺失 / 校验失败 |
| `io_error` | PTY 子进程级错误 |
| `replay_unavailable` | replay 数据已被 GC |

`error` 帧**不**作跨协议兼容带。

## 兼容性策略（SPEC §3.9 P5）

* 新 client → 旧 server：客户端 connect 后只发 RTP1 `attach`；收不到
  `attach_ack` 且无明确 RTP1 错误 → 进入 "无 RTP1 模式"。
* 旧 client → 新 server：客户端不识别 magic → adapter 路径失败 → 回退
  到旧协议直连（如 bounded-seq-v1 HTTP）。
* Minor 扩展：envelope `efv` 不变；新字段通过 postcard 默认行为（未知字段忽略）。
* Breaking 变更：`efv` +1；旧客户端 transport boundary 拒认 → adapter 回退。

## 实现映射

| 类型 | RTP1 struct | 实现位置 |
|---|---|---|
| `attach` | `rtp1::AttachRequest` | `rtp1.rs:AttachRequest` |
| `attach_ack` | `rtp1::AttachAck` | `rtp1.rs:AttachAck` |
| `output` | `rtp1::OutputFrame` | `rtp1.rs:OutputFrame` |
| `input` | `rtp1::InputFrame` | `rtp1.rs:InputFrame` |
| `error` | `rtp1::ErrorFrame` | `rtp1.rs:ErrorFrame` |
| `session_event` | `rtp1::SessionEvent` | `rtp1.rs:SessionEvent` |
| ... | ... | ... |

完整见 `rtp1.rs`。

## 测试

* 单元（`packages/ridge-kernel/src/rtp1.rs`）：9 个测试覆盖 envelope + 22 类型 + cap。
* Conformance（`packages/ridge-kernel/tests/conformance_rtp1.rs`）：12 acceptance + 7 remote lifecycle。
* Live WS（`packages/ridge-cli/tests/rtp1_kernel_e2e.rs`）：开真实 kernel 子进程 → WS attach → 全 message flow 验证。

## 详尽规范

完整 wire 规范见 `specs/L2-PROTO-001.md`。
