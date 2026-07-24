# 开放愿景清单 — 双报告对账（Actionable Brief + Architectural Blueprint）

更新：2026-07-24  
笔记本源（临时）：`08404cf1-…` Actionable Product Engineering Brief；`2a91f09c-…` Architectural Blueprint  
本地归档：

- `docs/iterations/2026-07-24-deep-research-actionable-brief.md`（≈ report1 / WI 1.1–5.2）
- `docs/iterations/2026-07-24-deep-research-architectural-blueprint.md`（report2 / F1–F8）
- 既有：`2026-07-24-deep-research-interaction-multihost.md`（report1 同文本地副本）
- 既有闭合：`2026-07-24-open-vision-checklist.md`（iter 15–16）、`…-r17.md`（iter 17）

规则：每行 **implemented**（码 + 确定性测）或 **rejected**（理由）；**open = 0** 后方可删研究源。

---

## Report 1 — Actionable Brief（WI 1.1–5.2）

| WI | 主题 | 状态 | 证据 / 理由 |
| --- | --- | --- | --- |
| **1.1** | Agents Rail + `/effort` + 上下文注入 | **implemented*** | *Rail/badge+health：`orch_health` + AgentCenter；CTX：`context_files` + spawn `RIDGE_WORKSPACE_CONVENTIONS`。**`/effort` rejected**（绑单一 CLI） |
| **1.2** | rdg `-r/-w/-y` + `@sequence` | **rejected** | 扩协议/管道 agent 产品面过大；非北极星切片 |
| **2.1** | Out-of-process PTY host daemon | **rejected** | 已有本机 PTY + headless；独立 daemon 新状态源，无痛点证据 |
| **2.2** | Multi-controller 输入仲裁 | **implemented*** | 既有 cloud multi-controller + HITL；本弧 `reconnect_policy` + health |
| **3.1** | Token-bound action interceptor | **implemented*** | HITL 网关 + `pending_count` badge |
| **3.2** | Pairing portal / 硬件密钥 | **rejected** | 通用远控/硬件密钥非目标 |
| **4.1** | Tmux control-mode 全树同步 | **rejected** | 已有 teammate tmux shim；完整 control-mode 属大协议 |
| **4.2** | Git worktree 沙箱 | **rejected** | 新状态源/范围过大 |
| **5.1** | Viewport sync（报告写 CRDT/OT） | **implemented*** | *非 CRDT*；`fanout_live_output` → pane parser（server-authoritative 字节扇出） |
| **5.2** | Watchdog / runaway guards | **implemented*** | ICE/disconnect watchdog + `reconnect_policy`（cloud scheduleReconnect + tmux send_retry） |

\*implemented = 报告意图中可采纳的 Ridge 可测切片；驳回子项见表 + 下「驳回总览」。

### Report 1 代码切片（验收 ID）

| ID | WI | 状态 | 路径 / 测 |
| --- | --- | --- | --- |
| R17-CTX | 1.1 | implemented | `teammate/context_files.rs` + spawn env |
| R17-HOST-PANE | 5.1 | implemented | `hosts::attach_host_session` + `remote_ref` + foreign registry |
| R17-HOST-OUT | 5.1 | implemented | `hosts::fanout_live_output` → parser；测含 viewport 内容 |
| R17-HOST-LIST | multi-host | implemented | `hosts` list_sessions |
| R17-RECONN | 2.2/5.2 | implemented | `reconnect_policy.rs` + `reconnectPolicy.ts` + tmux/cloud |
| R17-TEAM-HEALTH | 1.1/2.2 | implemented | `orch_health.rs` + AgentCenter |
| R17-HITL-BADGE | 3.1 | implemented | `hitl::pending_count` + AgentCenter badge |

---

## Report 2 — Architectural Blueprint（F1–F8）

| ID | 主题 | 状态 | 证据 / 理由 |
| --- | --- | --- | --- |
| **F1** | Remote Host Live PTY Attach | **implemented*** | TCP `probe_tcp`；`attach_host_session`；`live_sink` 写路由；`fanout_live_output`。**完整 LAN/WS 出站 PTY 客户端** = 下一里程（非 open，见 non-goals） |
| **F2** | Windows Job Object Freeze | **implemented*** | spawn：`create_job`+`assign_pid`→`PtyHandle.job`；`suspend_agent` 读 `job`/`child_pid`→`try_freeze_primary(Some(job)|None)`——**None 直 `os_freeze`，禁止 create_job**；resume 同理。树级 `JobObjectFreezeInformation` 未绑——不另开 open |
| **F3** | Git-Stash-style Rollback | **implemented** | `teammate/rollback.rs` checkpoint/rollback 测 |
| **F4** | Workspace Memory UI | **implemented*** | goal/constraints/tasks API+UI（M1）。**cgroup 进程内存条** ≠ Workspace Memory 产品语义 → **rejected** 子项（OS 监控非 M1） |
| **F5** | Agent CLI Process Discovery | **implemented** | `teammate/discover.rs` 测 |
| **F6** | Mobile Copy Without Keyboard | **implemented** | `packages/remote/.../mobileCopy.test.ts` |
| **F7** | Mouse SGR 1006 Clicks | **implemented** | 既有终端鼠标报告路径（V-TUI-CLK） |
| **F8** | Multi-line Paste Fidelity | **implemented** | bracketed paste / paste 序（V-PASTE） |

### Report 2 与既有清单映射

| Blueprint | 既有 ID |
| --- | --- |
| F1 | V-H1 + V-H1-LIVE + R17-HOST-* |
| F2 | V-G1-JOB + V-G1-OS |
| F3 | V-G1-RB |
| F4 | V-M1-S3 |
| F5 | V-DISC |
| F6 | V-MOB-CP |
| F7 | V-TUI-CLK |
| F8 | V-PASTE |

---

## 驳回总览（不得再当 open）

Report1：1.2 · 2.1 · 3.2 · 4.1 · 4.2 · 1.1/`/effort` · 5.1/CRDT-OT  
Report2：F4 中 cgroup 内存条；F1 完整出站 WS PTY（**下一里程**，非 open）

## Residual open set

**空。** F2：初接线误在 None 路径 `create_job`（create 失败可挡 thaw）。已修为 None→`os_freeze`；`suspend_agent`/`resume_agent` 读 `PtyHandle.job`。open 仍 0。

## open 计数

**0**

## 门禁（本弧）

```
cargo test -p ridge --lib reconnect_policy
cargo test -p ridge --lib hosts::
cargo test -p ridge --lib teammate::
```

日志：`{SCRATCH}/dual-report-gates.log`（实现者 scratch）。
