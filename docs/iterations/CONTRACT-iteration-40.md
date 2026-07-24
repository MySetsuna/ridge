# CONTRACT — Iteration 40 / AC4-C1（约 2–3 日 · WS-PTY 全垂直）

**Credit ID**: C1（合并债务 G-WS-PTY，非拆碎 22–29）

## 产品结果

桌面多 Host LAN 出站 PTY：hello/list → subscribe → write/resize → fanout/pump → detach 不杀远端；LAN 相位/背压；Hosts 轮询泵。

## 独占主文件

- `src-tauri/src/hosts/outbound.rs`
- `src-tauri/src/hosts/lan_transport.rs`
- `packages/remote/src/shared/hosts/foreignPaneStatus.ts`(+test)
- `src-tauri/src/hosts/mod.rs`（bind/pump/detach/live cap）
- `src-tauri/src/commands/terminal.rs`（foreign write/resize/kill detach）
- `src/lib/stores/hosts.ts` / `HostsPanel.svelte`（产品路径）

## 验收

| # | 信号 |
| --- | --- |
| 1 | `cargo test -p ridge --lib hosts::` exit 0 → `{SCRATCH}/gates-credit-C1.log` |
| 2 | pump_outbound_to_fanout_feeds_parser 绿 |
| 3 | write_to_pty_async 含 remote_ref |

## 停机

前端多 WebRTC；第二协议 SSOT。
