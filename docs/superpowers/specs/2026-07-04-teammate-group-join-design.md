# 编组「加成员」+ Agent 自助拉入 设计稿

> 日期：2026-07-04 · 状态：实现中
> 关联：`2026-06-30-agent-collab-enhance-design.md`（功能3 编组 / P3）、`2026-06-19-domain-zero-teammate-design.md`（Domain C MCP）、`docs/mcp-integration.md`

## 1. 背景与问题

当前「编组（TeammateGroup）」是**纯前端 localStorage** 模型（`src/lib/teammate/teammateGroups.svelte.ts`），成员用稳定 `agent_id` 引用。已有能力：建组（建时选成员）/ 改名 / 解散 / 移除失联成员 / 给组派任务（广播）。

**两个缺口**：

1. **没有「向已有组追加成员」**——store 只有 `create/rename/dissolve/removeMember/recordTask`，`TeammateGroupsSection.svelte` 也只在建组时选成员、之后只能移除。想把一个新唤起的 agent 拉进已有组 → 做不到（只能解散重建）。
2. **没有任何后端 / MCP 通道触达编组**——编组数据只在浏览器里。跑在分屏里的 agent（经 MCP）**零程序化路径**把自己或别人加进组。

目标：让一个 agent 能经 MCP 把某个 teammate（按 `agent_id`）拉进某个**按名字寻址**的已有编组，同时补齐前端「人工加成员」UI。

## 2. 架构约束

- 编组是前端 SSOT（localStorage，键=工作区 `.ridge` 路径）。后端**不持有**编组数据，因此后端 MCP 工具无法同步确认「组是否存在 / 加成功」。
- 后端→前端唯一可用桥是 **Tauri 事件**（`app_handle.emit`），与既有 `teammate://circuit-tripped`、`teammate-layout-changed` 同构。
- 安装版下任何前端/后端改动都需**重建 app** 才生效；后端改动还需重编 Rust。本次实现只做到「代码 + 单测通过」，真机需用户重建。

## 3. 方案（全链路，一次写入·事件桥）

```
agent ──MCP tools/call ridge_join_group{group_name, agent_id}──▶ 后端(server.rs)
        校验 agent_id ∈ 当前工作区花名册(profiles)
        emit "teammate://group-add-member" {workspaceId, groupName, agentId}
                                   │
                                   ▼
        前端 AgentCenterPanel 监听 ──▶ teammateGroupStore.addMemberByGroupName(groupName, agentId)
                                       findGroupByName → addMemberIn（去重、不可变）→ 落 localStorage
                                       组卡片即时出现该成员
```

**寻址**：编组按 `name` 寻址（后端不知道前端的 group id）。同名取首个匹配（MVP 限制，见 §6）。

**成员**：用 `agent_id`（花名册 `id` 字段）。agent 先 `ridge_get_team_profile` 拿 roster，取目标成员的 `id` 作 `agent_id`。工具也接受 `target_pane_id`（Uuid/数字）→ 后端经 `profiles::agent_id_for_pane` 反查 `agent_id`，二者二选一。

## 4. 改动清单

### 后端
- `packages/ridge-core/src/mcp/registry.rs`：默认注册表新增第 6 个工具 `ridge_join_group`（`group_name` 必填；`agent_id` / `target_pane_id` 二选一）。更新计数类单测 5→6 + 新增必填字段断言 + 把它纳入 `routed_tools_are_advertised`。
- `src-tauri/src/teammate/profiles.rs`：新增 `contains_agent(wid, agent_id) -> bool`、`agent_id_for_pane(wid, pane_uuid) -> Option<String>`（纯查表，附单测）。
- `src-tauri/src/teammate/layout_event.rs`：新增事件名常量 `TEAMMATE_GROUP_ADD_MEMBER = "teammate://group-add-member"`（附断言）。
- `src-tauri/src/teammate/server.rs`：`mcp_tools_call` 增 `ridge_join_group` 分支——解析参数 → 校验 agent 存在（否则 `-32602`）→ emit 事件 → 返回 `{content:[{text:"dispatched"}]}`。

### 前端
- `src/lib/teammate/teammateGroups.svelte.ts`：纯函数 `addMemberIn`、`findGroupByName`、`parseGroupAddMember`（事件载荷防御式解析）；store 方法 `addMember`、`addMemberByGroupName`。
- `src/lib/teammate/teammateGroups.test.ts`：为上述纯函数补 AAA 单测（追加/去重/空白/组不存在/同名首命中/载荷解析）。
- `src/lib/teammate/TeammateGroupsSection.svelte`：组卡片新增「＋加成员」——展开列出 roster 中**未入组**的在线成员，点选即 `store.addMember`。
- `src/lib/teammate/AgentCenterPanel.svelte`：`onMount` 监听 `teammate://group-add-member`，落 `addMemberByGroupName`（先 `setWorkspace` 保证键正确），成功后可轻提示。

### 文档
- `docs/mcp-integration.md` + `static/docs/mcp-integration.md`：工具表补 `ridge_join_group`，§6 限制同步更新。

## 5. 测试
- `pnpm vitest run src/lib/teammate/teammateGroups.test.ts`（纯函数全绿）。
- `cargo test -p ridge-core mcp::registry`（注册表断言）+ `cargo check -p ridge`（后端编译）。
- 事件桥 + UI 需真机，本次不覆盖（重建后人工验）。

## 6. 已知限制（诚实说明）
- **组名寻址、同名取首个**：编组 id 在前端，后端只能透传名字。
- **一次写入·无同步确认**：事件 fire-and-forget，后端返回 `dispatched` 不代表已加入（组不存在则前端静默 no-op）。agent 想确认需人工看 UI（后续可加 `ridge://workspace/groups` 只读资源 + 前端→后端同步，本期不做）。
- **仅当前活动工作区 / 面板挂载时**：监听在 AgentCenterPanel；面板未挂载时事件丢失（可接受，加成员是用户主动看着面板做的场景）。
- **落焦点工作区**：后端 `ridge_join_group` 用 MCP 活动工作区（`active_workspace`）寻址并 emit，前端只在 `payload.workspaceId === 焦点 workspaceId` 时落地。两者短暂不同步时事件被丢弃——现**打 `console.warn` 不再静默**（评审 HIGH1）。跨工作区加成员不在本期范围。
- **早期 `.ridge` 路径竞态**：`workspaceSaveInfoStore` 异步填充；若事件在其就绪前到达，`setWorkspace` 会落到 `session:<id>` 桥而非 `file:<path>` 桥，`findGroupByName` 搜错桶→no-op（现打 `console.warn`，评审 MEDIUM3）。常态下用户已在看面板、路径早就绪，可接受。
- **失败可观测但无 UI 反馈**：`addMemberByGroupName` 返回 false（组名不存在/大小写/空白/搜错桶）时 `console.warn`，暂无 toast（评审 HIGH2；后续可加轻提示）。
- 安装版需**重建 app** 才生效。
