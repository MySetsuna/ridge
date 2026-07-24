# Inventory — Remote dual-end / multi-host team / agent panel / mobile touch

日期：2026-07-24（iteration 19）  
方法：codegraph + 源码读路径；非 PROJECT-STATE 臆测。

## A. Remote dual-end isolation & sync

| 路径 | 符号/文件 | 现状 | 失败模式（代码事实） |
| --- | --- | --- | --- |
| Cloud host pane fan-out | `cloud_pane.rs` subscribe/unsubscribe + refcount + `remove_desync_if_owner` | 已实现 multi-controller refcount；desync 按 owning sub_id | 快速退订→重订阅竞态已用 sub_id 防误删 |
| Host bridge subscribe | `cloudHostBridge.handleSubscribePane` + `makeCloudHostPaneSource` | live bytes + history-pull 首屏 | 无 paneOutputSource 时仅占位 |
| Controller resub | `cloudRemote._handleReconnect` → `_teardownSubscriptions` | 已修「重连有 scrollback 无 live」 | 需 teardown 后才能 re-subscribe |
| LAN WS | `wsRemote.subscribePane` | 显式 subscribe-pane | 与 cloud 路径分叉但契约化 |
| Capability | `REMOTE_ALLOWLIST` / `capabilityContract` | teammate 读/裁决 | **缺口**：`get_orchestration_health` 曾未放行 |

## B. Multi-host team

| 路径 | 符号 | 现状 |
| --- | --- | --- |
| TCP probe / attach | `hosts::probe_tcp`, `attach_host_session` | implemented |
| live_sink / fanout | `hosts::fanout_live_output` → parser | implemented |
| list sessions | hosts list | implemented |
| 完整 WS 出站 PTY | — | **下一里程**（non-goal） |

## C. Agent monitor panel

| 路径 | 现状 | 缺口 |
| --- | --- | --- |
| Desktop `AgentCenterPanel` | orch health + HITL badge + memory | — |
| Remote `SidebarTeamRoster` | topology + list_hitl + resolve | **无 orch health 徽章**；Suspended 点状态弱 |
| Topology status | `status: Suspended` 投影 | UI 未强调暂停态 |

## D. Mobile touch / scroll / swipe → TUI

| 路径 | 现状 | 缺口 |
| --- | --- | --- |
| `TerminalCanvas.touchWheel` | mouse-report → SGR wheel；else 本地 scrollback | **缺 alt-screen wheelAltScroll 对等**（less/claude 菜单滑不动） |
| Selection-as-mouse release | 用 btn 0 action 1 | **应对齐桌面 btn=3 release** |
| Desktop `handleWheel` / `wheelAltScroll` | 完整 | mobile 未复用决策 |

## Open rows → checklist

见 `2026-07-24-open-vision-checklist-r19-remote.md`。
