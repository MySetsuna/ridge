> **SUPERSEDED for AC4 accounting** — 原拆账不计 10 分。诚实 credit 见 `CONTRACT-iteration-40`…`49` 与 `{SCRATCH}/ac4-ledger.md`。
# CONTRACT — Iteration 30（约 2 日 · OP-RECONN-HOST · 前端 store + 面板泵）

## 范围边界

**本轮独占**：`hosts.ts` attachRemote/detach/pumpAll/reconnect counters；HostsPanel 轮询 pump + 远端接入；outboundReconnect TS 对等。  
后端 delay 公式在 24 已有；30 是 **产品路径接线**。

## 目标（7）

1. attachRemoteHostSession → attach_host_session。
2. detachRemoteHostSession。
3. pumpAllConnectedOutbound 在 Hosts 5s 轮询中调用。
4. HostSession.remoteSessionId 保留后端 id。
5. HostsPanel onAttach 区分 headless/remote。
6. noteOutboundReconnectAttempt / reset store。
7. hostsOutbound.test.ts + outboundReconnect.test.ts。

## 验收

| # | 信号 |
| --- | --- |
| 1 | vitest hostsOutbound + outboundReconnect 绿 |
| 2 | HostsPanel 含 pumpAllConnectedOutbound |
| 3 | hosts.ts 含 attachRemoteHostSession |

## 代码面

`src/lib/stores/hosts.ts`、`HostsPanel.svelte`、`hostsOutbound.test.ts`、`outboundReconnect.ts`。

