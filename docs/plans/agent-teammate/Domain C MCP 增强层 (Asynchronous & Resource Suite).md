明白！**Domain C (MCP 增强层)** 是让 `ridge` 从一个“硬核物理终端工作台”正式跃升为“高智商多智能体操作系统”的外挂引擎 。

如果说 Domain A 和 B 解决了“AI 能不能在终端里互相说话、干活”的生存问题，那么 Domain C 解决的就是“**它们能不能秒传大文件、能不能像人类一样并发工作而不卡死屏幕**”的发展问题 。

以下是 Domain C 的详细技术规范说明书（Specs）。

# 🛠️ Ridge 技术规范说明书：Domain C —— MCP 增强层 (Asynchronous & Resource Suite)

本规范定义了 `ridge` 如何在 `packages/ridge-core` 和 `Teammate Server` 中原生集成 MCP（Model Context Protocol）服务端架构 ，为原生支持 MCP 的核心特工（如 Claude Code ）提供一条脱离底层 PTY 字符流的**高速、结构化、非阻塞数据总线**。

## C1. `ridge` 内置端侧 MCP 服务端架构规范 (Local MCP Server Spec)

由于 `ridge` 已经使用了 `axum` 作为其 Local HTTP/WebSocket 服务器底座，MCP 的接入将极其平滑。我们将基于标准的 JSON-RPC 2.0 协议实现 MCP Server。

### 1.1 传输层与协议装载 (Transport Layer)

MCP Server 将挂载在现有的 Teammate Server 端口上，支持两种连接模式：

- **SSE (Server-Sent Events) 模式**：供某些 HTTP 优先的 Agent CLI 调用。挂载路径为 `/api/v1/mcp/sse`。

- **WebSocket 模式（推荐）**：提供全双工低延迟通信。挂载路径为 `/api/v1/mcp/ws` 。

### 1.2 核心 Tool 注册表 (MCP Tools Registry)

当 Agent 发送 `tools/list` 请求时，`ridge` 返回以下高度结构化的工具能力字典：

JSON

```
{
  "tools": [
    {
      "name": "ridge_split_pane",
      "description": "在当前工作区物理切分出一个新终端窗格，并指派新 Agent[cite: 60].",
      "inputSchema": {
        "type": "object",
        "properties": {
          "direction": { "type": "string", "enum": ["horizontal", "vertical"] },
          "role": { "type": "string", "description": "例如: tester, code-reviewer" },
          "initial_cmd": { "type": "string", "description": "新窗格启动后自动注入执行的命令" }
        },
        "required": ["direction", "role"]
      }
    },
    {
      "name": "ridge_send_to_teammate",
      "description": "向指定的 Pane ID 发送后台协同消息（无终端回显污染）[cite: 92].",
      "inputSchema": {
        "type": "object",
        "properties": {
          "target_pane_id": { "type": "number" },
          "message": { "type": "string" }
        },
        "required": ["target_pane_id", "message"]
      }
    }
  ]
}
```

### 1.3 `tools/call` 的 Rust 核心调度映射

在 `packages/ridge-core` 中，接收到 JSON-RPC 调用后，通过 `serde_json` 反序列化，并直接映射到 Domain B 的 Topology Bus：

Rust

```
pub async fn handle_mcp_tool_call(
    method: &str,
    params: Value,
    state: &WorkspaceState
) -> Result<Value, McpError> {
    match method {
        "ridge_split_pane" => {
            let req: SplitPaneReq = serde_json::from_value(params)?;
            // 触发 Domain B 拓扑引擎及 Tauri 视图层分屏
            let new_pane_id = state.topology.spawn_worker(req.role, req.initial_cmd).await?;
            Ok(json!({ "status": "success", "new_pane_id": new_pane_id }))
        },
        // ... 其他 Tool 路由
        _ => Err(McpError::MethodNotFound),
    }
}
```

## C2. 跨物理分屏的非文本资产缓存与共享资源 (Resource Cache & AST Transport)

这是 MCP 带来的最大“降维打击” 。为了防止庞大的 JSON、错误堆栈或 AST 语法树塞爆 PTY 导致卡顿或截断，`ridge` 定义了一套自定义的 `ridge://` 协议簇。

### 2.1 动态内存资源库 (`ridge://workspace/*`)

`ridge` 核心引擎直接将现有的丰富状态通过 `resources/read` 暴露，Agent 读取这些内容时零 I/O 开销：

- `ridge://workspace/active-panes`：返回工作区所有存活的 Agent 画像、性格及忙闲状态（JSON 格式）。用于 Leader 派活前的“查花名册” 。

- `ridge://workspace/git-status`：由于 `ridge` 底层集成了 `libgit2` ，可以直接返回结构化的修改列表（如哪些文件处于 staged 状态，哪些有 merge conflict），而无需 Agent 在终端里敲打 `git status` 。

- `ridge://workspace/editor-context`：如果用户在使用 Monaco Editor 模式 ，此资源返回用户当前光标所在的行号和选中的文本块。

### 2.2 跨特工大文件物流中转站 (`ridge://cache/*`)

当 Pane 1 的 Agent 需要向 Pane 2 传递一个 5MB 的日志文件或编译产物图表时：

1. **写入（Pane 1）**：Pane 1 触发 Tool `ridge_stash_data(content_base64)`。

2. **生成凭证**：`ridge` 内存（或内置的 SQLite ）暂存该数据，并返回一个 UUID 凭证：`ridge://cache/a1b2-c3d4`。

3. **发送信使**：Pane 1 调用 `ridge_send_to_teammate`，消息体仅包含：“测试失败了，完整堆栈见 `ridge://cache/a1b2-c3d4`”。

4. **读取（Pane 2）**：Pane 2 收到消息，解析出 URI，直接发起 MCP `resources/read` 读取巨量日志，瞬间完成上下文共享。终端界面依旧干干净净。

## C3. 异步非阻塞 Tool 调用与通知机制 (Async Tool Call & Notification)

为了让 Team Leader 实现“人类经理级别”的并发管理能力（给小弟派完活，自己继续干别的，小弟干完再来汇报），系统引入了**挂起与事件回拨机制**。

### 3.1 异步任务下发 (Fire and Forget)

当 Leader 调用 `ridge_delegate_task` 时，`ridge` MCP Server 会立即返回 `200 OK`，并附带一个 `Task Ticket`：

JSON

```
{
  "status": "dispatched",
  "task_id": "tsk_9988",
  "assigned_pane": 2,
  "message": "Task is running in background. You will receive an MCP notification upon completion."
}
```

此时，Leader 的终端输入焦点不会被锁死，它可以继续对其他 Pane 派活。

### 3.2 服务端推送事件 (Server-Initiated Notifications)

当小弟（Pane 2）完成任务并触发了结束标记（通过 Domain A 的 TML 或 Domain B 的 API）时，`ridge` 后端会主动向 Leader 的 MCP 连接推送一个标准通知：

JSON

```
{
  "jsonrpc": "2.0",
  "method": "notifications/progress",
  "params": {
    "progressToken": "tsk_9988",
    "data": {
      "status": "completed",
      "exit_code": 0,
      "summary": "单元测试通过，共 45 个用例全部 PASS。"
    }
  }
}
```

Leader 的 Agent 内核捕获到这个 Notification 后，会在其思维链中隐式拼接该结果，进而决定下一步的开发策略。

通过 C1、C2、C3 的结合，Domain C 让 `ridge` 脱离了简单的“终端复读机”形态，变成了一个支持高并发通信、结构化数据共享和后台算力调度的**超级大本营**。
