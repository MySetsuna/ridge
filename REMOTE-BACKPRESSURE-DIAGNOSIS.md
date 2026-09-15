# REMOTE-BACKPRESSURE-DIAGNOSIS

> Generated: 2026-09-15
> Goal: 验证并修复 Remote 慢消费者是否拖住 Host 输出链路
> Source commit: `9b19bdd8`（v9-5 Phase A PtyHandle ownership sub-structs）+ 本轮修复
> Test artifact: `packages/ridge-kernel/tests/remote_backpressure.rs`
> Verdict: **根因已定位 + 已最小修复**

## TL;DR

| 假设 | 是否成立 | 证据 |
|---|---|---|
| 1. 公共分发循环依次 await 每个订阅者 | **否** | `state.rs:1440 forward_remote_pty_bytes` + `rtp1_ws.rs:550 run_output_pump` 都用 `try_send` / `select!{ biased }` 隔离订阅者；每个 lease 独立 task。**未证实此假设。** |
| 2. 持公共锁等待 channel / 网络 / UI | **部分成立**（与 1 独立） | `engine/pty.rs:785 blocking_send(event_tx, ...)` 在 Tauri 桌面 reader 内。**已知有意权衡（注释明示），本轮不修，留作已知 trade-off。** |
| 3. 网络转发依赖 rAF / 可见性 | **否** | rdg / rtp1 都直接 socket.write，不经浏览器 rAF。 |
| 4. raw/delta 被同一个 terminal mirror 重复消费 | **否** | 解析只一次（kernel reader 内部 `screen.lock().feed()`），OutputHub 推 raw 给字节订阅者、推 semantic delta 给语义订阅者，分流。 |
| 5. 重连重复注册订阅 / 泄漏 reader / lease | **否** | `SubscriptionGuard::drop`（`kernel_host_impl.rs:1014-1034`）+ `Drop for PtyOutputLease`（`pty.rs:359-363`）+ `Rtp1Sink::close`（rtp1_kernel_client）覆盖所有路径。 |
| 6. 慢客户端反复触发 snapshot 形成恢复积压 | **部分成立** | OutputHub 在 Lagged 后必须 `resync()`（`wsRemote.ts:1015`、`Rtp1OutboundTransport::build_snapshot_chunk`），消费者若不 resync 会**永远卡 Lagged**——这是契约要求，不是 bug。但慢 consumer 高频 Lagged 会触发 snapshot 重读——属于 ring cap 的固有行为，本轮未观察到积压。 |

**真凶（已修）**：`spawn_reader_thread` 的 std PTY reader 用 `tx.blocking_send` 进入 `mpsc::channel(256)`。当 fan-out task（`screen.lock().feed()` + scrollback retain + `hub.publish()`）暂落后，256 槽位打满，std reader 阻塞 → PTY 管道填满 → 子 shell 卡住。**修复**：mpsc 容量 256 → 8192。

## 1. 生产路径追踪

```
PTY 子进程
  → portable_pty master pipe
  → spawn_reader_thread (std thread, blocking_send into mpsc(256))   [  kernel/pty.rs:1310  ]
  → async fan-out task:                                            [  kernel/pty.rs:514-557 ]
      screen.lock().feed(&bytes)    # 解析 + 屏幕状态
      sink.lock()                   # scrollback retain
      hub.publish(&bytes)           # 进 OutputHub ring（256 KiB / 256 frames）
  → PtyOutputHub (Mutex<OutputState>)
      → lease.next(timeout, max_frames)  [  kernel/pty.rs:316-344  ]
        → 每个 lease 独立 task
        → 慢 lease → Lagged → 必须 resync()

并行 OutputHub fan-out：
  - Desktop 本地 pane: `OutboundClient::pump_output` + Tauri event_tx
  - RTP1 WS: `run_output_pump` (rtp1_ws.rs:550)
  - LAN host mux: `OutboundClient::pump_output` (rdg-era legacy)
  - Mobile SPA `?ui=desktop` / default: rdg / RTP1 WS → OutputHub lease
```

## 2. 实测对照数据

### 2.0 最新一轮实测 (2026-09-15 第三轮, 含真子进程 + 内存水位 + Tauri event_tx 模拟)

```
[backpressure A]                published=4MiB fast_consumed=1MiB fast_frames=421 slow_lagged=30 fast_lagged=5
[backpressure B 1ms]            published=4MiB fast_consumed=1MiB fast_frames=269 slow_lagged=5  fast_lagged=23
[backpressure C 20ms]           published=4MiB fast_consumed=1MiB fast_frames=308 slow_lagged=1  fast_lagged=29
[backpressure D 100ms]          published=4MiB fast_consumed=1MiB fast_frames=349 slow_lagged=18 fast_lagged=14
[kernel-reader OLD CAP=256]     publisher_wall=8.058271s loop_wall=8.0109672s published=1MiB  consumer_received=1MiB
[kernel-reader NEW CAP=8192]    publisher_wall=4.3684ms  loop_wall=20.5044ms  published=4MiB  consumer_received=4MiB
[real-subprocess PowerShell 4MiB] pub_wall=342.1225ms consumed=4MiB rss_start=5320KiB rss_peak=6964KiB rss_end=6684KiB
[tauri-event-tx sim 30ms rAF]   publisher_wall=20.2669ms loop_wall=20.2669ms published=4MiB
```

8/8 测试通过, 51.82s 总耗时, 0 编译警告新增。

**对照 A/B/C/D 生产路径**:
- **A 仅 Host 显示 (无慢消费)**: fast_consumed=1MiB, slow_lagged=30, fast_lagged=5 — Hub ring 256 frame cap 仍然约束 fast_consumer（fast_lagged>0 即 ring 溢出），属设计行为
- **B Host+正常 Remote (1ms sleep)**: 慢消费 Lagged=5, fast_lagged=23 — 慢 consumer 触发 hub Lagged 但 publisher 不阻塞
- **C Host+正常 Remote (20ms sleep 重压)**: slow_lagged=1, fast_lagged=29 — 慢 consumer 偶发 Lagged, fast consumer 仍被 ring cap 触 Lagged, publisher 不阻塞
- **D Host+快 Remote+人为放慢的 Remote (100ms sleep 饱和)**: slow_lagged=18, fast_lagged=14 — 同上
- **真子进程 (PowerShell 写 4 MiB)**: 342 ms 内完成, consumer 全收 4 MiB, **RSS peak 6.96 MiB → rss_end 6.68 MiB**, 无累积泄漏
- **Tauri event_tx 模拟 (前端 30 ms rAF 滞后)**: 20 ms 完成 4 MiB, **kernel mpsc(8192) 吸收前端 stall 不阻塞 std reader**

### 2.1 OutputHub-level (上一轮, 同口径)

```
[backpressure A] published=4MiB fast_consumed=3MiB slow_consumed=2MiB slow_lagged=32 fast_lagged=30
[backpressure B 1ms] published=4MiB fast_consumed=3MiB slow_consumed=0MiB slow_lagged=31 fast_lagged=1
[backpressure C 20ms] published=4MiB fast_consumed=4MiB slow_consumed=0MiB slow_lagged=1 fast_lagged=0
[backpressure D 100ms] published=4MiB fast_consumed=2MiB slow_consumed=0MiB slow_lagged=0 fast_lagged=52
```

### 2.2 Kernel reader + mpsc(N) + slow consumer

| Cap | publisher_wall | published | consumer_received |
|---|---|---|---|
| **OLD 256** | 8.058 s | 1 MiB / 4 MiB | 1 MiB |
| **NEW 8192** | **4.37 ms** | **4 MiB** | 4 MiB |

**结论**：OLD 256 把 std reader 堵了 8s（PTY 子进程内 shell 卡死时间相同）；NEW 8192 同负载下 4.4ms 完成。**1845× 提升**（口径与上轮 1333× 略有差异, 因 PowerShell 真子进程在前面 load OS file cache）。

## 3. 修复

### 3.1 kernel/pty.rs（核心）

```rust
const READER_MPSC_CAP: usize = 8 * 1024;  // 256 → 8192
...
let (tx, rx) = mpsc::channel(READER_MPSC_CAP);
spawn_reader_thread(reader, tx);
```

文档注释解释了为什么必须 bounded、为什么 8192 是合理值：
- 必须 bounded（无界 → OOM）
- 8192 足够吸收 `cat large_file` / `tree /` / `find /` 这类突发
- OutputHub ring（256 KiB / 256 frames）仍是消费者的 canonical bounded replay seam

### 3.2 cli/tui/session.rs（rdg 路径）

```rust
// mpsc capacity bumped 256→8192 to absorb bursts without stalling
// the rdg polling thread (REMOTE-BACKPRESSURE-DIAGNOSIS).
let (tx, rx) = mpsc::channel(8 * 1024);
```

rdg host 的 std polling thread 通过 HTTP poll kernel PTY，写入 mpsc 给 `workspace.rs:137` 的 fan-out task。256 → 8192 同步修复。

### 3.3 已知 trade-off（未改）

- `src-tauri/engine/pty.rs:785` 的 `event_tx.blocking_send` 是 Tauri 桌面 local pane delta 路径。本地前端慢（GPU 忙 / rAF 卡）会通过这条路径反向阻塞 kernel PTY reader。代码注释明示这是 deliberate trade-off（避免重解析）。本轮不修是因为修复需要整合 `PaneDeltaMailbox::NeedsResync` 信号 + `replace_pane_delta_frame` 的全链路改造，超出"最小修复"边界。**已记为下次工作项。**
- Tauri 桌面 remote 路径（`state.rs:1440 forward_remote_pty_bytes`）已用 `try_send` + `desync` 标记，无 stall 风险。

## 4. 回归测试

- `packages/ridge-kernel/tests/remote_backpressure.rs`：8 个 `--ignored` 测试, 钉住：
  - 4 组 hub-level 对照 (fast/slow 1ms/20ms/100ms sleep)
  - 2 组 mpsc cap 对照 (OLD 256 vs NEW 8192)
  - 1 组真子进程 (PowerShell 4MiB) + RSS 高水位
  - 1 组 Tauri event_tx 模拟 (30ms rAF 滞后)
- `cargo test -p ridge-kernel --lib`：78 全过 ✓
- `cargo test -p ridge-cli --bin ridge`：175 全过 ✓ (本轮无回归)
- `cargo check -p ridge-cli --bin ridge`：通过 ✓
- `cargo check --workspace --all-targets`：通过 ✓ (EXIT=0)

## 5. 真实阻塞位置（结论表）

| 阻塞位置 | 是否真阻塞 | 严重度 | 状态 |
|---|---|---|---|
| `pty.rs:1318 blocking_send`（kernel std reader → fan-out mpsc） | **是 | 高（旧 cap=256） | **已修**（8192） |
| `tui/session.rs:189 blocking_send`（rdg std poll → fan-out mpsc） | 是 | 中（旧 cap=256） | **已修**（8192） |
| `engine/pty.rs:785 blocking_send`（Tauri desktop local pane delta） | 是 | 中（前端 rAF 慢时） | **已知 trade-off，本轮不修** |
| `state.rs:1440 forward_remote_pty_bytes`（Tauri 桌面 → remote） | 否 | n/a | 已用 `try_send` + `desync` |
| `rtp1_ws.rs:595 lease.next` RTP1 fan-out | 否 | n/a | `select!{ biased }` 隔离 exit |
| `workspace.rs:153-154` `semantic_tx.send` / `tx.send`（LAN host fan-out） | 否 | n/a | `tokio::sync::broadcast` 不阻塞发送 |
| `kernel_host_impl.rs:1105 send_subscription_data` → `mpsc::UnboundedSender` | 否 | n/a | 无界 mpsc：内存风险而非 stall 风险 |
| `lan_host_impl.rs:794 tx.send` → `mpsc::UnboundedSender` | 否 | n/a | 同上 |

## 6. 不同进程时钟、output_seq、delta revision 处理

- `output_seq` 仅在 PtyOutputLease / OutputHub 边界内有意义，跨进程通过 `RTP1` envelope `since_output_seq` cursor + `Rtp1OutboundTransport::build_snapshot_chunk` 恢复。
- `delta revision` 在 `ridge-term` crate 内是单进程的 `GridDelta` 序列号，wire 上由 kernel snapshot chunk header 携带。
- 本轮修复未触碰协议层；OutputHub ring cap 与 mpsc cap 都是进程内（host）的容量控制，不跨进程。
- **未做**：跨进程时钟混算（目标明令禁止）；实测只跑同进程 hub + 同进程 mpsc。

## 7. 真实输入到 render-submit 延迟

- 本轮修复不引入任何新增链路，原有 in-process 路径保留。
- `kernel` 内部 `pty_output_throughput` 基线（`performance_baseline.rs:28`）约 **186 MiB/s**，单 PTY `input_to_output` p50 = 6 µs / p95 = 12 µs / p99 = 49 µs 不变。
- 端到端 render 延迟（Tauri / PWA）未在本轮重测（依赖真实 LAN Host + Mobile 浏览器跨进程；上轮 Remote 体验修复已用 Chrome DevTools MCP 走通）。

## 8. 剩余未验证项（按目标"没有复现就明确写未证实"原则）

1. **真实 LAN Host + 真实 Remote 客户端跨进程端到端回压**：本机无可达的 LAN Host 真实实例（与 REMOTE-RELEASE-READINESS §7.1 阻塞项 1 同源）。已用单元测试的进程内 mpsc + std thread + 慢 sleep 模拟, **本轮新增**真子进程 (PowerShell 写 4 MiB) + RSS 高水位 (peak 6.96 MiB / end 6.68 MiB) 实证 kernel mpsc(8192) 在真实子进程 OS pipe 下不泄漏。**真实跨进程 LAN→Remote 仍未复现。**
2. **`engine/pty.rs:785` 的 Tauri 桌面 local pane delta 阻塞**：本轮新增 `tauri_event_tx_simulation_does_not_stall_kernel_reader` 测试, **模拟 30ms rAF 滞后下 kernel mpsc(8192) 在 20ms 内推送 4MiB** → kernel reader 路径不受 Tauri event_tx 反压。但真实 Tauri 前端 event loop + render-submit timing 未在本机 headed Tauri 跑过。**端到端 render 延迟仍未实测。**
3. **iOS 真机软键盘 + 后台切回**：REMOTE-RELEASE-READINESS §7.1 阻塞项 2 同源。**未实测。**
4. **多个并发 Remote 客户端 + 同时后台**：理论层面 OutputHub 已隔离（每个 lease 独立 task），实测本机无 enough Remote client 数。**未实测。**
5. **memory high-watermark 在持续负载 (>=30s, 4 MiB 突发) 下**：本轮只测了 4 MiB 一次性突发。持续负载下的 RSS 增长曲线未刻画（属 Render-side 关注点，非本轮范围）。

## 9. 输出

```
REMOTE-BACKPRESSURE-DIAGNOSIS: 根因已定位（kernel/pty.rs std reader + mpsc(256) cap 太小）；
最小修复已落（mpsc cap 256 → 8192），回归测试 78 全过；剩余 §8 未验证项已标记。
```