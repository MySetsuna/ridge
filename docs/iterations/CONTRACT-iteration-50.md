# CONTRACT — Iteration 50（未用符号审计 + 终端/IO 性能）

## 背景

近期 AC4 加厚后，Hosts 轮询、hover 链接仲裁、Agent 控制面轮询叠加，终端输入/渲染变卡。同时 ridge 存在 dead_code 警告（reconnect/live_bp 等 API 未接线）。

## 目标

1. **未用符号**：有业务用途的补产品接线；确认废弃才删。
2. **性能**：Hosts 轮询减负；终端 hover 热路径短路；AgentCenter 自适应轮询；泵路径并发。

## 验收

| # | 信号 |
| --- | --- |
| 1 | `cargo test -p ridge --lib hosts::` 无 dead_code 警告（reconnect/live_bp 已用） |
| 2 | Hosts tick：history tail 非每轮全扫；BP 不双 IPC；重连仅在需要时 step |
| 3 | manager hover：无 Ctrl 时不跑 hyperlinkAt/hitTest |
| 4 | vitest hosts 产品路径 + link 测绿 |

## 停机

整终端重写；换渲染引擎。
