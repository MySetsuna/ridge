> **SUPERSEDED for AC4 accounting** — 原拆账不计 10 分。诚实 credit 见 `CONTRACT-iteration-40`…`49` 与 `{SCRATCH}/ac4-ledger.md`。
# CONTRACT — Iteration 27（约 2 日 · OP-AGENT-CP）

## 范围边界

Agent/Remote 控制面数据：orch health 扩展 + suspend/hitl  bump + AgentCenter 展示；非视觉大改。

## 目标（7）

1. orch_health：degraded / generation / level / foreignAttached / outboundHostsConnected。
2. publish_hosts_control_plane。
3. suspend/resume 变更时 bump_health_generation。
4. hitl set_enabled bump。
5. hosts register_foreign/detach 调 publish。
6. AgentCenterPanel 读全量 health 并展示 level badge。
7. orch_health 单测含 publish + suspend bump。

## 验收

| # | 信号 |
| --- | --- |
| 1 | cargo test orch_health 绿 |
| 2 | AgentCenterPanel 含 orchLevel / foreignAttached |
| 3 | control_plane_level 矩阵测 |

## 代码面

`orch_health.rs`、`suspend.rs`、`hitl.rs`、`hosts/mod.rs` publish、`AgentCenterPanel.svelte`。

