# CONTRACT — Iteration 47 / AC4-C8（约 2 日 · Live backpressure 全垂直）

**Credit ID**: C8

## 产品结果

Live 输出背压：**Rust** 每会话 drop 计数 + 聚合快照命令 + inject 路径接线；**TS** liveBackpressure/livePumpPolicy 策略；**hosts store** 泵路径更新；**HostsPanel** 徽章。

## 独占主文件（多模块）

- `src-tauri/src/hosts/live_backpressure.rs`(+tests)
- `src-tauri/src/hosts/mod.rs` inject → live_bp.record_inject + `get_live_backpressure`
- `packages/remote/src/shared/hosts/liveBackpressure.ts`(+test)
- `packages/remote/src/shared/hosts/livePumpPolicy.ts`(+test)
- `src/lib/stores/hosts.ts` notePumpBatch / pumpBadge
- `src/lib/stores/hostsOutboundProduct.test.ts`
- `src-tauri/src/hosts/desktop_surface.rs`（get_live_backpressure desktop-only）

## 验收

| # | 信号 |
| --- | --- |
| 1 | `cargo test -p ridge --lib live_backpressure` exit 0 |
| 2 | vitest liveBackpressure + livePumpPolicy + hostsOutboundProduct |
| 3 | inject_live_output 后 registry total_dropped 可测 |

## 停机

cgroup 内存条；通用 VNC。
