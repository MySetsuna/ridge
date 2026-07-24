> **SUPERSEDED for AC4 accounting** — 原拆账不计 10 分。诚实 credit 见 `CONTRACT-iteration-40`…`49` 与 `{SCRATCH}/ac4-ledger.md`。
# CONTRACT — Iteration 22（约 2 日 · OP-WS-PTY T1 · 出站 Transport 内核）

## 范围边界（本轮独占，不与 23–24 重复计）

- **纳入**：`OutboundTransport` trait、`MockOutboundTransport`、mux wire encode/demux、`OutboundClient::connect_and_list`、`OutboundRegistry`、`bind_outbound_and_list` 填 sessions。
- **明确下轮**：subscribe/write/resize（23）；detach/pump UI（24）。

## 目标（6）

1. 抽象 `OutboundTransport`（send_json_rpc / send_raw / drain_pane_raw）。
2. Mock 可预设 list/hello 结果，RPC 日志可断言。
3. `connect_and_list` 状态机 Idle→Connecting→HelloOk→Listed。
4. `wire::encode_pane`/`demux_pane` 与 ridge-cli 帧布局一致。
5. `OutboundRegistry` 按 host_id 隔离客户端。
6. `bind_outbound_and_list` 写入 HostRecord.sessions + Connected。

## 验收

| # | 信号 |
| --- | --- |
| 1 | `cargo test -p ridge --lib hosts::outbound` 含 hello_list… multi_host 绿 |
| 2 | wire_pane_roundtrip 绿 |
| 3 | 源码可指：`src-tauri/src/hosts/outbound.rs` |

## 停机

前端直连多 host WebSocket；第二协议 SSOT。

## 代码面（本轮主交付）

`src-tauri/src/hosts/outbound.rs`（新建核心）、`hosts/mod.rs` 中 bind_outbound_and_list 接线。

