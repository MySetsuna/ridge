# 智能体协同增强 实现规划/设计文档

> 日期：2026-06-30 · 分支 develop · 上游：`2026-06-19-domain-zero-teammate-design.md`（落地）+ `2026-06-20-team-agent-upgrade-plan-design.md`（底座化瘦身，权威）
> 约束：硬约束 D1–D9 已拍板。本机后端改动需 rebuild 才生效，**rebuild 会杀当前会话** → 所有后端验证写成可交接手动步骤。

---

## 第一部分 · 现状勘察结论（每条带 文件:行 证据）

### A. 内置 MCP / teammate 通信链路：地基已通，三处缺口

**MCP server 确实已挂载到 teammate 服务**，链路端到端存在但有具体缺陷：

| 事实 | 证据 |
|---|---|
| MCP WS 已挂载 axum | `src-tauri/src/teammate/server.rs:336` `.route("/api/v1/mcp/ws", get(route_mcp_ws))` |
| WS 鉴权（bearer/x-ridge-token） | `server.rs:625-634` `route_mcp_ws` 调 `auth_ok`；消息循环 `mcp_socket:636-649`；分发 `handle_mcp_message:651-682` |
| `initialize`/`tools/list`/`tools/call`/`resources/read` 都实现 | `handle_mcp_message:665-680` |
| env 注入到每个 teammate 分屏 | `commands/terminal.rs:682-702`（`(Some(bind), _)` arm）注入 `RIDGE_TEAMMATE_URL`/`RIDGE_TEAMMATE_TOKEN`/`RIDGE_WORKSPACE_ID` |
| 令牌+端点状态 | `state.rs:69` `TeammateBinding{base_url,token}`，`state.rs:459` `teammate_binding`，写入 `server.rs:277-282` |
| 服务按需惰性启动 | `server.rs:154` `ensure_teammate_started`（首个 PTY 时拉起） |
| 已存在一份完整接入文档 | `docs/mcp-integration.md`（端点/token/工具/资源/Node 示例/诚实列出限制，195 行） |

**缺口 1（最关键 · 寻址不自洽，会导致「自由交流」实际打不通）**：
- 工具 `ridge_send_to_teammate`/`ridge_delegate_task` 把 `target_pane_id` 解析为**数字 pane 索引**（0 起）：`server.rs:699-702` `args.get("target_pane_id").and_then(|v| v.as_u64())`。
- 但资源 `ridge://workspace/active-panes` 返回的花名册里，每个成员的 `paneId` 是 **Uuid 字符串**，且**没有数字索引字段**：`commands/teammate.rs:39` `"paneId": pane.to_string()`、`teammate/profiles.rs:78` 同。
- 即：智能体读花名册拿到的是 Uuid，但发消息要求传索引 → **拿不到可靠的寻址键**，两个已注册 teammate 之间无法稳定互发。这是 P1 必须修的具体 bug。

**缺口 2（工作区寻址）**：`mcp_tools_call` 一律落在 `*ctx.state.active_workspace.read()`（`server.rs:696`），**不读调用方 WS 升级时携带的 `X-Ridge-Workspace` 头**（`route_mcp_ws` 只做 auth，未取 workspace）。单一活动工作区的常见场景能跑通；跨/非活动工作区会投错。`docs/mcp-integration.md:160` 已诚实标注此限制。

**缺口 3（工具覆盖）**：`tools/list` 列 5 个工具（`registry.rs:30-117`：split_pane/send_to_teammate/delegate_task/stash_data/get_team_profile），但 `tools/call` 只路由 2 个（send/delegate），其余返回 `unknown tool`（`server.rs:729`）。`resources/read` 只接 active-panes（`server.rs:761`）。无 `notifications/progress`（`server.rs:622-623` 注释）。

**通信落点本质**：`ridge_send_to_teammate` = 把 `message + "\n"` 当 PTY 字节写入目标分屏 stdin（`server.rs:712-713`）。这是物理骨架层「向终端注入文本」，靠目标分屏里跑的交互式 agent 读 stdin 生效。结构化 TML 总线已被 06-20 瘦身**刻意删除**，不复活（符合 D2/D8）。

### B. 「智能体 tab」考古：被瘦身成什么、留下什么

**它从未是一个独立的「智能体编排 tab」**，而是经历了两次形态收缩：

1. **形态变迁**：最初是「钉在每个工作区 tab 底部的全局插件」→ 现已改为**左侧图标栏的独立 Tab**（`sidebarTab='agents'`）。证据：`src/lib/plugins/index.ts:30-32`（注释说明不再注册为 global 插件）、`src/routes/+page.svelte:297`（`SidebarTab` 含 `'agents'`）、`+page.svelte:1463-1471`（图标按钮，`teammateEnabled` 门控）、`+page.svelte:1558-1563`（挂载 `<AgentCenterPanel workspaceId={$activeWorkspaceId}/>`）。

2. **能力瘦身（2026-06-20「底座化改造」权威计划）**：`docs/superpowers/specs/2026-06-20-team-agent-upgrade-plan-design.md` 明确「保留给人用的、砍掉给 AI 自治用的」。**被删/冻**：TML 协议+StreamCleaner、广播抢单、Leader AI 竞选、性格分派、DAG/Objective/协作审计三区、`set_tml_stream_enabled`。证据：该文档 §2 处置总表 + `AgentCenterPanel.svelte:1-11` 头注释「底座化瘦身后只保留成员(Roster)+异常(熔断告警)；目标/活动(TML 协作审计)已退场」+ `teammateModel.ts:13` 「TML 协作审计已退场」+ `teammateSettings.ts:13-14`。

**留下的可复用残件（直接拿来扩展，符合 D8「以 AgentCenterPanel 为唯一载体」）**：

| 残件 | 文件 | 可复用价值 |
|---|---|---|
| 指挥部侧栏（唯一载体） | `src/lib/teammate/AgentCenterPanel.svelte`（162 行，Roster+熔断+HITL 快捷开关） | P3 编组 UI 的宿主 |
| 前端类型/解析层 | `src/lib/teammate/teammateModel.ts`（TopologySnapshot/HitlRequest/CircuitTrip 防御式解析） | P3 复用 `TeammateProfile`/`statusDot`，扩 group 模型 |
| HITL 模态 + 开关桥 | `HitlApprovalModal.svelte`、`teammateSettings.ts` | 零回归保留（D7） |
| 进程级花名册 | `src-tauri/src/teammate/profiles.rs`（`upsert`/`remove_by_pane`/`has`/`topology_for`，**无持久化**，LazyLock Mutex） | P3 成员来源（注意：无持久化，群组需独立持久化） |
| 拓扑命令 | `commands/teammate.rs:22 topology_json`、`:50 get_teammate_topology` | P3 拉成员、P1 资源读 |
| 高层 HTTP API | `server.rs:332-334` team-profile/delegate-task/report-progress | P3「给组派任务」复用 delegate（D2/D4） |
| MCP 纯核心 | `packages/ridge-core/src/mcp/{protocol,registry,resource}.rs`（含 `StashStore` FIFO，单测齐全） | P1 扩工具/资源 |

### C. 内置文件编辑器打开机制（D5 依据）

- 编程式打开 API：`src/lib/stores/fileEditor.ts:356` `fileEditorStore.openFile(path, opts)` → 经 `invoke('read_file_for_editor', {path})` 从磁盘读内容（`fileEditor.ts:391`、`:451`）。
- **markdown 默认 `viewMode:'preview'`**（`fileEditor.ts:479`）→ 打开即渲染视图、不进可编辑 Monaco buffer，**天然近似只读查看**。
- 硬只读目前**仅 diff tab**支持（`FileEditor.svelte:320 readOnly:true`、`:361`）；`OpenFile` 无通用 `readOnly` 标志。D5「只读打开」可走 markdown preview 形态满足 MVP；要硬只读需给 `OpenFile` 加 `readOnly` 标志并接 Monaco（小改）。
- **静态资源未打包 docs**：`tauri.conf.json:40-43` 仅打包 `ridge.theme`/`static/remote`/`web-remote-dist`。引导文档要随应用分发，需新增 resources 条目并用 `resolveResource` 取磁盘绝对路径才能喂 `openFile`。

### D. D1 编组持久化的现实约束（影响 key 设计）

- **运行时 `Workspace.id` 每次会话重新生成**：`state.rs:564` `let id = Uuid::new_v4()` → **不跨重启稳定**，不能作持久化 key。
- 稳定工作区身份 = `.ridge` 文件路径：`state.rs:119` `associated_file_path: Option<PathBuf>`；前端已有 `workspaceSaveInfo` 映射 `workspaceId→{file_path,name}`（`paneTree.ts:1753`）。
- 成员身份：pane 是每会话 Uuid（重启即不存在）；`teammate_agent_pane_map` 的 **agent_id 字符串**是相对稳定键（`state.rs:110`）。重启后所有成员必然失联 → 与 D1「失联保留占位并标灰」一致。
- 持久化基建：`settings.ts UserSettings`（localStorage）已有 `teammateEnabled`/`teammateHitlEnabled` 模式（`settings.ts:36-40,132-139`），编组可走同款 localStorage（前端、KISS）。

---

## 第二部分 · 目标方案（三块功能，标清复用 vs 新增）

### 功能 1 · MCP 自由交流打通（P1，地基）

**目标**：让两个已注册 teammate 分屏经内置 MCP 真实互发消息，并端到端验证。

**设计（最小必要修补，不新建并行栈，符合 D2）**：

1. **修缺口 1（寻址自洽）— 复用为主**：让 `mcp_tools_call` 的 `target_pane_id` **同时接受 Uuid 字符串与数字索引**（`server.rs:699` 处）：先尝试 `as_str()` 解析 Uuid 命中 `teammate_pane_uuid` 反查，失败再回退 `as_u64()` 索引。同时让花名册资源**补出数字 `paneIndex` 字段**（`commands/teammate.rs:topology_json` 与 `profiles.rs:topology_for` 各加一字段），两端任选其一都能寻址。这样 agent 读 `ridge://workspace/active-panes` 拿到的键就能直接回传。
2. **修缺口 3（最小扩工具）— 复用 registry**：`tools/call` 增路由 `ridge_get_team_profile`（只读，返回花名册，让 agent 先发现目标再发消息）。`split_pane`/`stash_data` 非「自由交流」核心，列入 P1 可选/延后。
3. **缺口 2（工作区寻址）— 据实标注 + 可选修**：MVP 保持「落活动工作区」（D7 零回归、常见单工作区够用）；在 `route_mcp_ws` 升级时读 `X-Ridge-Workspace` 头并随连接上下文传入 `mcp_tools_call` 为可选增强（不阻断 P1）。
4. **数据模型/接口**：无新数据结构；复用 `TeammateBinding`、`teammate_pane_uuid_at_index`（`commands/pane`）、`write_pty_bytes_workspace`。
5. **通信落点**：维持「写目标分屏 PTY stdin」语义（D2 复用现有通道）。

**复用**：MCP WS 框架、registry、resource、write_pty 全部既有。**新增**：仅寻址兼容逻辑 + 一个工具路由分支 + 花名册一个字段。

### 功能 2 · MCP 接入引导（文档 + 面板入口 + 复制连接信息，P2）

**目标**：用户经指挥部一键用内置编辑器只读打开接入引导；并能一键复制**当前真实** endpoint+token。

**设计**：

1. **引导文档（D5）— 复用既有 `docs/mcp-integration.md`**：它已覆盖端点/token 机制/工具/资源/Node 示例/限制，且**只讲 token 机制不含活 token**（天然满足 D6）。动作：① 校订使之与 P1 修复后的寻址/工具状态一致；② 复制一份为随应用分发的静态资源（如 `static/docs/mcp-integration.md`），在 `tauri.conf.json:40` resources 增条目。
2. **面板入口（D5）— AgentCenterPanel 新增按钮**：在 `AgentCenterPanel.svelte` header 加「MCP 接入引导」按钮 → 调 `resolveResource('static/docs/mcp-integration.md')` 取绝对路径 → `fileEditorStore.openFile(path)`。markdown 默认 preview 即只读查看；如需硬只读，给 `OpenFile` 加可选 `readOnly` 标志并在 `FileEditor.svelte` Monaco `updateOptions({readOnly})`（小改，可选）。
3. **复制连接信息（D6）— 新增后端命令 + 前端按钮**：
   - **新增** `#[tauri::command] get_teammate_connection_info()`：先 `ensure_teammate_started(&state)`，再读 `teammate_binding` 返回 `{ wsEndpoint: base_url.replace("http","ws")+"/api/v1/mcp/ws", token }`。注册进 `lib.rs` invoke_handler。**token 只在运行时动态返回，绝不写进静态文档（D6）**。
   - AgentCenterPanel 加「复制当前连接信息」按钮 → invoke 上述命令 → `writeText` 到剪贴板。binding 为 None（还没开过终端）时提示「先打开一个终端分屏」。

**复用**：`fileEditor.openFile`、`read_file_for_editor`、剪贴板 `writeText`、`ensure_teammate_started`、`TeammateBinding`。**新增**：1 个只读命令、1 份打包资源条目、面板 2 个按钮（+可选 `readOnly` 标志）。

### 功能 3 · 侧边栏手动编组协作（P3，依赖 P1）

**目标**：在指挥部勾选成员 → 命名建组（配色标签）→ 改名/解散 → 给组派任务（组内广播该任务并记录为「组任务」）。

**数据模型（前端，新增）**：
```ts
interface TeammateGroup {
  id: string;            // 本地生成
  name: string;
  color: string;         // 组配色标签（预设色板取值）
  memberAgentIds: string[]; // 用稳定 agent_id 引用（D1）
  createdAt: number;
}
```
- **持久化（D1）**：localStorage，key = `ridge-teammate-groups:<stableWorkspaceKey>`，`stableWorkspaceKey` = 该工作区 `.ridge` 文件路径（经 `workspaceSaveInfo` 由 runtime workspaceId 解析），未保存工作区回退会话内存。新建 `src/lib/teammate/teammateGroups.svelte.ts`（store + 持久化 + vitest）。
- **成员失联占位（D1）**：渲染时把 `memberAgentIds` 与当前 `topology.roster` 对齐；roster 里没有的成员 → 标记 `Disappeared` 状态、置灰、**保留**，提供手动「移除」按钮，不自动删。复用 `teammateModel.ts` 的 `TeammateStatus`/`statusDot`。

**前端 UI（D3 MVP，AgentCenterPanel 扩展）**：在「成员」区下方加「编组」区——勾选成员的多选态 → 「建组」弹名称+配色 → 组卡片（组名/配色条/成员列表/改名/解散/派任务）。**拖拽编组不进 MVP（D3）**。

**给组派任务（D4 MVP）— 复用现有 delegate 通道，新增可选 group scope**：
- 组卡片「派任务」输入框 → 对 `memberAgentIds` 中每个在线成员，解析 agent_id→pane→index，逐个调既有派活通道（`delegate-task` HTTP 或前端经 invoke 写 PTY），即**广播该任务消息**（D4）。
- **通信落点（D2）**：复用现有 delegate/send，给请求体加**可选 `group_id` 字段**做投递范围标注/审计，**不新建通信栈**。后端 `DelegateBody`（`server.rs:533`）加 `#[serde(default)] group_id: Option<String>` 仅透传记录。
- 记录为「组任务」：前端 store 存一条 `{groupId, objective, ts, targets}` 历史。**指定单一执行者 / Leader 竞选延后（D4）**。

**复用**：AgentCenterPanel、teammateModel、get_teammate_topology、delegate 通道、settings localStorage 模式。**新增**：teammateGroups store + 编组 UI 区 + delegate 请求体一个可选字段。

---

## 第三部分 · 分阶段任务拆解

> 验证总则：ridge-core/前端用 `cargo test -p ridge-core` / `pnpm check` / `vitest`（**不杀会话**）；后端接线用 `cargo check -p ridge`（编译验证）+ 标注「**待 rebuild + 真机 e2e**」。真机步骤写成可交接手册（见各阶段「交接验证」）。

### 阶段 P1 — 打通并验证 MCP 自由交流（地基，可独立交付）

**产出**：两个已注册 teammate 分屏经 MCP 稳定互发消息；寻址自洽；验证脚本。

| 步 | 动作 | 文件 | 依赖 | 风险 |
|---|---|---|---|---|
| 1.1 | `mcp_tools_call` 寻址兼容：`target_pane_id` 接受 Uuid 串或数字索引 | `src-tauri/src/teammate/server.rs:699-711` | 无 | 中（热点文件，按 hunk 隔离并发会话） |
| 1.2 | 花名册补 `paneIndex` 数字字段 | `commands/teammate.rs:topology_json:22`、`teammate/profiles.rs:topology_for:54` | 无 | 低 |
| 1.3 | `tools/call` 增路由 `ridge_get_team_profile`（只读返回花名册） | `server.rs:mcp_tools_call:685` | 1.2 | 低 |
| 1.4 | （可选）`route_mcp_ws` 读 `X-Ridge-Workspace` 头并传入 tools_call | `server.rs:625,685` | 无 | 中（保持默认回退 active，零回归 D7） |
| 1.5 | 单测：寻址兼容（Uuid/index）+ registry 路由覆盖 | `ridge-core` / server 内联测试 | 1.1-1.3 | 低 |
| 1.6 | 验证脚本：Node MCP 客户端在 A 分屏 `initialize→tools/list→get_team_profile→tools/call(send 给 B)`，断言 B 分屏收到文本 | `scripts/cdp-teammate-mcp-e2e.mjs`（参照既有 `cdp-teammate-e2e.mjs`） | 1.1-1.3 | 中 |

**交接验证（rebuild 后人工）**：① rebuild+重启 ridge；② 开两个终端分屏，各注册为 teammate（经 tmux/agent 启动）；③ 在 A 分屏跑 `node scripts/cdp-teammate-mcp-e2e.mjs`；④ 观察 B 分屏出现注入文本，且 `get_team_profile` 返回的 `paneIndex/paneId` 能被 `tools/call` 直接接受。或用 `pnpm tauri:dev:cdp` 独立实例自验（不杀托管会话）。

### 阶段 P2 — 引导文档 + 面板打开入口 + 复制连接信息（依赖无，可独立交付）

**产出**：指挥部「MCP 接入引导」按钮只读打开文档；「复制连接信息」按钮动态复制真实 endpoint+token。

| 步 | 动作 | 文件 | 依赖 | 风险 |
|---|---|---|---|---|
| 2.1 | 校订引导文档与 P1 后状态一致（寻址/已路由工具） | `docs/mcp-integration.md` | P1（理想，否则标注现状） | 低 |
| 2.2 | 文档复制为分发资源 + 打包条目 | `static/docs/mcp-integration.md`、`tauri.conf.json:40` resources | 无 | 低 |
| 2.3 | 新增只读命令 `get_teammate_connection_info`（ensure_started→返回 ws 端点+token） | `commands/teammate.rs`、注册 `lib.rs:806` 区 | 无 | 中（暴露 token：仅本机 IPC，勿入 REMOTE_ALLOWLIST/web-remote） |
| 2.4 | AgentCenterPanel 加「MCP 接入引导」按钮 → `resolveResource`+`openFile` | `AgentCenterPanel.svelte`、`fileEditor.ts:356` | 2.2 | 低 |
| 2.5 | AgentCenterPanel 加「复制连接信息」按钮 → invoke+`writeText` | `AgentCenterPanel.svelte` | 2.3 | 低 |
| 2.6 | （可选）`OpenFile.readOnly` 标志 + Monaco 接线（硬只读） | `fileEditor.ts:58`、`FileEditor.svelte:320` | 无 | 低 |

**验证**：2.1/2.4/2.6 前端 `pnpm check`+`vitest`+`tauri:dev:cdp` 看打开渲染；2.3/2.5 后端 `cargo check`，**交接**：rebuild 后点「复制连接信息」→ 粘贴确认是 `ws://127.0.0.1:<port>/api/v1/mcp/ws` + 真 token；binding 为 None 时（未开终端）提示文案正确。**安全自查（D6）**：确认静态文档 grep 无活 token、命令只走桌面 IPC。

### 阶段 P3 — 侧边栏手动编组协作（依赖 P1）

**产出**：勾选建组（配色/改名/解散）+ 按工作区持久化 + 失联占位标灰 + 给组派任务（广播+记录）。

| 步 | 动作 | 文件 | 依赖 | 风险 |
|---|---|---|---|---|
| 3.1 | 新建 `teammateGroups.svelte.ts`（模型+localStorage 持久化，key=稳定工作区键）+ vitest | `src/lib/teammate/teammateGroups.svelte.ts`(+`.test.ts`) | 无 | 低 |
| 3.2 | 解析稳定工作区键（runtime id→.ridge 路径，回退会话内存） | 复用 `paneTree.ts:1753` workspaceSaveInfo | 3.1 | 中（未保存工作区降级为会话级，文档说明） |
| 3.3 | AgentCenterPanel 加「编组」区：多选成员→建组→组卡片（改名/解散/派任务） | `AgentCenterPanel.svelte`、复用 `teammateModel.ts` | 3.1、P1 | 中（载体扩展，控制文件行数） |
| 3.4 | 失联占位：roster 对齐 memberAgentIds，缺失标 Disappeared 置灰+手动移除 | `AgentCenterPanel.svelte`、`teammateModel.ts:statusDot` | 3.3 | 低 |
| 3.5 | 给组派任务：对在线成员逐个走 delegate；请求体加可选 `group_id` | `AgentCenterPanel.svelte`；`server.rs:DelegateBody:533`（加 `group_id`） | P1、3.3 | 中（后端改动需 rebuild） |
| 3.6 | 「组任务」历史记录（前端 store） | `teammateGroups.svelte.ts` | 3.5 | 低 |

**验证**：3.1/3.3/3.4/3.6 `pnpm check`+`vitest`（建组/持久化/失联占位/重排序逻辑单测）+`tauri:dev:cdp` 看交互；3.5 后端 `cargo check`，**交接**：rebuild 后建组→给组派任务→确认组内每个在线成员分屏收到任务文本、组定义重启后仍在（同一 .ridge 工作区）、失联成员标灰可手动移除。

---

## 第四部分 · 开放决策点（已由主控按最佳实践拍板）

**D1–D9 逐条确认无冲突，按现状落地**，以下与代码现实的衔接点已拍板：

1. **D1 持久化 key**：运行时 `Workspace.id` 每会话重生（`state.rs:564`），改用 **`.ridge` 文件路径**作稳定键（经 `workspaceSaveInfo` 解析）。**未保存的临时工作区其编组仅会话级、重启丢** —— 【已拍板：接受此降级，文档注明】。
2. **D5 只读形态**：引导文档是 markdown，`openFile` 默认 preview 渲染即只读查看。【已拍板：MVP 就用 preview 形态（零新增）；硬 `readOnly` 标志（步 2.6）列为可选增强，不进 MVP】。
3. **缺口 2 工作区寻址**：MCP `tools/call` 当前落活动工作区。【已拍板：MVP 保持此行为（D7 零回归 + 常见单工作区够用），跨工作区寻址（步 1.4）列为 P1 可选、不阻断】。
4. **新增后端命令 `get_teammate_connection_info`（返回含 token）**：【已拍板：**仅桌面 IPC，明确不加入 `REMOTE_ALLOWLIST`/不暴露给 web-remote**（token 泄露面控制）】。
5. **无新的需征询项**：TML/广播/竞选不复活（D8 + 06-20 瘦身一致）；HITL/TML 净化默认开关不动（D7）。

---

### 关键文件清单（绝对路径）

- 后端 MCP/teammate：`C:\code\wind\src-tauri\src\teammate\server.rs`、`C:\code\wind\src-tauri\src\commands\teammate.rs`、`C:\code\wind\src-tauri\src\teammate\profiles.rs`、`C:\code\wind\src-tauri\src\commands\terminal.rs`、`C:\code\wind\src-tauri\src\state.rs`、`C:\code\wind\src-tauri\src\lib.rs`
- MCP 纯核心：`C:\code\wind\packages\ridge-core\src\mcp\{registry,resource,protocol}.rs`
- 前端载体：`C:\code\wind\src\lib\teammate\AgentCenterPanel.svelte`、`C:\code\wind\src\lib\teammate\teammateModel.ts`、`C:\code\wind\src\lib\teammate\teammateSettings.ts`、`C:\code\wind\src\routes\+page.svelte`、`C:\code\wind\src\lib\plugins\index.ts`
- 文件编辑器：`C:\code\wind\src\lib\stores\fileEditor.ts`、`C:\code\wind\src\lib\components\FileEditor.svelte`
- 资源打包：`C:\code\wind\src-tauri\tauri.conf.json`
- 引导文档（已存在）：`C:\code\wind\docs\mcp-integration.md`、用户手册 `C:\code\wind\docs\teammate-user-guide.md`
- 设计上游：`C:\code\wind\docs\superpowers\specs\2026-06-19-domain-zero-teammate-design.md`、`C:\code\wind\docs\superpowers\specs\2026-06-20-team-agent-upgrade-plan-design.md`
- 新增（规划）：`C:\code\wind\src\lib\teammate\teammateGroups.svelte.ts`、`C:\code\wind\static\docs\mcp-integration.md`、`C:\code\wind\scripts\cdp-teammate-mcp-e2e.mjs`
