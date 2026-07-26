# Ridge 内置 MCP Server（Agent's Commune）· 接入文档

> 适用版本：≥ v0.1.4。桌面 Ridge 与无头 `rdg` **同一份实现**（crate `ridge-mcp`），能力对等。

---

## 它解决什么

Ridge 自带一个端侧 **MCP（Model Context Protocol）server**，把一个终端工作区变成**跨 agent 的协作总线**：
接进来的可以是 Claude Code、Cursor、或你自己写的 MCP 客户端——它们彼此没有共同的内部协议，
但都能在同一工作区里**发现同伴、派活、观察进展、异步回话、传大块中间产物**。

- **协议**：JSON-RPC 2.0（MCP `2024-11-05`）
- **传输**：stdio（经 `rdg mcp`）/ HTTP（`POST /api/v1/mcp`）/ WebSocket（`/api/v1/mcp/ws`）
- **鉴权**：`Authorization: Bearer <token>` 或 `x-ridge-token: <token>`
- **宿主**：桌面 Ridge、无头 `rdg tmux`（同一 crate，同一套工具与资源）

---

## 1. 接入（三选一）

### A. stdio —— 推荐，随安装即用

```bash
claude mcp add ridge -- rdg mcp
```

`rdg mcp` 自动发现本机端点与 token（`RIDGE_TEAMMATE_URL/_TOKEN` → 临时目录 sidecar），
后端重启换端口也会自愈。桌面 Ridge 与 `rdg tmux` 都能连。显式指定：
`rdg mcp --url http://127.0.0.1:PORT --token <tok>`。

> 为什么不建议写死 URL：端口是 ephemeral、token 每次启动重随机，静态配置隔天即失效。

### B. HTTP

```bash
claude mcp add --transport http ridge "$RIDGE_TEAMMATE_URL/api/v1/mcp" \
  --header "Authorization: Bearer $RIDGE_TEAMMATE_TOKEN"
```

一发一收的 JSON-RPC；通知（无 `id`）回 `202 Accepted` 空体。

### C. WebSocket

`ws://<host>/api/v1/mcp/ws`，升级请求带 `Authorization: Bearer <token>`（或 `x-ridge-token`）。

### 端点与 token 从哪来

| 来源 | 说明 |
| --- | --- |
| `RIDGE_TEAMMATE_URL` / `RIDGE_TEAMMATE_TOKEN` | Ridge 注入进每个 teammate 分屏的 env，子进程直接继承 |
| `TMPDIR/ridge-teammate-endpoint-*.json` | sidecar（`{"url","token"}`）；后端重启换端口后由宿主刷新 |
| `rdg tmux` 启动日志 | 无头 host 把 `RIDGE_TEAMMATE_*` 导出行打到 stderr |

---

## 2. 方法

| method | 说明 |
| --- | --- |
| `initialize` | 握手，返回 serverInfo / capabilities / instructions |
| `ping` | 心跳 |
| `tools/list` · `tools/call` | 工具发现与调用 |
| `resources/list` · `resources/templates/list` · `resources/read` | 资源发现与读取 |
| `notifications/*` | 通知**无响应**（符合 JSON-RPC 2.0，不再回伪错误） |

---

## 3. 工具（9 个，全部可调）

| 工具 | 参数 | 用途 |
| --- | --- | --- |
| `ridge_get_team_profile` | 无 | **先调它**。花名册：成员 `paneId` + `paneIndex` + 状态 + 编组 |
| `ridge_send_to_teammate` | `target_pane_id, message, from?` | 向该 pane 注入文本，并留一份到收件箱 |
| `ridge_delegate_task` | `target_pane_id, objective, max_steps?` | 派活：注入任务 + 目标标记「工作中」 |
| `ridge_capture_pane` | `target_pane_id, lines?` | 抓该 pane **渲染后**的屏幕文本（监控队友进展，不是转义序列堆） |
| `ridge_inbox_read` | `target_pane_id, peek?` | 取走投递给该 pane 的消息（跨 agent 异步回话通道） |
| `ridge_report_progress` | `target_pane_id, status, detail?` | 回流一条进展（桌面落前端进度事件） |
| `ridge_split_pane` | `direction, role, initial_cmd?` | 开一个新 pane 给队友；`role` 落成 pane 标题 |
| `ridge_join_group` | `group_name, agent_id? \| target_pane_id?` | 加入按名字寻址的已有编组（桌面前端 SSOT，fire-and-forget） |
| `ridge_stash_data` | `data` | 存文本，返回 `ridge://cache/<id>`，供别的 agent 回读 |

**寻址**：`target_pane_id` 同时接受花名册回传的 `paneId`（桌面是 Uuid 串，无头是 `%N`）与
`paneIndex`（数字）。越界/失效目标返回 `-32602`，绝不静默落到 0 号分屏。

**错误语义**：工具名不存在 → JSON-RPC `-32601`；宿主不提供该能力（如无头 host 无前端编组）→
正常 result 带 `isError: true`，客户端可让模型自行改道。

---

## 4. 资源

| URI | 内容 |
| --- | --- |
| `ridge://workspace/active-panes` | 花名册（同 `ridge_get_team_profile`） |
| `ridge://workspace/git-status` | 各 pane 所在仓库根 + 当前分支。**只读 `.git/HEAD`，不 spawn git**（见 CLAUDE.md「git 进程风暴」教训） |
| `ridge://workspace/editor-context` | 各 pane 的标题 / cwd / 忙闲 |
| `ridge://cache/<id>` | 回读 `ridge_stash_data` 存入的内容 |

---

## 5. 典型协作流

1. `ridge_get_team_profile` 看有谁、谁空闲。
2. 没有合适的 pane → `ridge_split_pane { direction, role }` 开一个。
3. `ridge_delegate_task { target_pane_id, objective }` 派活（fire-and-forget，别同步等）。
4. 大块上下文走 `ridge_stash_data` 拿 `ridge://cache/<id>`，把 URI 写进 objective 让对方 `resources/read`。
5. 想知道干得怎么样 → `ridge_capture_pane` 抓屏；对方可用 `ridge_report_progress` 主动汇报。
6. 收对方留言 → `ridge_inbox_read`（取走即清空；`peek: true` 只看不取）。

---

## 6. 当前限制（诚实说明）

- **服务端主动推送（`notifications/progress`）仍未实现**：当前是请求-响应循环。要「进展更新」请轮询
  `ridge_capture_pane` / `ridge_get_team_profile`，或让 worker 调 `ridge_report_progress`、leader 读收件箱。
- 所有动作落在**当前活动工作区**（桌面）/ **default socket**（无头），暂不支持跨工作区寻址。
- `ridge_join_group` 一次写入·fire-and-forget：编组数据在前端 localStorage，后端无法确认是否落地；
  无头 host 直接返回 `isError`（无前端）。
- 收件箱是**进程内内存**（每 pane 200 条 FIFO），宿主重启即清空；Stash 同理（64 条 / 32 MiB）。
- 无头 host 的 `ridge_split_pane` 忽略 `direction`（引擎里 pane 是列表，没有几何方向）。

---

## 7. 最小客户端示例（Node，HTTP 传输）

```js
// 跑在 Ridge 分屏里：读环境变量拿端点 + 令牌
const base = process.env.RIDGE_TEAMMATE_URL;      // http://127.0.0.1:<port>
const token = process.env.RIDGE_TEAMMATE_TOKEN;
const call = async (method, params = {}) => {
  const r = await fetch(`${base}/api/v1/mcp`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', 'x-ridge-token': token },
    body: JSON.stringify({ jsonrpc: '2.0', id: Date.now(), method, params }),
  });
  return r.status === 202 ? null : r.json();
};

const profile = await call('tools/call', { name: 'ridge_get_team_profile', arguments: {} });
const roster = JSON.parse(profile.result.content[0].text).roster;
const target = roster[0].paneId;             // 或 roster[0].paneIndex（数字）
await call('tools/call', {
  name: 'ridge_delegate_task',
  arguments: { target_pane_id: target, objective: '跑单元测试' },
});
const screen = await call('tools/call', {
  name: 'ridge_capture_pane',
  arguments: { target_pane_id: target, lines: 40 },
});
console.log(screen.result.content[0].text);
```

---

*实现：`packages/ridge-mcp/`（协议 + 工具 + 传输，两端唯一一份）、
`src-tauri/src/teammate/mcp.rs`（桌面宿主实装）、`packages/ridge-tmux/src/mcp.rs`（无头宿主实装）、
`packages/ridge-cli/src/mcp_stdio.rs`（`rdg mcp` stdio 桥）。用户手册：`docs/teammate-user-guide.md`。*
