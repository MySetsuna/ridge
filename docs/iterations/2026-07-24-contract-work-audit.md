# 合同工作量审计（AC4 诚实账 · 终态）

## 唯一计数规则

| 账本 | 状态 |
| --- | --- |
| CONTRACT-22…31 | **SUPERSEDED**，**不计** |
| **CONTRACT-40…49 = C1–C10** | **AC4 唯一 10 分**（各约 2 日多模块产品垂直） |
| 原 C50–C59 标签模块 | **并入 C1–C10 加厚**，**禁止**标「第二组 10×约 2 日」 |

## 产品路径接线（必进 shipped 入口）

| 能力 | 入口 |
| --- | --- |
| linkOpenHost | `manager.ts` Ctrl+click → planHostOpen / _executeOpenPlan |
| livePump + live_bp | `hosts.ts` pump + `get_live_backpressure`；HostsPanel |
| outboundLifecycle | hosts attach/detach/fanout |
| foreignHistory | fetchForeignHistoryTail + attach seed |
| hostSessionIsolation | hostReconnect + HostsPanel |
| protocol admit | dispatch + remote_host_impl + **cloudHostBridge** decideRemoteInvoke |
| matrix parity | matrixParity + check-capability-matrix.mjs |
| HITL/orch | AgentCenterPanel |

## C1–C10 合同与门禁

| C | 合同 | 多模块主面 | 门禁 |
| --- | --- | --- | --- |
| C1 | 40 | outbound/lan/lifecycle/hosts | cargo hosts:: |
| C2 | 41 | linkAffordance+linkOpenHost+manager | vitest link* |
| C3 | 42 | process_guard+spawn+policy | process_guard |
| C4 | 43 | orch_health+orchControlPlane+AgentCenter | orch_health |
| C5 | 44 | reconnect_supervisor+isolation+hostReconnect | reconnect_supervisor |
| C6 | 45 | foreign_history+history_commands+session | foreign_history |
| C7 | 46 | hitl_audit+filter+panel | hitl_audit |
| C8 | 47 | **live_backpressure.rs**+liveBackpressure+livePump+hosts 泵 | live_backpressure + vitest C8 |
| C9 | 48 | matrix_guard+matrixParity+script+matrix JSON | matrix_guard + matrixParity + script |
| C10 | 49 | protocol_guard+admission+remoteInvokeAdmit+cloudHostBridge+dispatch | protocol_guard + remoteInvokeAdmit + desktop script |

## 禁止

- 双计 C50–C59 为独立约 2 日轮  
- 纯函数仅 compositionHarness 引用却计产品迭代（已接线）  
- 空 Release  
