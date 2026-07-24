> **SUPERSEDED for AC4 accounting** — 原拆账不计 10 分。诚实 credit 见 `CONTRACT-iteration-40`…`49` 与 `{SCRATCH}/ac4-ledger.md`。
# CONTRACT — Iteration 23（约 2 日 · OP-WS-PTY T2–T3 · I/O 路由）

## 范围边界

- **纳入**：subscribe/unsubscribe/write_pty/resize；attach 绑 live_sink→write；**修复** `write_to_pty_async` 漏 foreign；`route_foreign_resize`。
- **不含**：detach UI、pump 命令（24）。

## 目标（6）

1. `OutboundClient::subscribe` / `write_pty` / `resize_pane` 需已 subscribe。
2. attach_host_session 有 outbound 时 subscribe + sink 调 write_pty。
3. write_to_pty_async 与 write_to_pty_inner 对称走 remote_ref。
4. resize_pane_inner foreign 分支 route_foreign_resize。
5. write 无 subscribe 失败可测。
6. stats.write_ok / resize_ok 递增。

## 验收

| # | 信号 |
| --- | --- |
| 1 | hosts 测 bind→subscribe→write 计数 |
| 2 | `rg remote_ref write_to_pty_async` 有分支 |
| 3 | write_without_subscribe_fails 绿 |

## 代码面

`hosts/outbound.rs` 方法、`hosts/mod.rs` attach sink、`commands/terminal.rs` async write + resize。

