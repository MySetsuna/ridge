# CONTRACT — Iteration 17：交互/多 host/协作/容错（深研弧）

日期：2026-07-24  
报告：`RESEARCH-REPORT-interaction-multihost`（豁免清理至 R17-* 全 implemented）

## 目标（全部本轮）

| ID | 验收 |
| --- | --- |
| R17-CTX | `teammate::context_files` 或等价模块 + cargo 单测 |
| R17-HOST-PANE | attach 写入 workspace pane + remote_ref；测 |
| R17-HOST-OUT | `inject_live_output` + 测 |
| R17-HOST-LIST | `list_host_sessions` 命令 + 测 |
| R17-RECONN | `reconnect_policy` 纯函数 + 测 |
| R17-TEAM-HEALTH | `orchestration_health` 快照 + 测 |
| R17-HITL-BADGE | `pending_hitl_count` + 测 |

## 门禁

- `cargo test -p ridge --lib hosts::`
- `cargo test -p ridge --lib teammate::`
- 相关新测全绿

## 不做

E2EE/SSOT、CRDT、屏幕镜像、生产部署、auto-merge。

## 停机

任一门禁红 → 修到绿；不得在 open>0 时删报告源。
