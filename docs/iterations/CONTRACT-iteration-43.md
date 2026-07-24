# CONTRACT — Iteration 43 / AC4-C4（约 2 日 · Agent 控制面）

**Credit ID**: C4

## 产品结果

orch health：degraded/level/generation/foreignAttached/outboundHostsConnected；suspend/hitl  bump；AgentCenter 控制面 badge。

## 独占主文件

- `src-tauri/src/teammate/orch_health.rs`
- `src-tauri/src/teammate/suspend.rs` / `hitl.rs`（bump）
- `src/lib/teammate/AgentCenterPanel.svelte`（health 展示）

## 验收

| # | 信号 |
| --- | --- |
| 1 | `cargo test -p ridge --lib orch_health` → `{SCRATCH}/gates-credit-C4.log` |
| 2 | AgentCenter 含 orchLevel / foreignAttached |
