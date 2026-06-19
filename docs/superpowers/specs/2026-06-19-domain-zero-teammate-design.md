# Domain Zero — 端侧多智能体协同自治系统 实施设计（对账 + 调整）

> 日期：2026-06-19
> 来源方案：`docs/plans/agent-teammate/Domain Zero 总领提要.md` 及 Domain A/B/C/D 四份规范。
> 本文把四份方案与 **现有代码库现状** 对账，记录必要调整，并给出可验证的落地路径。

## 0. 愿景一句话

把 Ridge 从「1 用户 ↔ 1 Agent」的单点终端，进化为「1 用户 ↔ 1 智能体团队 ↔ 共享工作区终端群」的
端侧多智能体协同自治系统：物理骨架层（裸终端也能喊话派活）+ MCP 增强层（结构化静默高速总线）+
逻辑自治层（画像/拓扑/竞选）+ 宿主交互层（上帝视角可视化 + 人类中间审批 HITL + 熔断）。

## 1. 现状盘点（四 Domain 已存在 vs 缺失）

经四路并行勘探（Explore teammates），结论：**地基已铺，上层缺失**。

| 子系统 | 现状 | 关键既有符号 / 位置 |
|---|---|---|
| A1 tmux-shim | **PARTIAL** | 真实 shim 二进制 `src-tauri/src/bin/tmux.rs`(2943行)，拦截 split-window/list-panes/send-keys 等；PATH 注入 `prepend_path_with_wind_tmux_shim`(terminal.rs)；构建 `scripts/ensure-teammate-shim.mjs`。传输=**HTTP/TCP loopback + bearer**（非 UDS+JSON-RPC）。 |
| A2 TML | **MISSING** | 全仓 0 命中。 |
| A3 StreamCleaner | **MISSING** | PTY 读路径 `ridge-tmux spawn_pane` → `take_decoded_utf8` → `chunk::process` 无净化层。 |
| B1 Teammate 画像 | **PARTIAL** | 仅 pane↔agent_id 映射（`teammate_agent_pane_map`）、`PaneState{Idle,Busy,Starting}`、env 注入握手（`RIDGE_TEAMMATE_URL/TOKEN/WORKSPACE_ID`）。无 typed Teammate / role / capabilities / personality。 |
| B2 Topology/竞选 | **MISSING** | 无 petgraph、无 leader 概念、无竞选。`workspace/graph.rs` 是 pane 布局图非 agent 拓扑。 |
| B3 Teammate API | **PARTIAL** | axum HTTP 有 register-agent/find-idle-pane/split/send-keys/list-panes/tmux summon/release。无 get_team_profile/delegate_task/broadcast/report_progress。 |
| B4 Rust Core | **PARTIAL** | `AppState`(parking_lot RwLock)+`Workspace` 侧表。无 DashMap teammates、无 active_leader、无 handle_agent_delegate。 |
| C1/C2/C3 MCP | **MISSING** | 无 MCP。但 `remote/server.rs` 有**完整可复用的 axum WS + JSON-RPC 2.0** 模式（`ws_handler`/`handle_socket`/`jsonrpc_result/error`）。`ridge://` 当前仅 OAuth deeplink。 |
| D1 可视化 | **PARTIAL** | `SplitContainer.svelte` 有 busy/starting 状态徽章（单一 animate-pulse）。无 laser beam、无 Agent Center 侧栏。 |
| D2 HITL/风险 | **PARTIAL** | `capability.rs` 有 **二元** 读写门控 `is_mutating`/`MUTATING_METHODS`/`CapabilitySet.readonly`，dispatch step1.5 拦截。无 L0/L1/L2 分级、无挂起、无审批模态。`RidgeDialog.svelte` 是通用模态。 |
| D3 熔断/冲突 | **MISSING** | 无死循环检测、无文件写锁。`fs_watch.rs`(notify) 仅只读观测。Monaco diff 存在但非冲突仲裁。 |

## 2. 关键调整（「方案不够好 → 据实调整」）

1. **传输保留 HTTP/TCP loopback，不改 UDS+JSON-RPC。**
   规范 A1 要求 Unix Domain Socket + JSON-RPC 2.0。但现有 shim↔teammate-server 已用 `127.0.0.1:<port>` + `X-Ridge-Token`
   全功能跑通，且 **Windows 上 AF_UNIX 工具链脆弱**、跨平台一致性差。重写传输是高风险零收益churn。
   → **保留现有传输**，把规范的*语义*（typed 模型、拓扑、高层 API、MCP）叠加其上。tmux 命令名（pane.split 等）作为内部映射保留即可，无需替换 REST 路径。

2. **Topology 不引 petgraph，手写轻量有向图。**
   ridge-core 是「地基」crate，显式克制依赖（Cargo.toml 注释强调）。团队规模小（数个 pane），
   `HashMap<NodeId, Teammate>` + `Vec<TaskEdge>` 邻接足矣。→ KISS，零新依赖。

3. **typed Teammate 模型层入 `Workspace`，不另起并行 DashMap。**
   规范 B4 的 `DashMap<String,Teammate>` 会与现有 `Workspace` 侧表 desync。
   → 纯模型/拓扑放 ridge-core（可单测），运行时实例挂在现有 `Workspace`（受同一 RwLock 保护），register-agent 升级为「带画像注册」。

4. **MCP 复用 `remote/server.rs` 的 WS+JSON-RPC 模式，挂到 teammate axum。**
   axum 0.7 已含 `ws` feature，无需新 crate。SSE 可选，WS 优先（规范也推荐）。

5. **风险分级扩展 `capability.is_mutating`，不另起 RBAC 系统。**
   在既有二元门控之上加 `RiskLevel{L0,L1,L2}` 分类器（含裸终端命令模式匹配），L2 触发 HITL 挂起。

## 3. 架构分层与落点（调整后）

```
纯核心层 (ridge-core, 零 Tauri, 可单测) ── 本次主交付
  teammate/tml.rs          A2  TML 协议 + 字节状态机解析
  teammate/stream_cleaner  A3  PTY 流净化 (MUTATION_HIDE 等)
  teammate/model.rs        B1  Teammate / AgentRole / Capabilities / Personality
  teammate/topology.rs     B2  TopologyGraph + Leader 竞选 + 性格分派
  teammate/risk.rs         D2  RiskLevel 分类器 (方法 + 裸命令)
  mcp/protocol.rs          C1  JSON-RPC/MCP 报文类型
  mcp/registry.rs          C1  Tool 注册表 + tools/list
  mcp/resource.rs          C2  ridge:// URI 解析 + 内存 Stash

Tauri 接线层 (src-tauri, 需 rebuild 验证) ── Phase 2
  teammate/server.rs       B3  高层 API 路由 (get_team_profile/delegate_task/...)
  Workspace 状态           B4  挂 TopologyGraph 实例 + 带画像注册
  ridge-tmux spawn_pane    A3  StreamCleaner 接入读路径
  风险网关                 D2  send-keys/tools_call 前置 classify → L2 挂起
  MCP server 挂载          C   /api/v1/mcp/ws (复用 WS 模式)

前端层 (Svelte 5, check/vitest/CDP 可验证) ── Phase 2
  Agent Center 侧栏        D1  Objective/Roster/Audit/DAG
  Pane 状态呼吸灯          D1  Thinking/Executing/Idle
  HITL 审批模态            D2  Approve/Reject/Modify
  协作连线 overlay         D1  SVG 贝塞尔光束
```

## 4. 执行方式（用 Teammate 方式）

- **Phase 1（本会话主目标，可验证）**：4 个并行 teammate 各产出一块**互不依赖的纯核心 + 单测**
  （TM-A: tml+cleaner；TM-B: model+topology；TM-C: mcp；TM-D: risk）。各只写自己的 stub 文件；
  我（lead）负责 `mod.rs`/`lib.rs` 接线与**权威集成测试** `cargo test -p ridge-core`。这是验证置信度最高的一层。
- **Phase 2（部分本会话）**：前端层用 `pnpm check`/`vitest`/CDP 自验；后端接线层产出代码并**标注待 rebuild**
  （后端 rebuild 会杀本托管会话，最终 e2e 交用户/或用 `tauri:dev:cdp` 独立实例）。

## 5. 不做 / 延期边界

- 不重写 shim 传输为 UDS。
- D3 熔断/文件写锁：依赖运行时信号，放 Phase 2 末或后续（先打 risk/HITL 地基）。
- C2 的 git-status/editor-context 资源：git-status 需把现有 shell-out 结果缓存进内存（Phase 2）；
  editor-context 在前端，需新增 Rust 侧镜像（延期）。
- 真机/手机 e2e 一律交用户，遵循「后端改动需 rebuild」既有约束。

## 6. 验证矩阵

| 层 | 验证手段 | 杀会话? |
|---|---|---|
| ridge-core 纯核心 | `cargo test -p ridge-core` | 否 |
| 前端 | `pnpm check` + `vitest` + 可选 `tauri:dev:cdp` | 否 |
| Tauri 后端接线 | 需 rebuild 覆盖安装/重启，或 `tauri:dev:cdp` 独立实例 | 是（正式版）/否（dev:cdp 并存） |
</content>
