# 00 — 系统总览

## 目标

Ridge 是一个把"终端模拟"提升为一等公民的分布式系统：
PTY 子进程生命周期、字节序、replay authority 全部由 **ridge-kernel**
独占，桌面端 / headless host / rdg 全部是其消费者；远端通过统一的
**RTP1** wire 协议与 kernel 通信。

## 拓扑

```
                ridge-kernel              (Runtime Authority)
              ┌─────┴─────┐
              │           │
           Desktop      headless host / rdg
           (Tauri)      (ridge-cli)
              │           │
              └─────┬─────┘
                    │
                  RTP1                 (canonical wire over WebSocket)
                    │
                  bound-seq-v1 HTTP    (legacy adapter / transport-boundary only)
                    │
                  Remote (browser / mobile / CLI controller)
```

## 组件清单

| 组件 | crate | 角色 | 关键不变量 |
|---|---|---|---|
| **ridge-kernel** | `packages/ridge-kernel/` | PTY 进程权威 + output_seq + replay | PTY_SINGLE_OWNER |
| **ridge-term** | `packages/ridge-term/` | 终端语义（VTE 解析、grid、scrollback） | 桌面 / kernel 共享 native vte core |
| **ridge-core** | `packages/ridge-core/` | 共享 workspace / teammate / remote schema | 无 Tauri 依赖 |
| **ridge-mcp** | `packages/ridge-mcp/` | MCP protocol 路由 | 与 kernel 进程同寿命 |
| **ridge-remote** | `packages/ridge-remote/` | LAN remote 控制面 + 嵌入 UI | 与 ridge-cli 同进程 |
| **ridge-tmux** | `packages/ridge-tmux/` | headless tmux 引擎 | 同桌面端逐字节同源 |
| **ridge-cli** | `packages/ridge-cli/` | headless host（CLI 二进制 `ridge`） | 单一二进制 / 系统服务模板 |
| **ridge-mcp-bridge** | `packages/ridge-mcp-bridge/` | desktop ↔ kernel MCP 桥 | 仅桌面端使用 |
| **rg-split** | `packages/rg-split/` | 桌面 splitter（GPU 加速） | 仅桌面渲染使用 |

## 模块边界 / 不变量

### 1. PTY 进程与字节序（SPEC-L2-TERM-001 §3.1）

* PTY 子进程生命周期：**唯一**由 `PtyRegistry`（kernel 内）拥有。
* output_seq（PTY 字节序）：**唯一**由 `PtyOutputHub` 产生；lease cursor 定位 bytes。
* 任何 client（Desktop / headless / rdg / 远端）通过：
  * **RTP1 WS** `/v1/rtp1`（canonical）
  * **bounded-seq-v1 HTTP** `/v1/domain/ptys/:id/output/:lease`（legacy adapter）
  消费这些字节序；**绝不**重新 spawn PTY 或另起 output_seq。

### 2. RTP1 wire 协议（SPEC-L2-PROTO-001 §3）

* 5 字节固定 header：`magic[4] | efv[1] | type[1] | flags[1] | payload_len[4]`
* `runtime_epoch`：kernel 每次启动 mint 一次 UUID v7；attach 必校验，stale 必拒。
* input_seq 与 output_seq 命名空间分离。
* Realtime 帧 ≤ 64 KiB；snapshot / replay 块 ≤ 256 KiB 必须 continuation 链。

### 3. 远程 lifecycle（SPEC-L2-REMOTE-001 §3）

* Terminal lifecycle：`Starting → Running → Exited → Reaped`（单调）。
* Connection state machine：`Detached → Connecting → Attached → Reconnecting → Desynced → Closing/Failed`。
* Network 中断 ≤ 30s：自动以 `since_output_seq` resume；超 replay window → `desync` + `replay` / `snapshot`。
* Host 重启 → 新 `runtime_epoch` → stale attach 必拒 → client 走 rediscovery。

### 4. CLI 统一

* `rdg` 二进制已下线（commit ea0a3afa + 后续）；唯一二进制 `ridge`。
* `RIDGE_RTP1_KERNEL=1` env flag 切换 KernelHost 订阅循环到 RTP1 WS；
  HTTP 路径保留为 legacy adapter fallback（默认 off 新路径）。

## 数据通路追踪（高层）

```text
PTY 子进程 stdout
  → kernel: PtyOutputHub.publish → notify        (in-process)
  → kernel: PtyOutputHub 保留最近 256 KiB / 256 帧
  → 出口 A：HTTP long-poll                       (legacy bounded-seq-v1)
       shell.KernelPtyReader → Tauri command → renderer → WebGPU
  → 出口 B：RTP1 WS                              (canonical)
       shell.rtp1_kernel_client → mpsc → engine → renderer → WebGPU
```

详细追踪见 `60-data-flow.md`。

## 关键事实（速查）

| 事实 | 值 |
|---|---|
| PTY 字节序 cap | 256 KiB / 256 帧（`OUTPUT_REPLAY_CAP_BYTES` / `OUTPUT_REPLAY_CAP_FRAMES`） |
| realtime frame cap | 64 KiB（`MAX_REALTIME_FRAME`） |
| snapshot chunk cap | 256 KiB |
| snapshot/replay 唯一边界信号 | `flags.continuation`（bit0） |
| runtime_epoch 生成 | `Uuid::now_v7()`（kernel boot 时一次） |
| host_id 格式 | `<COMPUTERNAME>@<runtime_epoch>` 或 `RIDGE_HOST_ID` env |
| 命令行二进制 | `ridge`（在 `packages/ridge-cli/target/{debug,release}/`） |
| 系统服务名 | `ridge-cli` / `ridge-tmux` |

## 不在 Foundation 范围内

* Live Tauri/WebGPU e2e（需要 release build + headed Windows runner）
* `mux.rs::channel::PANE_RAW` 的 RTP1 适配实接（surface 已就位）
* 桌面端 `engine::kernel_pty` 完整迁移到 RTP1（HTTP 路径仍可用）

详细见 `RIDGE-RUNTIME-FOUNDATION-FINAL.md` §9。
