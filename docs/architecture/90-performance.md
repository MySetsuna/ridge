# 90 — Performance & Fault Injection

> 模块：`packages/ridge-kernel/src/`
> 测试：`packages/ridge-kernel/tests/performance_baseline.rs`（`--ignored`）
> 详尽规范：`specs/L2-PERF-001.md`

Performance 测量规则（**禁止未测先优化**）：
> 任何优化 PR 必须先附 baseline 对比报告；本目录只声明测量方法、
> 不规定优化阈值（阈值由 baseline 实测后定）。

## 测量规范（SPEC §3.1）

| 指标 | 起点 | 终点 | 单位 |
|---|---|---|---|
| `input_ui_to_pty` | key/input event 被前端捕获 | Rust/kernel `write_to_pty` 接受 | ms (P50/P95/P99) |
| `pty_roundtrip` | PTY write 已被 kernel 接受 | 对应 PTY output 字节被 producer 观察到 | ms |
| `output_to_render_submit` | output/delta 帧抵达 client transport | GPU command 提交完成 | ms |
| `input_ui_to_render_submit` | input event 被前端捕获 | 对应渲染响应的 GPU command 提交完成 | ms (P50/P95/P99) |
| `lan_input_to_render_submit` | 客户端 input 帧出栈 | 远端 GPU command 提交完成（端到端） | ms |
| `wan_input_to_render_submit` | 同上，模拟 RTT | 同上 | ms |
| `pty_output_throughput` | 子进程 stdout 字节 | bytes 抵达 consumer cursor | bytes/s |
| `render_submit_cost` | JS rAF / dirty notification 触发 | GPU command 提交完成 | ms (P50/P95) |
| `physical_presentation_latency` | GPU command 提交 | 像素真正落到屏幕 | UNMEASURED |
| `cpu_idle_usage` | 静态 shell（无输出） | % CPU | % |
| `cpu_high_output_usage` | `yes` / `cat` 跑满 | % CPU | % |
| `memory_per_terminal` | 一 pane idle 1 分钟 | RSS 增量 | MiB |
| `scrollback_cost` | scrollback 行数增加 | bytes | bytes/line |
| `reconnect_time` | 模拟网络断 | attach_ack 抵达（runtime_epoch 未变） | ms |
| `replay_time` | 触发 replay | snapshot/replay_data 完 | ms |
| `snapshot_resync_time` | 触发 resync | 客户端 grid 等价（runtime_epoch 未变） | ms |
| `stale_epoch_detection_latency` | host kernel kill -9 | 客户端收到 `error{code:"runtime_epoch_stale"}` | ms |
| `rediscovery_latency` | 客户端收到 stale_epoch | 客户端调 `host_list_sessions` + 新 attach_ack 抵达 | ms |

> "字符出现在屏幕上"是合成本质，不作为单条指标；改用可观测的"GPU command 提交完成"。
> `physical_presentation_latency` 在 v1 标 UNMEASURED，禁止以 JS `performance.now()` 近似。

## 场景矩阵（SPEC §3.2）

每个场景至少跑 3 次，报告 P50/P95/P99 + std：

| 场景 | 命令 |
|---|---|
| bash / powershell | 交互提示符 + `ls` |
| 大量 cat | `cat /var/log/syslog` 50 MiB |
| `yes` | `yes foo` |
| git diff | 大仓库下 `git diff` |
| ripgrep | `rg foo large_src/` |
| npm/cargo build | 大 crate `cargo build` |
| Claude/Codex streaming | 跑 Claude CLI 跑 30 s |
| vim | 打开 1 MiB 文件 |
| fzf | 跑 `fzf` 在 1k 项上 |
| htop / lazygit | `htop` 或等价 |
| 中文 | `echo "中文测试一下"` |
| emoji | `echo "👨‍👩‍👧 🇨🇳 🎉"` |
| resize storm | 脚本反复 `printf '\x1b[8;%d;%dt' $r $c` |

Foundation 已覆盖：bash / Unicode / emoji / resize storm / CJK / alternate
screen（见 `terminal_live.rs`）。其它场景在 live e2e 范围内（gated by
release build + headed runner）。

## 故障注入（SPEC §3.3）

| 故障 | 工具 | 触发 |
|---|---|---|
| latency | Linux: `tc qdisc add dev lo root netem delay 100ms 20ms` | inject |
| packet loss | `tc netem loss 5%` | inject |
| connection interruption | `iptables -A INPUT -p tcp --dport X -j DROP` | inject |
| client sleep/wake | macOS/Windows 休眠 | manual |
| Wi-Fi switch | OS hotspot toggle | manual |
| client crash | `kill -9` 客户端进程 | inject |
| host crash | `kill -9` kernel 进程 | inject |

每种故障跑对应验收场景：
* latency → `lan_input_to_render_submit`
* loss → `reconnect_time` + grid 一致性
* interruption 30 s → reconnect + resync（runtime_epoch 未变）
* client crash → 重启后 attach 流程
* host crash（kernel kill -9） → 新 `runtime_epoch` → stale attach
  rejection → client rediscovery；测量 `stale_epoch_detection_latency` +
  `rediscovery_latency`；**不**承诺旧 terminal snapshot resync。

## 收集方法

* **Rust 端**：tracing-subscriber；事件 `{latency_ms, scenario, pane_id}`。
* **JS 端**：PerformanceObserver + `performance.now()`；上报到 `/perf/sample`
  （Tauri command，本地落盘）。
* **聚合**：`scripts/perf-runs/aggregate.ts`（新增）输出 markdown 报告到
  `artifacts/perf/`。
* **CI**：可选 `--scenario baseline` smoke；不卡 build。

## Baseline 要求（SPEC §3.5）

* 本 spec 通过后第一周必须产出一份 baseline 报告
  `artifacts/perf/baseline-YYYY-MM-DD.md`（Foundation 范围内捕获见下）。
* 报告至少包含：硬件（CPU/RAM/OS）、窗口大小、每场景 P50/P95、当前已知异常。
* 任何后续优化 PR 必须附 "before vs after" 对比。

## Foundation 范围捕获

测试运行命令：

```bash
cargo test -p ridge-kernel --test performance_baseline -- --ignored --nocapture
```

实测（dev profile / 单线程 tokio runtime / 本机）：

| 场景 | 结果 |
|---|---|
| `pty_output_throughput` (16 MiB drain) | 3,751,936 bytes in 3.0 ms (93 polls) = **1.19 GiB/s** sustained |
| `input_to_output_single_pane` (n=256) | p50=6 µs · p95=11 µs · p99=22 µs |
| `multi_pane_publish` (64 MiB / 16 threads) | 44 ms ≈ 1.4 GiB/s |
| `rtp1_attach_latency` (n=1000) | p50=2 µs · p95=3 µs · p99=3 µs |
| `rtp1_fan_out_sizes` (170 input → 170 RTP1 frames) | 290 ms · max payload 32,851 B (cap 65,536) |

## Baseline 报告

Foundation baseline 见：

```text
artifacts/perf/baseline-2026-09-12.md  (待写；当前 raw 数据从 perf_baseline 测试 stderr 捕获)
```

后续优化必须：
1. 先 capture `before` 数字（重跑 baseline）。
2. 提交 PR；CI 跑 perf_baseline `--ignored` 自动 capture `after`。
3. P95 差异 > 10% → 需要回归解释；不达标 → revert。

## 详尽规范

* `specs/L2-PERF-001.md` 完整指标 + 场景 + 故障注入列表
* 测试代码：`packages/ridge-kernel/tests/performance_baseline.rs`
