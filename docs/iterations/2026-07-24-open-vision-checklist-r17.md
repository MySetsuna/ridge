# 开放愿景清单 — Deep Research 弧（iteration 17，对抗全表）

更新：2026-07-24  
报告：`docs/iterations/2026-07-24-deep-research-interaction-multihost.md`  
笔记本临时源：`RESEARCH-REPORT-interaction-multihost`（**豁免清理直至本表无 open 且无「未驳回未实现」**）

## 报告 WI 全表（1.1–5.2）

| WI | 主题 | 状态 | 处理 |
| --- | --- | --- | --- |
| **1.1** | Visual Agents Rail + effort + context 注入 | **implemented*** | *拆分：状态栏/badge+health 落地；`/effort` 模型切换 **驳回**（绑单一 CLI）；**CTX 注入**落地为 spawn-process(is_agent) 写 `RIDGE_WORKSPACE_CONVENTIONS` |
| **1.2** | rdg `-r/-w/-y` 管道 + `@sequence` | **rejected** | 扩协议/产品面过大；Unix 管道 agent 非当前北极星切片 |
| **2.1** | Out-of-process PTY host daemon | **rejected** | 已有本机 PTY + headless native；独立 daemon 新状态源，无痛点证据 |
| **2.2** | Multi-controller 输入仲裁 | **implemented*** | *既有 cloud multi-controller + HITL 远端裁决；本弧补 **reconnect 退避共用** 与 health 可见性 |
| **3.1** | Token-bound action interceptor | **implemented*** | *既有 HITL 网关；本弧补 **pending badge UI** + `pending_count` |
| **3.2** | Multi-teammate pairing portal / 硬件安全 | **rejected** | 屏幕镜像/通用远控非目标；硬件密钥超出范围 |
| **4.1** | Tmux control-mode bridge 全同步 | **rejected** | 已有 teammate tmux shim；完整 control-mode 树同步属大协议，本弧不做 |
| **4.2** | Git worktree 沙箱 | **rejected** | 新状态源/范围过大 |
| **5.1** | Viewport sync server-authoritative | **implemented*** | *非 CRDT；落地 **foreign host 输出 fan-out → pane parser**（`fanout_live_output`） |
| **5.2** | Watchdog / runaway guards | **implemented*** | *既有 ICE/disconnect watchdog；本弧 **reconnect_policy 接 cloud scheduleReconnect + tmux send_retry** |

\*implemented 指 Ridge 可测代码切片闭合报告意图中**可采纳**部分，驳回部分见上表。

## 本弧代码切片（验收 ID）

| ID | 映射 WI | 状态 | 证据 |
| --- | --- | --- | --- |
| R17-CTX | 1.1 | **implemented** | `context_files` 测 + spawn-process env 注入 |
| R17-HOST-PANE | 5.1/多 host | **implemented** | attach + remote_ref + register_foreign |
| R17-HOST-OUT | 5.1 | **implemented** | `fanout_live_output` → parser feed 测 |
| R17-HOST-LIST | 多 host | **implemented** | list_sessions 测 |
| R17-RECONN | 2.2/5.2 | **implemented** | reconnect_policy + cloud TS + tmux retry |
| R17-TEAM-HEALTH | 1.1/2.2 | **implemented** | orch_health 测 + AgentCenter 展示 |
| R17-HITL-BADGE | 3.1 | **implemented** | pending_count + AgentCenter badge |

**open 计数：0**（报告 WI 均 implemented* 或 rejected）

## 驳回总览（不得再当 open）

1.2 · 2.1 · 3.2 · 4.1 · 4.2 · 1.1 中的 `/effort` · 5.1 中的 CRDT
