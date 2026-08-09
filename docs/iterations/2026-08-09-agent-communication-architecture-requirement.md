# Agent 通信架构重构：详尽需求说明

状态：已纳入 Active Requirement `REQ-AGENT-COMMUNICATION-ARCH-REBUILD-01`。  
范围：Ridge 项目现状、愿景与规划基线（2026-07-21）→ 来源「Agent 通信架构重构」。

## 1. 来源结论与本仓解释

来源的核心判断不是废弃 MCP，而是拆开三个被混用的层次：

```text
MCP 工具调用 → Message Hub 消息/任务语义 → Runtime/Delivery Adapter → 具体 CLI Agent
                                                        └→ PTY fallback
```

MCP 是外部 Agent 访问通信系统的控制面；Message Hub 才保存 inbox、topic、task、event、artifact、subscription 与 delivery；Runtime Adapter 负责唤醒/继续目标 Agent；PTY 仅为无程序化接口 Agent 的最后兼容通道。来源把 A2A 放在外部 Agent 互操作层，不替代本地高频事件总线。

来源原始证据：NotebookLM source `9516749e-c317-4f13-9cda-b64b00cec465`，关联 notebook `66919cb9-1329-4ddf-955c-f426d15a9fe6`；临时对话补充 source `df4d5dcc-9813-4c61-ae9f-1e9199cb7555`，关联 notebook `f6ffd900-708d-44ee-9818-1a3269c533fc`，作为同等重点约束。临时来源特别强化浏览器 cookie/API 边界、MIME/大小/SHA256 校验、轮询与重试上限、token/cookie 不入日志、取消必须穿透 worker/子进程等运行安全要求。来源中关于具体 CLI 官方能力、SQLite/WebSocket/NATS 选型与 PTY 物理条件，均须以本仓代码、能力探测、测试和运行证据复核，不直接视作事实。

## 2. 要解决的问题

当前不能把“向目标终端写一段字符串并模拟 Enter”当成通信协议，因其：

- 丢失发送者、消息类型、权限、优先级与任务身份；接收 Agent 无法区分用户输入、Agent 建议、系统事件、任务与控制指令。
- MCP 工具调用是 Agent 主动行为，不等于 MCP Server 能主动唤醒正在运行的 Agent；“写入 inbox”与“Agent 已开始处理”必须分成两个状态。
- 普通消息、事件和任务语义混淆；任务必须有接受、执行、完成、失败、取消、超时与产物关联。
- PTY 受 Agent 生成中、授权确认、TUI 重绘、焦点、用户键盘输入、前台 shell 进程及控制字符影响，可能注入错误 pane、污染用户输入或丢消息。
- 桌面、Remote、rdg、ridge-mcp、headless host 若各维护身份/状态副本，必然出现 roster、history、group、pane border 与恢复入口漂移。

## 3. 目标架构

### 3.1 Kernel/Teammate SSOT

内核/Teammate 服务持有唯一 Agent registry、生命周期、任务边、消息索引、订阅、delivery 和审计事实。UI、Remote、rdg、MCP、headless 只读写该契约，不创建第二身份目录。

稳定身份至少包括：

```text
agent_id, session_id, workspace_id, pane_id
cwd, executable, argv, capabilities
generation, lease, status, online, last_seen
```

身份不得由 pane title、显示名称或 CWD 推断；CWD 是上下文属性，不是身份键。

### 3.2 生命周期与任务状态

Agent 生命周期：

```text
discovered
  → spawning / attaching
  → online
  → working / waiting / attention
  → completed / stopped / failed / offline
```

创建只有在 spawn/attach 成功后提交；销毁只有在 destroy/lease closure 成功后提交；中间失败保留 diagnostic-only 记录，不留下可发送的假成员。每次重连递增 `generation`，以 `lease` 拒绝旧 Agent 的迟到消息。

Task 生命周期至少为：`created → assigned → accepted → running → waiting/blocked → completed | failed | cancelled | expired`。任务状态不得由普通文本推断；每次状态转移需要操作者、时间、原因、关联消息与可选 artifact。

### 3.3 强类型消息模型

统一 envelope 至少包含：

```text
message_id
correlation_id
causation_id
idempotency_key
from / to
workspace_id / pane_id
generation / lease
kind, type, sequence
priority, created_at, deadline
payload or artifact reference
ack / nack / typed error
```

`kind` 固定区分：

| kind | 语义 | 例 |
|---|---|---|
| `message` | 协作内容，不改变任务状态 | `review.comment` |
| `task` | 可接受、执行、完成/失败/取消 | `code.fix` |
| `event` | 发布订阅事实 | `tests.failed`、`git.changed` |
| `control` | 中断、暂停、恢复、接管 | `interrupt` |
| `artifact` | diff、日志、报告、构建物 | `test-report` |
| `reply` | 对消息或任务的结构化答复 | `task.result` |

## 4. Message Hub 与 MCP 契约

### 4.1 Hub 责任

第一阶段可用 Rust + SQLite 保存 `agents/messages/subscriptions/deliveries/tasks/artifacts`，配合内存事件通道/WebSocket 做本地实时分发；业务模型不得绑定某个 NATS 实现。Hub 必须支持：

- inbox 持久化、未确认消息、消费游标与重复消费防护。
- topic 订阅、明确 consumer、通配范围与取消订阅。
- task graph、父子任务、correlation/causation 链。
- 每个目标独立 delivery 记录：适配器、尝试次数、状态、错误、时间、可靠性等级。
- 背压、上限、取消、过期回收；不得因一个离线 Agent 无限堆积。
- UI 关闭后消息、任务和订阅仍可被恢复；内核退出时给出可观察失败，不假成功。

### 4.2 MCP 工具

MCP 只操作 Hub，不直接写 PTY。最小工具面：

| 工具 | 必需行为 |
|---|---|
| `ridge_send_message` | 校验目标身份/lease/capability，写入 inbox，返回 `message_id` 与 delivery 状态 |
| `ridge_create_task` | 创建带类型、输入、截止时间、优先级的 task，返回 `task_id` |
| `ridge_publish_event` | 向 topic 发布结构化事件，按订阅者生成 delivery |
| `ridge_fetch_inbox` | Agent 主动 pull，支持 limit、wait、cursor 与明确 ack 语义 |
| `ridge_task_update` | 以合法状态转移更新 task，携带 result/error/artifact |
| `ridge_list_agents` | 返回指定 workspace 的稳定 roster、状态、能力、generation/lease 摘要 |
| `ridge_agent_ack` | 确认接收/处理，区分 received、accepted、completed、rejected |

“已写入终端”不得等同“目标 Agent 已处理”；每层返回独立状态。

## 5. Delivery Engine

适配器选择顺序：

1. Runtime API/SDK/app-server/HTTP/ACP：目标 Agent 有官方程序化入口时，传结构化消息并继续会话。
2. A2A Adapter：跨系统/远程 Agent 能力发现、任务、状态与 artifact 互操作。
3. MCP Pull：目标 Agent 主动 `ridge_fetch_inbox`，适合普通通知及不允许主动唤醒的场景。
4. PTY Fallback：无上述能力时的 best-effort 兼容层。

每次 delivery 必须记录 `delivery_adapter` 与 `delivery_reliability`。来源中对 Codex/Claude/OpenCode/Gemini 的接口判断只作候选；本仓须通过能力探测、adapter contract test 或真实/等价 E2E 证明后才能启用。

### PTY 安全闸

同时满足以下条件才可注入：

```text
agent.status == idle
terminal.mode == agent_prompt
pending_approval == false
shell_foreground_process == target_agent
```

另须确认无用户键盘竞争、无 active command、目标 pane/lease 未变化；注入内容带机器消息隔离标识，失败立即退回 Inbox，不盲目重试、不打断用户。PTY 失败必须可诊断，且 `best_effort` 不得冒充可靠送达。

## 6. 可靠性、安全与可观测性

- `idempotency_key` 保证并发重复发送只产生一个逻辑消息；delivery attempt 可重复但业务副作用不可重复。
- 目标 stale/offline 时，发送前最多一次有界 roster refresh；随后返回稳定错误，不静默 respawn。
- 消息、任务、控制按优先级排队；用户输入、控制、HITL 高于 history/render；队列、字节数、等待时长均有上限。
- cancel 必须穿透到 adapter、PTY、timer、listener、worker 与 pending RPC；销毁后计数归零。
- 权限按 sender、target、workspace、kind、capability、trust level 校验；控制指令与 artifact 访问不得默认等同普通 message。
- 日志不得包含 cookie、token、浏览器存储或未脱敏 payload；每次投递关联 `trace_id/message_id/task_id`。
- 观测至少包含 roster refresh、ack latency、queue depth、delivery adapter、retry/cancel、stale rejection、teardown residue、resume result。

## 7. 本仓对齐与缺口

CodeGraph 当前确认：

- `packages/ridge-core/src/teammate/topology.rs` 已有 Teammate 节点、任务边、确定性 leader 与 `Working` 投影；需与 Kernel registry/generation/lease、Hub delivery 对齐，不能作为第二 SSOT。
- `packages/ridge-kernel/src/agent_profiles.rs` 的 `AgentProfile` 被 Kernel MCP、Tauri teammate discover、project commands 等多处使用；当前 CodeGraph 未发现充分覆盖测试，需先补契约测试。
- `src/lib/teammate/agentCommuneModel.ts` 已把 live roster status 与 cold history 分开，history 以稳定 Agent identity 分组且不依赖 CWD；应把该纯投影绑定到后端结构化身份和 typed task/event 状态。
- `AgentCenterPanel.svelte` 已有结构化 resume：`executable/argv/cwd/sessionId`、single-flight 与失败 pane 清理；下一步需让其使用 Kernel lease/generation 与 delivery/task 状态，而非仅由 UI 控制。
- `SidebarTeamRoster.svelte`、AgentCenter/Commune 与 Remote roster 当前存在 group/history/resume/send 等多端入口；必须共享同一 DTO、错误码、ack 和权限语义，并补覆盖测试。

## 8. 渐进迁移

### Phase 1：MCP 与 PTY 解耦

建立 Hub/Message/Delivery 最小内核；`ridge_send_message` 先入 Hub，再由旧 PTY adapter 消费。保留旧 UI 与 PTY 能力，但所有投递带 adapter/reliability 标记。

### Phase 2：Runtime Matrix 与智能投递

建立 Agent 状态机、能力矩阵、generation/lease fencing、MCP pull 与 Runtime adapter；补齐 ack、去重、取消、背压、HITL 与 stale 错误。

### Phase 3：外部互操作

接入 A2A/ACP/HTTP/SDK adapter 与 artifact 映射；A2A 只做边界适配，Hub 内部模型保持 Ridge 自有语义。

## 9. 确定性验收

1. CodeGraph trace 覆盖 Kernel registry/lifecycle → Hub → MCP/rdg/Runtime/PTY adapters → desktop/Remote/headless projections → tests。
2. create/attach/destroy 成功各一次；任何失败不留下 active 假成员；旧 generation、过期 lease、离线目标发送前拒绝。
3. 相同 `idempotency_key` 并发发送只产生一个逻辑消息；delivery、ack、task 状态可逐步验证；cancel/destroy 后 pending、timer、listener、worker、queue 归零。
4. message/task/event/control/artifact 的序列化、权限、优先级、correlation/causation、错误码均有 Rust/TypeScript 确定性测试。
5. 多 Agent、多 workspace、多 CWD、多 session 的 desktop/Remote/headless fixture 证明身份不依赖 title/CWD；group、leader、history、status、aria-label 同源。
6. history cold scan 不阻塞 live roster/input；损坏/超大 JSONL 局部失败且可诊断；resume 单飞并在当前 workspace 创建唯一 pane。
7. ridge-mcp 无 Tauri 桌面时能 initialize/tools/list 并成功执行 roster 或 communication 工具；Kernel 退出时返回 typed failure，不假成功。
8. PTY 仅在四项安全闸满足时注入；busy、approval、用户输入、前台进程不符时留在 Inbox，且有可靠性与退避证据。
9. 实际或等价 Multi-Agent E2E 覆盖 create → send → delivery → receive → ack → reconnect generation → destroy，无重复、旧消息投递或资源泄漏。
10. 全量测试/check、相关 Rust/TypeScript/Remote 回归与 Sonar 质量闸通过；新增问题为 0，运行命令、报告、失败原因与残余差距留档。
