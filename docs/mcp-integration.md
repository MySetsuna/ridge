# Ridge 桌面内置 MCP Server（Agent's Commune）· 接入文档

> 适用版本：≥ v0.1.4。**Ridge 桌面版已经内置 MCP server，无须安装 `rdg`。** `rdg` 是另一款无头应用；仅在它自身作为 host 运行时，才按本文的「无头 rdg」章节接入。

---

## 它解决什么

Ridge 自带一个端侧 **MCP（Model Context Protocol）server**，把一个终端工作区变成**跨 agent 的协作总线**：
接进来的可以是 Codex CLI、Claude Code、Gemini CLI、Cursor、OpenCode、Aider（经兼容 bridge），或你自己写的 MCP 客户端——它们彼此没有共同的内部协议，
但都能在同一工作区里**发现同伴、派活、观察进展、异步回话、传大块中间产物**。

- **协议**：JSON-RPC 2.0（MCP `2024-11-05`）
- **传输**：桌面 Ridge 优先 HTTP（`POST /api/v1/mcp`）或 WebSocket（`/api/v1/mcp/ws`）；stdio 客户端使用独立 `ridge-mcp` companion
- **鉴权**：`Authorization: Bearer <token>` 或 `x-ridge-token: <token>`
- **宿主**：桌面 Ridge；无头 `rdg tmux` 是独立 host，复用协议实现但不构成桌面 Ridge 的安装依赖

---

## 1. 安装与接入

### A. 桌面 Ridge（首选；不安装 `rdg`）

打开 Ridge 后，在要运行 Agent 的 **Ridge pane** 内启动 MCP 客户端。Ridge 会为该 pane 的子进程注入：
`RIDGE_TEAMMATE_URL`、`RIDGE_TEAMMATE_TOKEN`、`RIDGE_WORKSPACE_ID`；MCP server 已随桌面后端启动。
客户端使用下列 HTTP 或 WebSocket 端点即可，无须另起服务、无须写死端口或 token。

> 端口与 token 都是临时值。客户端若在 Ridge pane 外启动，既拿不到这些变量，也不应把 token 复制到配置文件或聊天记录。

### B. 桌面 Ridge · HTTP

适合每次都在 Ridge pane 内启动、并在运行时读取环境变量的客户端。以下仅是临时直连探针；**不要**把展开后的 URL 或 token 写入持久 MCP 配置：

```bash
curl -sS -X POST "$RIDGE_TEAMMATE_URL/api/v1/mcp" \
  -H "Authorization: Bearer $RIDGE_TEAMMATE_TOKEN" \
  -H "content-type: application/json" \
  --data '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}'
```

一发一收的 JSON-RPC；通知（无 `id`）回 `202 Accepted` 空体。

### C. 桌面 Ridge · WebSocket

`ws://<host>/api/v1/mcp/ws`，升级请求带 `Authorization: Bearer <token>`（或 `x-ridge-token`）。

### D. 持久 MCP 配置（任意 MCP 客户端；Ridge companion）

持久 MCP 配置中的 HTTP URL 通常是静态值，而 Ridge 本机端点会漂移。`ridge-mcp` 是只做端点发现与 stdio↔HTTP 转发的 Ridge companion，**不是 `rdg`**：

桌面安装包已捆绑同版本 companion；普通用户无须源码、Rust 或 Cargo。在 Ridge 安装目录运行：

```bash
ridge-mcp --print-config
codex mcp add ridge -- /absolute/path/to/ridge-mcp
claude mcp add ridge -- /absolute/path/to/ridge-mcp
# Gemini CLI、OpenCode、Cursor 等：使用下列通用 stdio 配置
```

```json
{
  "mcpServers": {
    "ridge": { "command": "/absolute/path/to/ridge-mcp", "args": [] }
  }
}
```

`--print-config` 输出可粘贴的 `mcpServers.ridge` stdio 配置，命令为当前 companion 的绝对路径；
不含端口或 token。Windows 为 `ridge-mcp.exe`，Linux/macOS 为 `ridge-mcp`。升级由安装器原位替换，
卸载随 Ridge 一并清理。

仅源码开发者使用：

```bash
cargo install --path packages/ridge-mcp-bridge --bin ridge-mcp
codex mcp add ridge -- ridge-mcp
# Claude Code 同理：claude mcp add ridge -- ridge-mcp
```

server 不读取 `clientInfo.name` 作路由或授权决策；上述客户端获得同一 tools/resources 契约。没有 MCP
能力的 CLI 须使用其自身 bridge，Ridge 不冒充其私有 runtime API。

companion 默认按 **`%LOCALAPPDATA%/ridge/kernel.json`（独立 ridge-kernel MCP）** 发现当前端点；无活动
kernel 时 fail closed，不读取旧 teammate 环境变量或 sidecar。无 Tauri 仅内核时，companion 仍可连
`http://127.0.0.1:<kernel-port>/api/v1/mcp`（token 同 kernel.json）。显式指定可用
`ridge-mcp --url http://127.0.0.1:PORT --token <tok>`；仅兼容旧宿主时才加 `--legacy-sidecar`，
以启用 `RIDGE_TEAMMATE_URL` / `RIDGE_TEAMMATE_TOKEN` 与 Ridge runtime endpoint sidecar 后备发现。
sidecar 仅属运行期、只接受普通文件与 `127.0.0.1` 端点（Unix 要求 `0600`）。端口或 token 更新后，
首次连接失败或 `401/403` 会重新发现并重试一次。修改 Codex MCP 配置后须新开会话。

### E. 无头 `rdg`（独立 host；按需）

`rdg` 只用于无桌面环境托管 Ridge 的无头会话；它不是 Ridge 桌面 MCP 的安装器。无头 `rdg tmux` 同样会导出
`RIDGE_TEAMMATE_*`；如需由上节的 `ridge-mcp` companion 接入旧无头 host，须显式加
`--legacy-sidecar`。旧 `rdg mcp` 仅保留兼容别名。

### 端点与 token 从哪来

| 来源 | 说明 |
| --- | --- |
| `RIDGE_TEAMMATE_URL` / `RIDGE_TEAMMATE_TOKEN` | 旧 teammate env；仅在显式 `--legacy-sidecar` 后备发现中读取 |
| `%LOCALAPPDATA%/ridge/kernel.json` | 独立 `ridge-kernel` 登记（`pid/port/token`）；`POST /api/v1/mcp` 为无桌面 MCP 面（REQ-RIDGE-MCP-AS-KERNEL-API-01） |
| `TMPDIR/ridge-teammate-endpoint-*.json` | 旧运行期 sidecar（`{"url","token"}`，非 MCP 持久配置）；仅在显式 `--legacy-sidecar` 后备发现中读取 |
| `rdg tmux` 启动日志 | 无头 host 把 `RIDGE_TEAMMATE_*` 导出行打到 stderr |
| `rdg kernel ensure|agents|fs-list|mcp-smoke` | CLI 验收：拉起内核、领域只读、MCP tools/list |

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

## 3. 主要协作工具

| 工具 | 参数 | 用途 |
| --- | --- | --- |
| `ridge_get_team_profile` | 无 | **先调它**。花名册：成员 `paneId` + `paneIndex` + 状态 + 编组 |
| `ridge_send_to_teammate` | `agent_id 或 target_pane_id, message, submit?, from?` | 按 fenced Agent 或 pane 写入 Hub；默认 `submit=true`，仅显式 `false` 保留草稿；不代表 PTY 已写入 |
| `ridge_send_and_submit` | `agent_id 或 target_pane_id, message, from?` | 同一 Hub 入口，强制 `submit=true`；返回 `deliveryId`，不代表 PTY 已写入 |
| `ridge_delegate_task` | `agent_id 或 target_pane_id, objective, max_steps?` | 排入 Hub 并标记任务；同样不等于 Agent 已执行 |
| `ridge_delivery_status` | `agent_id 或 target_pane_id, delivery_id` | 按 `deliveryId` 查询投递状态：终端接受、Agent 确认分列 |
| `ridge_acknowledge_receipt` | `agent_id 或 target_pane_id, delivery_id, status, detail?` | 目标 Agent 明确确认/拒绝；唯一路径可令 `agentAcknowledged=true` |
| `ridge_report_execution_rejection` | `executor, policy_source, request_id, reason, next_step, ...` | 上报外部网关拒绝；显示归因卡，**不**伪称 Ridge 可重试 |
| `ridge_capture_pane` | `target_pane_id, lines?` | 抓该 pane **渲染后**的屏幕文本（监控队友进展，不是转义序列堆） |
| `ridge_inbox_read` | `agent_id 或 target_pane_id, peek?` | 取走投递给该 fenced Agent/pane 的消息（跨 agent 异步回话通道） |
| `ridge_report_progress` | `target_pane_id, status, detail?` | 回流一条进展（桌面落前端进度事件） |
| `ridge_get_launch_capabilities` | 无 | 动态发现宿主 profile 及允许的 model/reasoning；空集合禁止覆盖 |
| `ridge_split_pane` | `direction, role, launch_profile?, model?, reasoning_effort?, ...` | 开新 pane；profile/覆盖值须来自上项，不猜命令或品牌 |
| `ridge_join_group` | `group_name, agent_id? \| target_pane_id?` | 加入按名字寻址的已有编组（桌面前端 SSOT，fire-and-forget） |
| `ridge_stash_data` | `data` | 存文本，返回 `ridge://cache/<id>`，供别的 agent 回读 |

**寻址**：`target_pane_id` 同时接受花名册回传的 `paneId`（桌面是 Uuid 串，无头是 `%N`）与
`paneIndex`（数字）。越界/失效目标返回 `-32602`，绝不静默落到 0 号分屏。

**Agent identity 寻址**：Hub/通信工具可用 `agent_id` 或 `target_pane_id`（二者并存时须为同一份 Kernel roster identity）。显式 `workspace_id`/`generation`/`lease` 必须与 identity 相等；identity 须同时给出 workspace、generation、lease、online、lifecycle、capabilities，否则 fail closed。canonical Hub key 同时含 workspace、Agent、generation、lease，故 selector 可互读收件箱、状态、ACK、幂等结果。agent-only 不调用 host pane resolve/pane_key/probe，不走 PTY；仅 MCP pull、已注册 Runtime 或 A2A。静态 tmux roster 若无法提供完整 fence，不宣称 `agent_id` 可寻址。PID/进程名只能作发现证据，不授予 PTY、stdin 或控制台注入权限。`ridge_capture_pane`、`ridge_report_progress` 等 pane 操作仍仅支持 `target_pane_id`。

**投递语义**：`submitRequested` 仅表明提交意图；`terminalAccepted=true` 仅表明 host 已完成终端写入；
均不表明终端程序或 Agent 已消费。返回字段以 `deliveryId` 为准；只有目标 Agent
调用 `ridge_acknowledge_receipt` 后，回执才会出现 `agentAcknowledged=true`。

**外部拒绝**：若执行层在 Ridge 之前拒绝（例如 `rejected: blocked by policy`），调用
`ridge_report_execution_rejection`。卡片必须给出真实执行者、策略来源、request ID、原因与可行下一步；
它不是 Ridge HITL 挂起项，Ridge 不会把“知悉”描述为已批准或已重试。

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
2. 没有合适的 pane → 先调 `ridge_get_launch_capabilities`；仅从返回清单选 profile/model/reasoning，再调 `ridge_split_pane`。清单为空或所需项不支持即停，不默认 RidgeCode 或任何 CLI。
3. `ridge_delegate_task { target_pane_id, objective }` 派活并保存 `deliveryId`；需核验投递状态时调用 `ridge_delivery_status`，别把排队回执说成已执行。
4. 大块上下文走 `ridge_stash_data` 拿 `ridge://cache/<id>`，把 URI 写进 objective 让对方 `resources/read`。
5. 想知道干得怎么样 → `ridge_capture_pane` 抓屏；对方可用 `ridge_report_progress` 主动汇报。
6. 收对方留言 → `ridge_inbox_read`（取走即清空；`peek: true` 只看不取）；目标 Agent 消费后以 `ridge_acknowledge_receipt` 明确回执。

---

## 6. 当前限制（诚实说明）

- **服务端主动推送（`notifications/progress`）仍未实现**：当前是请求-响应循环。要「进展更新」请轮询
  `ridge_capture_pane` / `ridge_get_team_profile`，或让 worker 调 `ridge_report_progress`、leader 读收件箱。
- 所有动作落在**当前活动工作区**（桌面）/ **default socket**（无头），暂不支持跨工作区寻址。
- `ridge_join_group` 一次写入·fire-and-forget：编组数据在前端 localStorage，后端无法确认是否落地；
- `ridge_join_group` 入参：`group_name` 必填，且须 `agent_id` **或** `target_pane_id`（可只给 agent_id，不必解析 pane）；成员须在花名册（`teammate_agent_pane_map` 含 `auto:` + typed profiles）。非法参数 JSON-RPC `-32602`；companion/无桌面宿主返回 MCP `isError`（能力不支持，非 silent OK）。见 `REQ-MCP-JOIN-GROUP-01`。
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
`packages/ridge-mcp-bridge/`（独立 `ridge-mcp` stdio companion；`rdg mcp` 复用它）。用户手册：`docs/teammate-user-guide.md`。*
