# Ridge 架构总览

> 单一入口：本目录是项目的权威架构说明。`specs/L2-*.md` 在仓库根
> 的副本只是过渡锚点，所有新建 / 修改应直接落到本目录的对应文件。

## 目录

```
docs/architecture/
├── README.md                           ← 本文件（入口 / 索引）
├── 00-overview.md                      ← 系统拓扑、组件清单、模块边界
├── 10-kernel.md                        ← ridge-kernel 内部架构
├── 20-rtp1-wire.md                     ← RTP1 wire 协议 + 22 消息类型
├── 30-remote-lifecycle.md              ← attachment state machine + recovery
├── 40-cli.md                           ← ridge CLI / 旧 rdg 路径
├── 50-desktop-tauri.md                 ← Tauri 桌面端 + WebGPU 渲染
├── 60-data-flow.md                     ← 端到端数据通路
├── 70-runtime-epoch.md                 ← host_id / runtime_epoch / 身份
├── 80-security.md                       ← 鉴权 / controller_id / 输入所有权
├── 90-performance.md                   ← 性能契约 / 故障注入 / baseline
├── RIDGE-RUNTIME-FOUNDATION-FINAL.md   ← Foundation 终报告（沿用）
├── notes/                              ← 历史验收文档（保留作 trace）
│   ├── IMPLEMENTATION-ACCEPTANCE.md
│   ├── PHASE-A-FINAL-ACCEPTANCE.md
│   └── PHASE-A6-LIVE-VALIDATION.md
└── specs/                              ← L2 spec（沿用）
    ├── L2-PROTO-001.md   RTP1 wire protocol
    ├── L2-REMOTE-001.md  Remote session lifecycle
    ├── L2-TERM-001.md    Terminal rendering contract
    └── L2-PERF-001.md     Performance & fault injection
```

## 文档地图

按阅读顺序：

1. **00-overview.md** — 第一次来必看。组件拓扑、模块边界、不变量。
2. **10-kernel.md** — kernel 是 PTY / lifecycle / output_seq 的唯一权威。
3. **20-rtp1-wire.md** — kernel ↔ remote 的 canonical wire。
4. **30-remote-lifecycle.md** — attachment state machine + 重连 / 恢复剧本。
5. **40-cli.md** — headless host（ridge / 旧 rdg）的内部架构。
6. **50-desktop-tauri.md** — Tauri 桌面端 + WebGPU。
7. **60-data-flow.md** — 跨模块端到端追踪。
8. **70-runtime-epoch.md** — kernel boot 时 mint 的 UUID v7 身份。
9. **80-security.md** — 鉴权 / 输入所有权 / 传输完整性。
10. **90-performance.md** — baseline + 故障注入 + 优化闸。

## 与 specs 的关系

| Spec | 对应实现 / 测试 |
|---|---|
| `L2-PROTO-001.md` | `packages/ridge-kernel/src/rtp1.rs` + `rtp1_ws.rs` + `rtp1_session.rs`；`packages/ridge-kernel/tests/conformance_rtp1.rs`（12 acceptance）+ `packages/ridge-cli/tests/rtp1_kernel_e2e.rs`（live WS） |
| `L2-REMOTE-001.md` | `packages/ridge-kernel/src/pty.rs`（lifecycle）+ `rtp1_session.rs`（attachment state machine）；`packages/ridge-kernel/tests/stability_fault.rs` |
| `L2-TERM-001.md` | `packages/ridge-kernel/src/pty.rs`（PTY registry + bounded replay）；`packages/ridge-kernel/tests/terminal_live.rs`（16 live PTY scenarios）+ `tests/performance_baseline.rs` |
| `L2-PERF-001.md` | `packages/ridge-kernel/tests/performance_baseline.rs`；故障注入脚本与 vte/clipboard 集成见各模块测试 |

## 测试矩阵

```
cargo test -p ridge-kernel --lib             78 (rtp1/rtp1_session/rtp1_ws/pty/...)
cargo test -p ridge-kernel --test conformance_kernel_backed
cargo test -p ridge-kernel --test conformance_replay
cargo test -p ridge-kernel --test conformance_runtime
cargo test -p ridge-kernel --test conformance_rtp1          ← 12 RTP1 acceptance + 7 remote lifecycle
cargo test -p ridge-kernel --test kernel_backend_waterfall
cargo test -p ridge-kernel --test stability_fault            ← 13 (incl. per-controller wire validation)
cargo test -p ridge-kernel --test terminal_live             ← 16 live OS PTY scenarios
cargo test -p ridge-kernel --test performance_baseline      ← 5 perf baselines (--ignored)
cargo test -p ridge-cli      --bin ridge                     175 lib
cargo test -p ridge-cli      --test rtp1_kernel_e2e          ← 1 live kernel + WS
cargo test -p ridge-cli      --test kernel_lifecycle_e2e     4/5 (1 pre-existing harness timeout)
```

总计：149 kernel + 175 ridge-cli + 1 live WS e2e + 4/5 pre-existing e2e = PASS。

## 交叉引用

- `CLAUDE.md` (仓库根) — Claude Code 项目指令
- `.claude/` (仓库根) — Claude Code 本地设置
- `AGENTS.md` (仓库根) — 项目 agent 指引（背景）
