> **SUPERSEDED for AC4 accounting** — 原拆账不计 10 分。诚实 credit 见 `CONTRACT-iteration-40`…`49` 与 `{SCRATCH}/ac4-ledger.md`。
# CONTRACT — Iteration 24（约 2 日 · OP-WS-LIFE · detach + pump 生产路径）

## 范围边界

- **纳入**：detach_foreign / detach_host_session；disconnect 清订阅；reconnect_resubscribe；**pump_host_output 接线**；foreignPaneStatus 纯函数。
- **不含**：Hosts 前端 store 全量（30）、orch 控制面（27）。

## 目标（7）

1. detach 清 foreign map + unsubscribe + 不杀远端。
2. detach_host_session Tauri 命令。
3. pump_outbound_to_fanout → pump_host_output 命令（消除 dead_code）。
4. pump 测：inject → pump → parser 含文本。
5. reconnect_resubscribe 无双订。
6. foreignPaneStatus decide badge / close copy。
7. forget 清 outbound client。

## 验收

| # | 信号 |
| --- | --- |
| 1 | pump_outbound_to_fanout_feeds_parser 绿 |
| 2 | detach 后 is_subscribed=false |
| 3 | lib.rs 注册 pump_host_output |
| 4 | vitest foreignPaneStatus |

## 代码面

`hosts/mod.rs` detach/pump 命令与测、`foreignPaneStatus.ts`、`lib.rs` 注册。

