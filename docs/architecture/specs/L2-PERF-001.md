---
id: L2-PERF-001
level: L2
title: Terminal / Remote Performance
status: APPROVED
origin: authored
migration_state: PROPOSED
depends_on: [L1-PROJECT-001, L2-TERM-001, L2-PROTO-001, L2-REMOTE-001]
---

## §1 范围

定义终端 + 远控的性能测量规范与故障注入规范。
**禁止未测先优化**：任何优化 PR 必须先附 baseline 对比报告；本 spec 只声明测量方法、不规定优化阈值（阈值由 baseline 实测后定）。

## §2 现状事实

### §2.1 既有 perf 设施

| 模块 | 位置 | 用途 |
|---|---|---|
| `scripts/perf-runs/` | 仓库根（被 `.stcignore` 排除） | perf 输出落盘 |
| `wdio.perf.conf.ts` | `tests/` | WebdriverIO 性能 e2e |
| `pnpm e2e:perf` | `package.json` | frame attribution + stress |
| `delta_mode` | `engine/parser.rs` | Rust-side 解析；避免回 JS 端 |
| `OUTPUT_ACTIVITY_INTERVAL = 250ms` | `lib.rs` | pane activity 节流 |
| `PaneDeltaMailbox` | state.rs | bounded mailbox 控 UI 端 flood |

### §2.2 缺失

- 没有统一的 perf 测试套件（`scripts/perf-runs/` 是落盘目录非定义）。
- 远控路径无 end-to-end latency 测量。
- 无故障注入测试 harness。

## §3 测量规范

### §3.1 指标定义

| 指标 | 起点 | 终点 | 单位 |
|---|---|---|---|
| `input_ui_to_pty` | key/input event 被前端捕获（JS event 时间戳） | Rust/kernel `write_to_pty` 接受该 input（已写入 PTY master） | ms (P50/P95/P99) |
| `pty_roundtrip` | PTY write 已被 kernel 接受 | 对应 PTY output 字节被 producer 观察到（即 `output_seq` 上出现关联响应） | ms |
| `output_to_render_submit` | output/delta 帧抵达 client transport | GPU command 提交完成（即 render queue 接受） | ms |
| `input_ui_to_render_submit` | input event 被前端捕获 | 对应渲染响应（PTY echo / TUI repaint）的 GPU command 提交完成 | ms (P50/P95/P99) |
| `lan_input_to_render_submit` | 客户端 input 帧出栈 | 远端 GPU command 提交完成（端到端） | ms |
| `wan_input_to_render_submit` | 同上，模拟 RTT | 同上 | ms |
| `pty_output_throughput` | 子进程 stdout 字节 | bytes 抵达 consumer cursor | bytes/s |
| `render_submit_cost` | JS rAF / dirty notification 触发 | GPU command 提交完成 | ms (P50/P95) |
| `physical_presentation_latency` | GPU command 提交 | 像素真正落到屏幕 | UNMEASURED（v1；JS `performance.now()` 无法证明 physical present；需 GPU timestamp + platform instrumentation） |
| `cpu_idle_usage` | 静态 shell（无输出） | % CPU | % |
| `cpu_high_output_usage` | `yes` / `cat` 跑满 | % CPU | % |
| `memory_per_terminal` | 一 pane idle 1 分钟 | RSS 增量 | MiB |
| `scrollback_cost` | scrollback 行数增加 | bytes | bytes/line |
| `reconnect_time` | 模拟网络断 | attach_ack 抵达（runtime_epoch 未变） | ms |
| `replay_time` | 触发 replay | snapshot/replay_data 完 | ms |
| `snapshot_resync_time` | 触发 resync | 客户端 grid 等价（runtime_epoch 未变） | ms |
| `stale_epoch_detection_latency` | host kernel kill -9 | 客户端收到 `error{code:"runtime_epoch_stale"}` | ms |
| `rediscovery_latency` | 客户端收到 stale_epoch | 客户端调 `host_list_sessions` + 新 attach_ack 抵达 | ms |

> "字符出现在屏幕上"是合成本质，不作为单条指标；改用可观测的"GPU command 提交完成"。`physical_presentation_latency` 在 v1 标 UNMEASURED，禁止以 JS `performance.now()` 近似。

### §3.2 场景矩阵

| 场景 | 命令 | 备注 |
|---|---|---|
| bash / powershell | 交互提示符 + `ls` | baseline |
| 大量 cat | `cat /var/log/syslog` 50 MiB | 测吞吐与 scrollback |
| `yes` | `yes foo` | 测持续背压 |
| `git diff` | 大仓库下 `git diff` | 测混合 ANSI |
| ripgrep | `rg foo large_src/` | 测高亮与实时输出 |
| `npm/cargo build` | 大 crate `cargo build` | 测混合进度输出 |
| Claude/Codex streaming | 跑 Claude CLI 跑 30 s | 测增量更新（ANSWER 块） |
| vim | 打开 1 MiB 文件 | 测 full-screen TUI + refresh |
| fzf | 跑 `fzf` 在 1k 项上 | 测全屏 repaint |
| htop / lazygit | `htop` 或等价 | 测持续刷新 |
| 中文 | `echo "中文测试一下"` | 测 UTF-8 + CJK width |
| emoji | `echo "👨‍👩‍👧 🇨🇳 🎉"` | 测 grapheme cluster |
| resize storm | 脚本反复 `printf '\x1b[8;%d;%dt' $r $c` | 测 resize 频率 |

每个场景至少跑 3 次，报告 P50/P95/P99 + std。

### §3.3 故障注入

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
- latency → `lan_input_to_render_submit`
- loss → `reconnect_time` + grid 一致性
- interruption 30 s → reconnect + resync（runtime_epoch 未变）
- client crash → 重启后 attach 流程
- host crash（kernel kill -9） → 新 `runtime_epoch` → stale attach rejection → client rediscovery；测量 `stale_epoch_detection_latency` + `rediscovery_latency`；**不**承诺旧 terminal snapshot resync（SPEC-REMOTE-001 §3.6 #3）。

### §3.4 收集方法

- **Rust 端**：tracing-subscriber + tracing-subscriber 层 `target: "ridge::perf"`；事件 `{latency_ms, scenario, pane_id}`。
- **JS 端**：PerformanceObserver + `performance.now()`；上报到 `/perf/sample`（Tauri command，本地落盘）。
- **聚合**：`scripts/perf-runs/aggregate.ts`（新增）输出 markdown 报告到 `artifacts/perf/`。
- **CI**：可选 `--scenario baseline` smoke；不卡 build。

### §3.5 Baseline 要求

- 本 spec 通过后第一周必须产出一份 baseline 报告 `artifacts/perf/baseline-YYYY-MM-DD.md`。
- 报告至少包含：硬件（CPU/RAM/OS）、窗口大小、每场景 P50/P95、当前已知异常（regression）。
- 任何后续优化 PR 必须附 "before vs after" 对比。

## §4 Acceptance

1. **测量可重复**：同一场景两次跑 P95 差 ≤ 10%。
2. **故障注入下 invariants 不破**：故障注入后必满足下列全部：
   - **no crash**（进程不 panic / 不 SIGSEGV / 不 deadlock）；
   - **no silent corruption**（grid 与 baseline 字符级等价，或显式走 snapshot resync 后等价）；
   - **expected state transition**（reconnect → desync → snapshot 状态机按 SPEC-REMOTE-001 §3.3 触发；host crash 后 stale_epoch rejection 必出现）；
   - **bounded recovery**（reconnect_time / replay_time / rediscovery_latency 在参考阈值内）；
   - **eventual grid convergence**（**仅 runtime_epoch 未变**时成立；runtime_epoch 改变后 grid 收敛**不**成立，仅 identity 重发现成立）。
3. **baseline 报告**：第一份报告覆盖 §3.2 所有场景。
4. **本地 latency 参考（待 baseline 后定）**：`input_ui_to_render_submit` P95 ≤ 16 ms（与 rAF 同步）。
5. **远端 latency 参考（待 baseline 后定）**：`lan_input_to_render_submit` P95 ≤ 80 ms（局域网常规）。
6. **吞吐参考（待 baseline 后定）**：`pty_output_throughput` 持续 ≥ 10 MiB/s 不掉帧。
7. **stale epoch 检测**：host 重启后 client 第一次 `attach` 必走 `runtime_epoch_stale` 路径；测 `stale_epoch_detection_latency`（参考 SPEC-PROTO-001 §4.7）。
8. **rediscovery**：测 `rediscovery_latency`；不得承诺旧 terminal snapshot resync。

> 4/5/6 条均为参考方向，非硬阈值；具体值等 baseline 后再写入。

## §5 与现有 perf 设施关系

- `wdio.perf.conf.ts` 复用为 e2e 帧延迟测量；`scripts/perf-runs/` 作为落盘点保留。
- `OUTPUT_ACTIVITY_INTERVAL` 等常量保留；测量时报告其值。

## §6 与其他 Spec 关系

- SPEC-TERM-001 §5 与本 spec §3.2 的"渲染一致性"与"延迟"指标共享 acceptance；§3.1 的 PTY 字节序/kernel authoritative 与本 spec §3.1 的 `pty_roundtrip` 起点定义一致。
- SPEC-PROTO-001 §3.4.3（runtime_epoch stale 拒绝）、§3.7（snapshot chunk / realtime 限速）对应本 spec §3.3 故障注入 + §3.2 throughput 场景。
- SPEC-PROTO-001 §3.2 negotiated_version / server_version 与本 spec §3.1 的 latency 测量起点协商无关（mode 与协议版本协商独立）。
- SPEC-PROTO-001 §3.4.3 / §3.7 / §4 acceptance 与本 spec §3.3 的"丢包 → reconnect + resync"是同一现象的不同视角。
- SPEC-REMOTE-001 §3.5 与本 spec §3.4 同列关键场景；§3.5.3 host 重启与本 spec §3.3 故障注入中的"host crash"对应。
- SPEC-REMOTE-001 §3.7 acceptance 与本 spec §4 acceptance 共享"runtime_epoch 未变时 grid 收敛"前提；host restart 后仅 rediscovery + stale_epoch_detection latency 可测。
