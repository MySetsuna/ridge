# Ridge 内置 MCP Server · 接入文档

> 适用版本：≥ v0.0.8（Domain C「内置端侧 MCP server」随该版本发布）。

---

## 有没有内置 MCP？—— 有

Ridge **自带一个端侧 MCP（Model Context Protocol）server**。任何 MCP-原生的客户端（如 Claude Code、Cursor、或你自己写的 MCP 客户端）都能挂进来，用标准协议驱动 Ridge 的多智能体协同能力——在分屏间派活、互发消息、读取工作区上下文。

- **传输**：WebSocket
- **协议**：JSON-RPC 2.0（MCP 2024-11-05）
- **端点**：`ws://<host>/api/v1/mcp/ws`
- **鉴权**：Bearer token

> 这是仓库里**唯一**的 MCP server；没有其它内置或外挂的 MCP 服务端。它复用 Ridge 既有的 teammate HTTP/WS 传输（`src-tauri/src/teammate/server.rs`）+ 纯协议核心（`packages/ridge-core/src/mcp/`）。

---

## 1. 怎么找到端点和令牌

Ridge 为每个 **teammate 分屏**注入两个环境变量——你的 MCP 客户端进程（跑在某个分屏里的智能体）直接读它们即可，无需手配：

| 环境变量 | 含义 |
| --- | --- |
| `RIDGE_TEAMMATE_URL` | teammate 服务的 base URL（形如 `http://127.0.0.1:<port>`），把 scheme 换成 `ws`、路径接 `/api/v1/mcp/ws` 即为 MCP 端点 |
| `RIDGE_TEAMMATE_TOKEN` | Bearer 鉴权令牌 |

WebSocket 连接时带上鉴权头：

```
Authorization: Bearer <RIDGE_TEAMMATE_TOKEN>
```

（也支持 `X-Ridge-Token: <token>` 头，与 teammate 其它路由一致。）

> teammate 服务**按需惰性启动**：进程在首个 PTY 创建时拉起并绑定，所以只有「在 Ridge 分屏里运行」的客户端才拿得到上述环境变量。

---

## 2. 握手与发现

### initialize

```jsonc
// → 请求
{ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }

// ← 响应
{
  "jsonrpc": "2.0", "id": 1,
  "result": {
    "protocolVersion": "2024-11-05",
    "serverInfo": { "name": "ridge-teammate", "version": "0.0.8" },
    "capabilities": { "tools": {}, "resources": {} }
  }
}
```

### tools/list

```jsonc
// → 请求
{ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }

// ← 响应（result.tools[]，每项含 name / description / inputSchema）
```

可用工具（注册表，`mcp/registry.rs`）：

| 工具 | 用途 | 状态 |
| --- | --- | --- |
| `ridge_send_to_teammate` | 向指定分屏的队友发一段文本/消息 | ✅ 已接线（落活动工作区） |
| `ridge_delegate_task` | 给某个 Worker 派活（注入任务 + 标记 Working） | ✅ 已接线（落活动工作区） |
| `ridge_split_pane` | 分屏，开一个新分屏给队友 | ⚙️ 已登记，`tools/call` 暂未路由 |
| `ridge_stash_data` | 把数据暂存到内存 Stash，供 `ridge://cache/<id>` 读取 | ⚙️ 已登记，`tools/call` 暂未路由 |
| `ridge_get_team_profile` | 取团队花名册 | ⚙️ 已登记（也可走 `resources/read`） |

### resources/list（约定）

资源以 `ridge://` URI 暴露：

| URI | 内容 | 状态 |
| --- | --- | --- |
| `ridge://workspace/active-panes` | 活动工作区花名册（roster + leader + edges） | ✅ 已接线 |
| `ridge://workspace/git-status` | 工作区 Git 状态 | ⚙️ 已定义 URI，读取暂未接线 |
| `ridge://workspace/editor-context` | 编辑器上下文 | ⚙️ 已定义 URI，读取暂未接线 |
| `ridge://cache/<id>` | 从内存 Stash 读暂存数据 | ⚙️ 已定义 URI，读取暂未接线 |

---

## 3. 调用工具

### tools/call — 给队友派活

```jsonc
// → 请求
{
  "jsonrpc": "2.0", "id": 3, "method": "tools/call",
  "params": {
    "name": "ridge_delegate_task",
    "arguments": { "target_pane_id": 2, "objective": "为缓存层补单元测试" }
  }
}

// ← 响应
{
  "jsonrpc": "2.0", "id": 3,
  "result": { "content": [ { "type": "text", "text": "delivered" } ] }
}
```

- `ridge_send_to_teammate`：参数 `{ target_pane_id, message }`，向该分屏注入文本。
- `ridge_delegate_task`：参数 `{ target_pane_id, objective }`，注入任务 + 把目标分屏标为「工作中」。
- `target_pane_id` 是**分屏索引**（从 0 起），落在**当前活动工作区**。

未实现的工具名会返回 JSON-RPC 错误 `-32601 method not found: unknown tool: <name>`。

---

## 4. 读取资源

### resources/read — 读活动工作区花名册

```jsonc
// → 请求
{
  "jsonrpc": "2.0", "id": 4, "method": "resources/read",
  "params": { "uri": "ridge://workspace/active-panes" }
}

// ← 响应（text 内嵌 JSON 字符串：{roster,leaderId,edges}）
{
  "jsonrpc": "2.0", "id": 4,
  "result": {
    "contents": [ {
      "uri": "ridge://workspace/active-panes",
      "mimeType": "application/json",
      "text": "{\"roster\":[...],\"leaderId\":null,\"edges\":[]}"
    } ]
  }
}
```

其它 `ridge://` URI 当前返回 `-32602 resource not yet available`（URI 已定义、读取后续接线）。非法 URI 返回 `-32602 invalid ridge:// uri`。

---

## 5. 错误码

标准 JSON-RPC：`-32700` 解析错误 / `-32600` 非法请求 / `-32601` 方法或工具未找到 / `-32602` 参数无效 / `-32603` 内部错误。

---

## 6. 当前限制（诚实说明）

- `tools/call` 目前只路由 `ridge_send_to_teammate` 与 `ridge_delegate_task`；其余工具已在 `tools/list` 中可见但调用返回 unknown。
- `resources/read` 目前只接 `ridge://workspace/active-panes`。
- **`notifications/progress` 服务端推送暂未实现**（需要 WS split sink）；当前是请求-响应循环。
- 所有动作落在**当前活动工作区**（暂不支持跨工作区寻址 pane）。

---

## 7. 最小客户端示例（Node，原生 WebSocket）

```js
// 跑在 Ridge 分屏里：读环境变量拿端点 + 令牌
const base = process.env.RIDGE_TEAMMATE_URL;      // http://127.0.0.1:<port>
const token = process.env.RIDGE_TEAMMATE_TOKEN;
const url = base.replace(/^http/, 'ws') + '/api/v1/mcp/ws';

const ws = new WebSocket(url, { headers: { Authorization: `Bearer ${token}` } });
let id = 0;
const call = (method, params = {}) =>
  new Promise((res) => {
    const myId = ++id;
    const onMsg = (e) => {
      const m = JSON.parse(e.data);
      if (m.id === myId) { ws.removeEventListener('message', onMsg); res(m); }
    };
    ws.addEventListener('message', onMsg);
    ws.send(JSON.stringify({ jsonrpc: '2.0', id: myId, method, params }));
  });

ws.addEventListener('open', async () => {
  await call('initialize');
  const tools = await call('tools/list');
  console.log(tools.result.tools.map((t) => t.name));
  await call('tools/call', { name: 'ridge_delegate_task', arguments: { target_pane_id: 1, objective: '跑单元测试' } });
});
```

---

*相关文档：用户手册 `docs/teammate-user-guide.md`；设计细节 `docs/superpowers/specs/2026-06-19-domain-zero-teammate-design.md`。*
