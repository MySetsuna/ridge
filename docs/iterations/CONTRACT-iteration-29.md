> **SUPERSEDED for AC4 accounting** — 原拆账不计 10 分。诚实 credit 见 `CONTRACT-iteration-40`…`49` 与 `{SCRATCH}/ac4-ledger.md`。
# CONTRACT — Iteration 29（约 2 日 · OP-BP-GUARD + LAN transport 状态机）

## 范围边界

**本轮独占**：`lan_transport.rs` 生产形态相位/背压；live_output_cap；append_capped。  
**不与 22 重复计**：22 是 Mock trait 内核；29 是 LAN 相位机 + 背压。

## 目标（6）

1. LanOutboundTransport 相位 Idle→Resolving→Handshaking→Ready。
2. max_pending_rpc 满则 backpressure reject。
3. endpoint_url 不泄露 token。
4. HostRegistry live_output_cap + inject dropped。
5. append_capped 单测。
6. lan 接 OutboundClient list/subscribe 绿。

## 验收

| # | 信号 |
| --- | --- |
| 1 | cargo test hosts::lan_transport 绿 |
| 2 | live_output_cap_drops_overflow 绿 |
| 3 | backpressure_rejects_when_pending_full 绿 |

## 代码面

`lan_transport.rs`、`hosts/mod.rs` cap、`outbound.append_capped`。

