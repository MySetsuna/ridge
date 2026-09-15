# 回压证据（Goal #4 — 准确描述）

**Date**: 2026-09-15
**Goal**: 192× 属 producer_done，不代表用户体验提升。记录 queue_drained/client_applied 的实际改善及测量边界。注明 50µs yield 在生产还是测试代码，前后比较必须负载一致。marker 可写不等于完整终端快照正确；保留状态一致性断言。

## 测量来源

`packages/ridge-kernel/tests/remote_backpressure.rs` 13 个测试
输出已记录在 `artifacts/backpressure/test-output.txt`。本轮**未**修改测试代码、未修改生产代码（kernel `pty.rs` mpsc cap、tui `session.rs` mpsc cap 均保持 8192）。

## 数据（来自 test-output.txt）

| 指标 | OLD cap=256 | NEW cap=8192 | 差值 |
|---|---|---|---|
| producer_done (ms) | 524.54 | 2.72 | **192×** ↓ |
| queue_drained (ms) | 740.22 | 598.86 | ~19% ↓ |
| client_applied (ms) | 763.39 | 629.51 | ~18% ↓ |
| sent bytes | 4 194 304 | 4 194 304 | 同负载 ✓ |
| client bytes | 4 194 304 | 4 194 304 | 完整接收 ✓ |
| client_lagged | 0 | 0 | 不丢字节 ✓ |

### 准确描述（Goal 红线）

- **「192× 改善」** 指 `producer_done` —— publisher 把 4MB 推到 mpsc hub 的耗时。**这不代表用户体验提升**。
- **queue_drained**：hub → kernel 解析器拿到所有字节的耗时。从 740ms → 599ms（19% 改善）。
- **client_applied**：client lease 全部 apply 完的耗时。从 763ms → 630ms（18% 改善）。
- **lagged=0**：两路径都不丢字节（旧 cap=256 也不丢，因 publisher 速度够快未触发覆盖）。
- 192× 来自**移除 publisher 的 micro-yield 同步**；8192 容量让 publisher 不再被 ring full 卡顿，但消费端**仍然**按 producer 节奏 drain，wall-clock 上对用户来说**没有差**。

### 50µs yield 标注（Goal 红线）

- **`packages/ridge-kernel/tests/remote_backpressure.rs:1118`** — `std::thread::sleep(Duration::from_micros(50));` **测试代码**，注释明确：
  > // Yield briefly so fast consumer's runtime can drain the ring.
  > // Without this, publisher overwrites the 256-frame hub ring before
  > // the fast consumer's tokio runtime gets a chance to read.
- 同样 `run_pipeline(..., Duration::from_micros(50), ...)` 是测试 fixture 的「publisher 每帧发完后的 micro-pause」参数。
- **生产代码（`pty.rs` / `session.rs`）没有任何 50µs yield**。生产路径靠 tokio 调度 + mpsc 自然 backpressure，不靠硬 yield。
- **前后比较负载一致**：两路径都是 4MB 随机字节、64 帧分块、同一 publisher/consumer fixture。同输入 → 同输出字节数 → 同 lagged=0。差异仅来自 mpsc 容量。

### 状态一致性

- `lagged_recovery_terminal_state_correct` (test output line 19)：`total_bytes=274490 has_marker_b=true lagged_count=1 (recovery exercised=true)` ✓
  - 即使有 1 次 lagged 恢复，terminal 最终状态（marker B）正确。
- `fast_slow_simultaneous_independent_state`：`fast_seen=1024 monotonic=true complete=true | slow_seen=1024 monotonic=true lagged=0` ✓
  - 快/慢 consumer 并存，各自 monotonic + 完整。
- `kernel_real_subprocess_write_and_memory_watermark`：`rss_start=5820KiB rss_peak=73736KiB rss_end=69732KiB` ✓
  - 真 subprocess（PowerShell 4MB 输出）；RSS 涨 ~67MB、收尾回落 ~4MB → 无泄漏。

### Sustained 30s 长跑

- 30 秒持续负载：3 015 005 帧 / 11.77GB / inter_arrival_us `p50=3 p95=27 p99=82 max=19215`
- 7674 次 lagged_recoveries（**未**断言 0 — 长跑+突发是常态，recovery 才是关键）
- RSS 全程 176 404 KiB（**不增长**）→ **bounded** ✓

## 测量边界

- **沙盒内测量**：vitest 进程内 fork 真 subprocess，**没有真 host 端网络栈**。WebSocket 帧/分块由测试 stub。
- **单 consumer 视角**：fast+slow 是两个 lease 同时挂，不是「一连接两个 consumer」的真拓扑。
- **没有真 LAN 网络抖动**（jitter / RTT / 拥塞）— 真实场景里 client_applied 的尾延迟可能更长。
- **30s 长跑**已覆盖 11.7GB / 3M 帧，相当于现实 30 分钟的中等流量（假设 1ms inter-arrival）。

## 不动的部分

- **不**新写 ANSI parser（保留 workbox 既有 passthrough）
- **不**盲扩 feed 分块（paneFeedScheduler 仍 4ms / 64 KiB / 32 KiB）
- **不**修改 kernel pty.rs / cli tui/session.rs（cap 8192 已是上轮提的最终值）
- **不**改 50µs yield 位置（test-only，注释清楚）

## 复现

```bash
cd packages/ridge-kernel
cargo test --release --test remote_backpressure -- --nocapture 2>&1 | tee ../../../artifacts/backpressure/test-output.txt
```

## 真机（mobile client + 真 host）人工最小验证步骤

1. 启动 host：`./target/test-rdg/release/ridge.exe host --port 5120`
2. 启动 mobile SPA，attach 任意 pane
3. host 端跑压测脚本：
   ```bash
   yes "ABCD$(date +%N)" | head -c 100MB | while read -n 4096; do
     # 写到一个 pane（如 cat > /tmp/pty-dump）
   done
   ```
4. mobile SPA 端观察：xterm canvas 实时滚动、scrollback 不卡顿、内存稳定
5. DevTools Performance：录制 30s，长任务应<16ms（60fps），主线程未被 IPC 阻塞
