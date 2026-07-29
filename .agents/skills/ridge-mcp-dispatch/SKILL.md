---
name: ridge-mcp-dispatch
description: 教大模型使用 Ridge 终端内置的 MCP server 在多个终端 pane 之间调度、派发任务给 teammate agent。当你运行在 Ridge/rdg 里（环境变量含 RIDGE_TEAMMATE_URL / RIDGE_TEAMMATE_TOKEN），需要拆分 pane、把子任务委派给其它 pane 的 agent、跨 agent 传递大块中间产物、或读取工作区花名册/git 状态/编辑器上下文时使用。触发词：派活、委派、分屏、teammate、编组、ridge:// 资源、调度终端。
---

# Ridge 内置 MCP —— 终端内多 agent 调度

Ridge 桌面 app 与 `rdg` 无头 host **各自**内建 MCP server，协议实现同源；二者是不同应用。
在 Ridge desktop pane 内，server 已由 Ridge 启动，勿为此安装或优先调用 `rdg`。它让 pane 内的 agent
驱动**同一工作区其它 pane**：拆分屏、派活、传数据、读上下文。协议是 JSON-RPC 2.0 over HTTP/WS。

## 1. 我在 Ridge 里吗？怎么连？

桌面 Ridge 会把下列变量注入其 pane 的子进程；它们存在即表示可直连内置 MCP：

```
RIDGE_TEAMMATE_URL    # 例 http://127.0.0.1:52731  —— HTTP base，端口是 ephemeral
RIDGE_TEAMMATE_TOKEN  # 鉴权 token
RIDGE_WORKSPACE_ID    # 发起方工作区 UUID（HTTP 放置路由需要，作 X-Ridge-Workspace 头）
```

**MCP WebSocket 端点** = `RIDGE_TEAMMATE_URL` 把 `http` 换成 `ws`，再接 `/api/v1/mcp/ws`：

```
ws://127.0.0.1:<port>/api/v1/mcp/ws
```

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
| `resources/read` | `params: { "uri": "ridge://..." }` 读资源。 |

请求形如 `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{...}}`；一发一收。

## 3. 投递与协作工具（`tools/call` 的 name）

| name | arguments | 作用 / 语义 |
|---|---|---|
| `ridge_get_team_profile` | `{}` | **先调它**。返回花名册快照（每个 teammate pane 的身份、状态，同时给 `paneId`(UUID) 和 `paneIndex`(数字)）。派活前先发现目标。 |
| `ridge_split_pane` | `{direction:"horizontal"\|"vertical", role, initial_cmd?}` | 在工作区分出新 pane，`role` 是角色标识（如 `worker`/`reviewer`），`initial_cmd` 可选，创建后立即执行。 |
| `ridge_send_to_teammate` | `{target_pane_id, message}` | 写入目标 pane 草稿；**不**发送 Enter。仅返回 `draft_injected`。 |
| `ridge_send_and_submit` | `{target_pane_id, message}` | 写入后显式派发 Enter；仅返回 `submit_dispatched`。 |
| `ridge_delegate_task` | `{target_pane_id, objective, max_steps?}` | 委派并显式提交任务、标记 **Busy**；返回回执，不称 `delivered`。 |
| `ridge_delivery_status` | `{target_pane_id, receipt_id}` | 查询回执：`submit_dispatched`、`terminalAccepted`、`agentAcknowledged` 分列。 |
| `ridge_acknowledge_receipt` | `{target_pane_id, receipt_id, status, detail?}` | 目标 agent 明确确认/拒绝；唯此可置 `agentAcknowledged=true`。 |
| `ridge_report_execution_rejection` | `{executor, policy_source, request_id, reason, next_step, action?}` | 外部执行网关拒绝时上报 Ridge 桌面卡；必须如实归因，不能说成 Ridge 已拦截或可重试。 |
| `ridge_stash_data` | `{data}` | 把一段**纯文本**存进内存中转站，返回 `ridge://cache/<id>`，供另一个 agent `resources/read` 回读。用于跨 agent 传大块中间产物。FIFO 淘汰（默认 64 条 / 32 MiB）。 |
| `ridge_join_group` | `{group_name, agent_id? \| target_pane_id?}` | 把某成员加入按名字寻址的已有编组（`agent_id` 与 `target_pane_id` 二选一）。 |

**`target_pane_id` 寻址**：既接受花名册回传的 `paneId`（UUID 字符串），也接受 `paneIndex`（叶子数字索引，或其字符串）。UUID 会先校验它仍是当前活动工作区的叶子 pane。

**回执纪律**：`draft_injected` 不表示按过 Enter；`submit_dispatched` 不表示终端已收；
`terminalAccepted` 不表示 shell/Agent 已消费。只有 `ridge_acknowledge_receipt` 才能声明 Agent 确认。

> 已知偏差：`ridge_stash_data` 的 `tools/list` 规格里字段名写作 `content_base64`，但服务端实际
> 读的是 `data`（纯文本，非 base64）。**按 `data` 传**，别按规格里的 `content_base64`。

## 4. 四个资源（`resources/read` 的 uri）

| uri | 内容 |
|---|---|
| `ridge://workspace/active-panes` | 当前工作区花名册（同 `ridge_get_team_profile`，含 paneId+paneIndex）。 |
| `ridge://workspace/git-status` | 工作区 git 状态。 |
| `ridge://workspace/editor-context` | 当前编辑器上下文（打开的文件等）。 |
| `ridge://cache/<id>` | 回读 `ridge_stash_data` 存入的 blob。 |

## 5. 典型派发流程

1. `resources/read ridge://workspace/active-panes`（或 `ridge_get_team_profile`）—— 看有哪些 pane、谁空闲。
2. 没有合适的空 pane → `ridge_split_pane {direction, role}` 开一个。
3. `ridge_delegate_task {target_pane_id: <paneId 或 index>, objective}` 把子任务派下去（pane 转 Busy）；保存其 `receiptId`，需证实投递时再查 `ridge_delivery_status`。
4. 需要给多个 agent 共享大段上下文 → `ridge_stash_data {data}` 拿 `ridge://cache/<id>`，把 URI 放进 objective 里让对方 `resources/read` 回读，避免把大块内容塞进消息。
5. 派发后不要把回执误述为「已执行」；继续做别的，或轮询花名册/回执。对方若消费该消息，应调用 `ridge_acknowledge_receipt`。

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
