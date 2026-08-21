---
name: ridge-mcp-dispatch
description: 教大模型使用 Ridge 终端内置的 MCP server 在多个终端 pane 之间调度、派发任务给 teammate agent。当你运行在 Ridge/rdg 里（环境变量含 RIDGE_TEAMMATE_URL / RIDGE_TEAMMATE_TOKEN），需要拆分 pane、把子任务委派给其它 pane 的 agent、跨 agent 传递大块中间产物、或读取工作区花名册/git 状态/编辑器上下文时使用。触发词：派活、委派、分屏、teammate、编组、ridge:// 资源、调度终端。
---

# Ridge 内置 MCP —— 终端内多 agent 调度

Ridge（桌面 app）与 `rdg`（无头 host）内建一个 **MCP server**，让运行在某个 pane 里的
你，去驱动**同一工作区其它 pane** 里的 teammate agent：拆分屏、派活、传数据、读上下文。
协议是 JSON-RPC 2.0，三条传输：**stdio（`rdg mcp`）/ HTTP（`POST /api/v1/mcp`）/ WebSocket**。
桌面与 rdg 是**同一份实现**（crate `ridge-mcp`），工具与资源完全对等。

## 1. 我在 Ridge 里吗？怎么连？

只有当下面两个环境变量都存在时，本 MCP 才可用（Ridge PTY 注入，子进程继承）：

```
RIDGE_TEAMMATE_URL    # 例 http://127.0.0.1:52731  —— HTTP base，端口是 ephemeral
RIDGE_TEAMMATE_TOKEN  # 鉴权 token
RIDGE_WORKSPACE_ID    # 发起方工作区 UUID（HTTP 放置路由需要，作 X-Ridge-Workspace 头）
```

**端点**（任选其一）：

```
POST  http://127.0.0.1:<port>/api/v1/mcp      # 一发一收 JSON-RPC，最省事
ws://127.0.0.1:<port>/api/v1/mcp/ws           # 长连
rdg mcp                                        # stdio 桥，端点/token 自动发现，端口漂移自愈
```

给别的 MCP 客户端接入直接用：`claude mcp add ridge -- rdg mcp`。

**鉴权**：升级请求带头 `x-ridge-token: <RIDGE_TEAMMATE_TOKEN>`（或 `Authorization: Bearer <token>`）。缺/错 → 401。

> 端点会漂移：后端 panic 自重启会换端口。env 里的值可能过期。权威副本落在 sidecar 文件
> `TMPDIR/ridge-teammate-endpoint-<sanitized>.json`（`{"url","token"}`），其中 `<sanitized>` =
> `$TMUX` 第一段路径里非字母数字全替换成 `_`。连不上先读这个文件刷新 url/token。

## 2. 协议方法（JSON-RPC 2.0）

| method | 说明 |
|---|---|
| `initialize` | 握手，返回 serverInfo/capabilities。可选，直接调工具也行。 |
| `tools/list` | 列出全部工具规格。 |
| `tools/call` | `params: { "name": <tool>, "arguments": {...} }` 调工具。 |
| `resources/list` · `resources/templates/list` | 列可读资源。 |
| `resources/read` | `params: { "uri": "ridge://..." }` 读资源。 |
| `ping` | 心跳。通知（无 `id`）**不会有响应**（HTTP 回 202）。 |

请求形如 `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{...}}`；一发一收。

## 3. 九个工具（`tools/call` 的 name）——全部已路由

| name | arguments | 作用 / 语义 |
|---|---|---|
| `ridge_get_team_profile` | `{}` | **先调它**。返回花名册快照（每个 teammate pane 的身份、状态，同时给 `paneId`(UUID) 和 `paneIndex`(数字)）。派活前先发现目标。 |
| `ridge_split_pane` | `{direction:"horizontal"\|"vertical", role, initial_cmd?}` | 在工作区分出新 pane，`role` 是角色标识（如 `worker`/`reviewer`），`initial_cmd` 可选，创建后立即执行。 |
| `ridge_send_to_teammate` | `{agent_id \| target_pane_id, message, submit?}` | 按 fenced Agent 或 pane 投递 Hub；默认 `submit=true`，显式 `false` 才保留草稿；不保证 PTY/stdin 写入。 |
| `ridge_send_and_submit` | `{agent_id \| target_pane_id, message}` | 同一 Hub 入口，强制 `submit=true`；返回 `deliveryId`，不代表 PTY/stdin 已写入。 |
| `ridge_delegate_task` | `{agent_id \| target_pane_id, objective, max_steps?}` | 委派一个多步任务并排入 Hub；Fire-and-forget，回执不代表 Agent 已执行。 |
| `ridge_stash_data` | `{data}` | 把一段**纯文本**存进内存中转站，返回 `ridge://cache/<id>`，供另一个 agent `resources/read` 回读。用于跨 agent 传大块中间产物。FIFO 淘汰（默认 64 条 / 32 MiB）。 |
| `ridge_join_group` | `{group_name, agent_id? \| target_pane_id?}` | 把某成员加入按名字寻址的已有编组（`agent_id` 与 `target_pane_id` 二选一）。 |

**`target_pane_id` 寻址**：既接受花名册回传的 `paneId`（UUID 字符串），也接受 `paneIndex`（叶子数字索引，或其字符串）。UUID 会先校验它仍是当前活动工作区的叶子 pane。

**Agent 寻址与安全边界**：Hub/通信工具的目标至少提供 `agent_id` 或 `target_pane_id`；两者并存时必须指向同一份 Kernel roster identity。显式 `workspace_id`/`generation`/`lease` 必须与该 identity 相等；identity 本身须具备 workspace、generation、lease、online、lifecycle、capabilities，否则 fail closed。canonical Hub key 同时含 workspace、Agent、generation、lease，故两种 selector 共用收件箱、回执与幂等域。agent-only 不调用 host pane resolve/pane_key/probe，也不走 PTY；仅 MCP pull、已注册 Runtime 或 A2A。PID/进程名只能作发现证据，不授予 PTY、stdin 或控制台注入权限。静态 tmux roster 若无完整 fence，不宣称 `agent_id` 可寻址；`ridge_capture_pane`、`ridge_report_progress` 等 pane 操作仍仅接受 `target_pane_id`。

此外三个（跨 agent 协作用）：

| name | arguments | 作用 / 语义 |
|---|---|---|
| `ridge_capture_pane` | `{target_pane_id, lines?}` | 抓该 pane **渲染后**的屏幕文本（默认 80 行）。监控队友进展就用它，不要去读原始 scrollback。 |
| `ridge_inbox_read` | `{agent_id \| target_pane_id, peek?}` | 取走投递给该 fenced Agent/pane 的消息（`ridge_send_to_teammate` 会自动留副本）。异构 agent 的异步回话通道；取走即清空，`peek:true` 只看。 |
| `ridge_report_progress` | `{target_pane_id, status, detail?}` | 主动汇报进展，桌面落前端进度事件（无头 host 回 `isError`）。 |

> 错误语义：工具名不存在 → `-32601`；**宿主不支持该能力**（如无头 host 无编组）→ 正常 result 带
> `isError: true`，据此改道即可，不必当协议崩坏。

## 4. 四个资源（`resources/read` 的 uri，`resources/list` 可发现）

| uri | 内容 |
|---|---|
| `ridge://workspace/active-panes` | 当前工作区花名册（同 `ridge_get_team_profile`，含 paneId+paneIndex）。 |
| `ridge://workspace/git-status` | 各 pane 所在仓库根 + 分支（只读 `.git/HEAD`，**不 spawn git**）。 |
| `ridge://workspace/editor-context` | 各 pane 的标题 / cwd / 忙闲。 |
| `ridge://cache/<id>` | 回读 `ridge_stash_data` 存入的 blob。 |

## 5. 典型派发流程

1. `resources/read ridge://workspace/active-panes`（或 `ridge_get_team_profile`）—— 看有哪些 pane、谁空闲。
2. 没有合适的空 pane → `ridge_split_pane {direction, role}` 开一个。
3. `ridge_delegate_task {target_pane_id: <paneId 或 index>, objective}` 把子任务派下去（pane 转 Busy）。
4. 需要给多个 agent 共享大段上下文 → `ridge_stash_data {data}` 拿 `ridge://cache/<id>`，把 URI 放进 objective 里让对方 `resources/read` 回读，避免把大块内容塞进消息。
5. 派发是 fire-and-forget：**不要**卡住等返回。要看进展就轮询 `ridge_capture_pane`（渲染后屏幕）
   或花名册状态；对方也可主动 `ridge_report_progress`，留言走 `ridge_inbox_read` 取。
6. 服务端**没有**主动推送（`notifications/progress` 未实现）——别等通知，自己轮询。

## 6. 不想开 WS？HTTP 也有一份（curl 友好）

同一 server 暴露等价 HTTP 路由，头带 `x-ridge-token: $RIDGE_TEAMMATE_TOKEN` +
`X-Ridge-Workspace: $RIDGE_WORKSPACE_ID`：

```bash
# 花名册
curl -s "$RIDGE_TEAMMATE_URL/api/v1/team-profile" \
  -H "x-ridge-token: $RIDGE_TEAMMATE_TOKEN" -H "X-Ridge-Workspace: $RIDGE_WORKSPACE_ID"

# 派活（返回 TaskTicket：{task_id, assigned_pane, status:"dispatched"}）
curl -s "$RIDGE_TEAMMATE_URL/api/v1/delegate-task" \
  -H "x-ridge-token: $RIDGE_TEAMMATE_TOKEN" -H "X-Ridge-Workspace: $RIDGE_WORKSPACE_ID" \
  -H 'content-type: application/json' \
  -d '{"target_pane":1,"objective":"跑测试并汇报"}'
```

其它 HTTP 路由：`/api/v1/split-window`、`/api/v1/send-keys`、`/api/v1/list-panes`、
`/api/v1/report-progress` 等（前缀同上）。WS(MCP) 与 HTTP 落到同一套工作区状态。

## 7. 纪律

- **先发现，再派活**：不确定目标 pane 时先读花名册，别猜 index。
- **一个工作区内寻址**：跨工作区调用会被 fail-closed 拒（缺/错 `X-Ridge-Workspace` → 400）。
- **别锁焦点**：派发是异步的，发完即走，不要同步等 teammate 结果。
- **大块数据走 stash**，消息体只放 `ridge://cache/<id>` 引用。
