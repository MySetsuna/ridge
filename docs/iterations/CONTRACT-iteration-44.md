# CONTRACT — Iteration 44 / AC4-C5（约 2 日 · Outbound reconnect supervisor）

**Credit ID**: C5 · **NEW code** (not relabel of 22–31)

## 产品结果

多 host 断线后可调度、可取消的 resubscribe；`step_host_reconnect` / `cancel_host_reconnect` 桌面命令；disconnect 自动 schedule。

## 独占主文件

- `src-tauri/src/hosts/reconnect_supervisor.rs`（新建）
- `hosts/mod.rs` schedule on disconnect + commands
- desktop_surface allowlist 扩展

## 验收

| # | 信号 |
| --- | --- |
| 1 | `cargo test -p ridge --lib hosts::reconnect_supervisor` → gates-credit-C5.log |
| 2 | cancel_stops_waiting_loop / unreachable_exhausts_attempts 绿 |
