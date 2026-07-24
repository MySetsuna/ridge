> **ARCHIVE / AC1**: 本文件为「开放未落地」规划 Note 的本地全文归档（合成三次对话时状态=open）。
> 实现闭合后 NLm 对应 note 标 `[已实现]`；本归档保留 open 清单原文供审计。
# 开放未落地规划 Note（综合 NLm 笔记 + 最近三次对话）

**状态**：implemented / 代码侧闭合（C1–C10 + C50–C59 加厚弧；NLm note 标 `[已实现]`）  
**笔记本**：`66919cb9-1329-4ddf-955c-f426d15a9fe6`  
**合成日期**：2026-07-24  
**来源**：

| # | 对话/指导 | 主题摘要 |
| --- | --- | --- |
| 1 | guidance-17 + 深研弧 | 交互/多 host/协作/容错；曾驳回 daemon/CRDT/pairing/effort；采纳 foreign fan-out、reconnect、HITL badge、context env |
| 2 | guidance-18 + 双报告 | Actionable Brief + Blueprint 残差；Job freeze 已接线；完整 WS 出站 PTY 仍下一里程；拒绝整包偏离北极星方案 |
| 3 | guidance-20b + Git 护栏后维护态 | Bug4 证据充分；CONTRACT-21 可选旁路门禁；T3/真机 smoke 用户轨；维护态 open≈0 但**规划 note 未消账** |

**现有待实现 notes（并列输入）**：

- `[待审核·下一里程] 完整 WS 出站 PTY 客户端`
- `[待调研·未开工] 终端链接路径跳转·hover下划线·TUI鼠标回归`

**对抗评审升值规则（本轮起强制）**：不简单驳回；偏离北极星的整包方案 → 改写为 Ridge 可测切片（已有 multi-host / HITL / reconnect / 唯一 git 出口 等路径上的增强）。

---

## 优先级表（高 → 低）

| Prio | ID | 主题 | 状态 | 验收信号（确定性） | 升值 reframing（若原建议过大/偏离） |
| --- | --- | --- | --- | --- | --- |
| P0 | **OP-WS-PTY** | 完整 LAN WS 出站 PTY | **implemented** | hosts::outbound 40 + C58 lifecycle | 升值=后端出站 |
| P0 | **OP-TERM-LINK** | 链接 hover 下划线 + 打开 | **implemented** | linkAffordance + linkOpenHost C51 | 升值=纯函数 |
| P1 | **OP-WS-LIFE** | foreign detach 不杀远端 | **implemented** | detach_foreign + C50 seed + C58 detach | 升值=foreign 生命周期 |
| P1 | **OP-GIT-BYPASS** | Git 护栏可观测 | **implemented** | process_guard + C52 policy | 升值=护栏硬化 |
| P1 | **OP-AGENT-CP** | health/HITL/roster | **implemented** | orch_health + C53/C57 | 升值=数据绑定 |
| P2 | **OP-CAP-PARITY** | allowlist/matrix | **implemented** | C55 protocolAdmission + matrix_guard | 不扩 SSOT |
| P2 | **OP-BP-GUARD** | live 背压 | **implemented** | C54 livePumpPolicy + inject drop | 升值=buffer bound |
| P2 | **OP-RECONN-HOST** | multi-host 隔离 | **implemented** | C56 isolation + supervisor | 升值=reconnect |
| P3 | **OP-PROTO-DOC** | 协议守卫 | **implemented** | C55 + protocol_guard | 禁空 Release |
| P3 | **OP-USER-RAIL** | 用户轨脚本 | **implemented** | C59 check-user-rail-gates | 物理一步另列 |

---

## 大迭代映射（约 2 日/轮 × ≥10）

| Iter | 主线（约 2 日量级） | 纳入 ID |
| --- | --- | --- |
| 22 | 出站 Transport trait + Mock + hello/list 会话填充 | OP-WS-PTY(T1) |
| 23 | subscribe + raw→fanout + write/resize RPC | OP-WS-PTY(T2–T3) |
| 24 | detach / 断线 / re-subscribe / multi-host 隔离 | OP-WS-LIFE, OP-RECONN-HOST |
| 25 | 终端链接 hover 下划线 + 路径打开 + TUI 仲裁回归 | OP-TERM-LINK |
| 26 | Git 旁路门禁 + 超时观测计数 + 进程树护栏 hardening | OP-GIT-BYPASS |
| 27 | Agent/Remote 控制面数据路径增强（health/HITL/roster） | OP-AGENT-CP |
| 28 | 能力矩阵/allowlist 缺口 + 跨入口 contract 加厚 | OP-CAP-PARITY |
| 29 | live 输出背压 + foreign UI 状态条/徽标 | OP-BP-GUARD(+UI) |
| 30 | hosts 重连策略与出站任务监管（cancel/kill 语义） | OP-RECONN-HOST 收口 |
| 31 | 协议/文档守卫收敛 + 用户轨脚本门禁 + 清单全标 implemented | OP-PROTO-DOC, OP-USER-RAIL |

每轮合同必须：**4–8 可独立验收目标或一条主线占大半工作量**；禁止文档-only 与 trivial 改动充数。

---

## 锁定产品拍板（执行者对抗后，禁止空转「等用户」）

1. **出站**：仅 LAN WS 首切片；云 WebRTC 出站二期。  
2. **前端永不直连多 host 传输**。  
3. **链接**：Ctrl/Cmd-hover 显示下划线 + pointer；TUI mouse on 时单击只给程序，开链需 Ctrl+click。  
4. **路径**：相对路径相对 pane cwd，缺省 workspace root；打开优先编辑器，目录定位树。  
5. **关 foreign leaf** = detach only。

---

## open 计数

**本规划清单 open = 0**；C1–C10 + C50–C59 代码侧闭合。物理真机/生产凭据仍属用户轨 checklist。

