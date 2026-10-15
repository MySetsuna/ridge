---
id: L3-COMMUNE-PROTOCOL-MEMBERSHIP
level: L3
title: Protocol-based terminal Agent Commune, groups, history and A2A
status: DRAFT
lifecycle: PLANNED
parent: L2-UNIFIED-CLIENT-RUNTIME
code_targets:
  - packages/ridge-acp/**
  - packages/ridge-mcp/**
  - packages/ridge-tmux/**
  - packages/ridge-kernel/**
  - packages/ridge-core/**
  - packages/ridge-cli/**
  - packages/remote/**
  - src/lib/teammate/**
  - src/lib/agent-chat/**
  - src/lib/components/SettingsPanel.svelte
  - src/routes/+layout.svelte
  - src/remote/**
  - src-tauri/src/teammate/**
  - src-tauri/src/bridge/**
  - src-tauri/src/commands/**
  - src-tauri/src/state.rs
  - src-tauri/src/lib.rs
  - docs/archive/**
test_targets:
  - packages/ridge-acp/**
  - packages/ridge-mcp/**
  - packages/ridge-kernel/**
  - src/lib/teammate/**
  - packages/remote/**
depends_on:
  - L2-AGENT-COMMUNE-EXACT-BINDING
  - L2-RIDGE-COMMUNION-M53
  - L2-RIDGE-PRODUCT-M1
---

# Agent’s Commune：通用协议成员、编组、历史与 A2A

## 批准边界与现状

用户明确要求：移除右侧 Agent’s Communion；保留左侧 Commune 的成员、编组、历史；所有 Agent 由用户在终端运行，Ridge 识别并连接同一会话；不按每种 Agent 写业务适配器，支持通用协议即可接入；沿用用户可扩展的 Agent 清单，并实现 A2A 协作。2026-10-11 用户明确批准“提升为确定要做的内容”：本方案已定版，列为必须实施与验收的内容。尚未修改业务代码；产品方案批准不等于已取得当前 SpecTree 实施锁。

配属 CHG-070 候选，与桌面展示层化统一 Runtime 一并实施。CHG-069 持锁期间保存已批准内容的提案；正式编号以登记时 `stc next` 为准。新契约正式应用前必须修订已有 ACP/Communion、精确绑定与 Hub 投递契约中的冲突，不直接改权威 specs/changes。

已核对：`AgentCenterPanel.svelte` 提供三个页签；历史仍读取 `read_agent_recent_replies`、恢复仍按 provider 启动命令；`agent_profiles.rs` / `agent_catalog.rs` 已有内置与用户覆盖；`acp_bridge.rs` 与 `ridge-acp/provider_adapter.rs` 仍有启动新 Agent 的路径；`src/routes/+layout.svelte:498` 挂载右侧 Communion。`ridge-mcp/a2a.rs` 已有协议客户端，`native_a2a.rs` 已有认证 A2A/Hub 服务入口，可复用，但不据源码存在宣称端到端完成。

## 1. 一套身份与服务，三个投影

```text
用户终端里的既有 Agent ── ACP / 已支持通用协议 ── Commune 服务
                                                   ├─ 成员投影
                                                   ├─ 编组与任务投影
                                                   ├─ 历史投影
                                                   └─ A2A 网关 ←→ 其他成员/授权外部 Agent

已有 Agent 主动协作 ── 原生 A2A client 或共同 MCP 工具 ── 同一 A2A/任务服务
```

服务位于独立 Runtime，桌面/CLI/Remote 均为其客户端。Ridge 不启动新 Agent、不使用模型供应商 API 替代终端 Agent、不将协议/自动协作投递转为 PTY 提示词注入，不采样终端内容冒充协议回复。用户主动的侧栏终端查看与人工输入按本轮批准的“终端 Agent 侧栏基础交互”执行。三页签读取同一登记、会话、任务与事件事实，不能另建三套识别/状态/历史系统。

### 协议与配置

- Ridge 实现按协议划分的客户端与传输驱动，例如 ACP、A2A；同协议的所有 Agent 共用代码。不得按 Claude/Codex/Gemini 等名称分支处理消息、历史、审批或恢复，也不从模型厂商配置决定真实成员身份。
- 现有 Agent 清单保留为用户可编辑配置：名称/图标、进程识别规则、协议类型、连接发现配置及凭据引用。它不是准入白名单，未列出的 Agent 也可经授权协议登记。配置里的能力只能作为说明；按钮权限来自真实握手，用户勾选不能制造能力。
- “等协议”指实施并通过互操作验收的通用协议，不承诺自动理解任意未知协议。协议扩展新增一份共同驱动；不新增逐品牌适配器。MCP 是工具入口，不能因 Agent 支持 MCP 就推断其支持 ACP 会话接入。
- 现有 `resumeArgv`、`yoloArgs`、provider 历史扫描退出 Commune 在线执行路径；旧配置及已索引历史归档。用户仍可查看旧记录，但不能凭名称/CWD 自动绑到新会话。

## 2. 已有终端会话如何接入

1. Kernel 以 pane 对应的进程树发现候选；识别规则由清单提供，只证明“发现进程”。Agent 也可向通用登记入口提供连接描述，名称不限制注册。
2. 连接已存在的协议端点，认证并完成版本/能力握手；验证它确实服务当前终端会话。登记是 Ridge 的连接管理职责，并非宣称 ACP 自带 PID 发现或终端绑定标准。
3. 通过协议返回的 session 身份与经过验证的端点登记，绑定 host、workspace、pane、进程实例、协议会话；标题/进程名/CWD 均不能作会话主键。PID 必须带进程启动实例标记，防复用；协议 sessionId 必须在认证 peer 命名空间内使用。
4. 绑定明确且具备能力后，注册为可交互成员。端点不存在/认证失败/协议不兼容/会话歧义分别显示原因；不能另起 ACP 进程或调用 `session/new` 冒充接管已有会话。
5. 连接断开只撤销连接与控制租约；进程退出才标已退出，缓存与历史仍保留。重新连接校验运行代际与绑定 revision，不把新进程自动当旧成员。

**真实能力边界：** ACP v1 标准 stdio 是客户端启动子进程、通过换行 JSON-RPC 通信；不是任意普通 CLI 的附加接口。已运行 Agent 必须已有可连接的 ACP/通用协议端点或由其运行方提供共同的传输入口；普通终端 stdin/stdout 不能强夺或混写 ACP。用户新增一项清单不能把不支持协议的 CLI 变成协议 Agent。[ACP transports](https://agentclientprotocol.com/protocol/v1/transports)

可复用通用 IPC/双向消息转发，但它只转发协议，不能根据品牌解析 CLI 或另启模型会话。只有 stdio 且未暴露既有连接的 Agent 可识别、定位终端，不能宣称已 ACP 接入。协议持有者若不支持同时控制，Commune 明示控制权不可用；不得用两个 client 同写 stdin。此限制须在第一阶段用真实外部 Agent 验证，不能用新启动 fixture 的通过替代。

## 3. 成员页签：谁在运行、能做什么

成员页默认聚合全部本地 workspace 的终端 Agent 与已登记协议成员；每项保留精确 workspace/pane/运行代际/进程实例，协议登记不作为人工终端查看与操作门槛。协议编组与历史继续按各自授权 workspace 查询。每条成员展示：用户名称、协议自报身份、所属终端、连接状态、当前任务/待审批、最近真实回复与可用功能。

“发现/待连接/已连接/失联/已退出”与“空闲/执行中/待审批/待输入/未知”分列；不能把进程活着等同协议在线，也不能从无输出猜已完成。未知 Agent 名称可显示握手信息；任何能力未知则按钮禁用并说明原因。

| 操作 | 行为 |
| --- | --- |
| 定位终端 | 聚焦精确 pane，终端不存在则说明原因 |
| 发消息/派任务 | 进入共同任务队列，对绑定的既有 session 发协议 prompt；同会话串行，忙时显示排队/明确拒绝策略 |
| 停止当前任务 | 请求协议 cancel，等待实际终态；不杀进程、不冒充暂停进程 |
| 审批 | 回应当前会话实际权限请求；多客户端只允许一个成功处理结果 |
| 查看历史 | 打开同一 session 的历史投影 |
| 移出 Commune | 撤销登记/后续路由，处理未发队列；不结束用户 Agent，进行中任务须明确选择等待或取消 |

消息在服务持久受理、协议发送、Agent 执行和任务终态分别记账。发送超时显示“结果待确认”，不能自动重发可能已执行的 prompt。连接恢复先查询/核对；不支持确认时由用户决断重发。ACP cancel 不等于退出 Agent，也不等于可任意暂停/恢复进程。

审批与流采用官方 `session/request_permission` / `session/update` 等已协商能力；不能把现有 Ridge 私有 `session/approve`、`agent/request_approval` 通知当所有 ACP Agent 的标准。[ACP tool calls](https://agentclientprotocol.com/protocol/v1/tool-calls)、[prompt turn](https://agentclientprotocol.com/protocol/v1/prompt-turn)

## 4. 编组页签：现有成员的协作关系

编组持久化于共同服务，以 workspaceId、稳定 groupId、revision 管理；前端只缓存投影。保留现有“一成员至多一编组”的语义，未分组为视图而非独立任务对象。成员引用精确登记身份，改名不影响归属，退出保留失联位置；替换成员须显式操作，不能按品牌/CWD补位。组长由已有成员担任，不创建 Lead/Staff Agent。

组卡显示组长、成员能力/在线状态、正在执行的组任务及分项结果。给组长发目标、向选定成员广播、按成员分工派任务是明确不同动作；组长不存在时不能把任务偷偷发给随机成员。

任务流程：用户向已有组长提交目标 → 组长提供方案 → 用户审定成员与分工 → 服务将子任务送到各自既有会话 → 汇总成员真实终态与产物 → 可再提交给既有组长综合。若组长具备主动调用 A2A/MCP 工具的能力，可在用户预先授权的成员/任务范围内自行派发。不能靠 system prompt 宣称已具备调用工具能力。

每次派发冻结 group revision、目标成员与绑定 revision；编辑组不改变已受理任务，加入新成员不自动收到旧任务。一个组任务关联多个子任务；部分失败、待审批、成员失联逐项呈现，不能整体假报成功。取消组任务按真实子任务取消能力执行，已完成成果保留。队列有界、同会话单写；任务失联不能改投同名成员。

## 5. 历史页签：真实会话与任务的账本

默认按当前 workspace 的真实会话展示，可按成员、编组、任务、时间搜索。对话、工具活动、审批和 A2A 协作链在一条可定位时间线中呈现；组任务关联子任务与实际产物，普通最新回复不等于交付成果。

历史由两类同源数据构成：Commune 已观察并持久化的协议事件；Agent 通过所协商的会话列表/回放能力提供的历史。Provider 列表是可选能力，缺失时只显示已登记会话；无法回放接入前内容时标“自接入起记录”，不能硬编码读取每家 JSONL 来冒充完整历史。[ACP session list](https://agentclientprotocol.com/protocol/v1/session-list)、[session setup](https://agentclientprotocol.com/protocol/v1/session-setup)

查看历史默认只读 Commune 缓存，不自动运行 Agent、创建会话、触发 prompt 或修改活动终端。显式“同步历史”使用协商能力；可能影响当前 session 的 load/resume 操作不得在背景查询执行，忙时等待或要求用户明确操作。分页使用协议原生 cursor，不解析它。

恢复分两种用户行为：仍在线会话“定位/重新连接”；进程已退出则显示“需用户在终端恢复，再连接”。恢复不由 Ridge 按品牌 CLI 命令拉起进程。协议支持回放/重新连接也不证明死亡进程已恢复，不能为了恢复调用 `session/new` 生成另一个会话。

接入前历史覆盖率、丢失区间、断线时段明确显示。实时/回放有稳定消息 ID 时去重；无 ID 则完整回放作为单次快照事务替换该协议历史投影，不能以文字相同推断重复。Commune 自己的投递、审批与协作账本不被回放覆盖；服务事件使用本地持久序号给 UI 续读。

## 6. A2A：让成员相互请求工作

已确认 A2A 是 Agent 间任务协作协议；Agent Card 描述能力与服务接口，任务有自己的标识、状态和产物。它与 ACP 的客户端会话控制职责不同；“注册 ACP”不会自动产生一个原生 A2A 服务或主动调用能力。[A2A specification](https://a2a-protocol.org/latest/specification/)

### 6.1 统一网关提供被调用能力

Commune 为已连接且授权的成员提供独立可发现的 A2A 路由与真实能力卡。卡绑定现有 member/session，服务实现共用一份，不为每个品牌启动一个 A2A server/Agent。现有 `ridge-mcp/native_a2a.rs` 与 `a2a.rs` 优先复用。

调用链：A 的授权请求 → Commune A2A 边界 → 共同任务/Hub 登记 → B 既有会话的 ACP prompt（B 原生 A2A 则使用共同 A2A client）→ 真实输出/终态回到任务服务 → 回给 A。ACP/A2A 语义转换仅按协议定义，不按品牌判断；不能把任意 ACP 通知机械映射为相同 A2A 状态。

A2A taskId/contextId 与 ACP sessionId、memberId、Hub deliveryId 各有职责，显式关联而不混为同一 ID。请求来源、授权 workspace、目标成员与上下文映射由认证服务校验；客户端 metadata 不能自行决定发送者或跨 workspace路由。每个 A2A context 对精确 ACP conversation 建立并发/串行约束，禁止不同调用者的上下文串入一个共享会话。

纯 ACP 成员只有一个既有对话且不能隔离多 context 时，只提供与该对话绑定的授权协作入口；不广告通用多租户会话创建。不能为隔离调用私自 `session/new`。成员断开后卡与任务可显示不可用，服务不启动替身。

### 6.2 主动调用其他成员

能力须单独显示：`可被调用` 与 `可主动协作`。被调用能力可由统一网关桥接；主动协作需要该 Agent 已有原生 A2A client，或能调用用户配置的共同 Commune MCP 工具。复用已有工具/Hub 路径，工具身份必须绑定真实成员、会话与权限；不再按品牌写工具适配器。[A2A and MCP](https://a2a-protocol.org/latest/topics/a2a-and-mcp/)

同组发现 → 向目标成员派发 → 查询/等待结果 → 获取产物是共同工具能力。已在运行的 Agent 若不支持加载此工具/配置，Ridge 不能从 ACP 握手强行给它加工具；成员仅作为被调用者，用户在自己的终端环境配置通用工具后再启用主动协作。首期协作限当前 workspace 的已授权同组成员，外部跨 Host 互联另经用户明确授权。

需防环：子任务带来源与父任务链，拒绝重复环，限制转发深度、任务量与队列；接收 Agent 不能仅因消息内容声称是组长就获得新权限。工具审批仍由目标会话/用户处理，A2A请求不自动授权文件或命令执行。输出产物按现有权限取用，不裸露本机路径/令牌给外部调用者。

### 6.3 修正现有 A2A 的完成语义与互操作

当前 `native_a2a.rs:263` 把 `adapter_accepted` 映射为 completed。这只证明投递获接收，不能用于 Commune Agent 工作的完成判定。新链路须区分投递回执与执行任务，真实 prompt 终态/产物才结算；审批暂停、需输入、失败、取消、结果未知均明确处理。

现有卡为整个 Ridge Hub，目标通过 Ridge metadata 指定；完善成员级路由，标准客户端不应猜私有 `ridge.target` 才能调用某成员。对照官方版本化 schema 校验字段、枚举、错误、取消、流与能力卡；不能因已有方法名称/单元测试即认定与官方互操作。仅广告实际实现的能力：未实现流/完整 history/上下文隔离则如实关闭。

## 7. 最小数据边界

| 对象 | 职责与保存 |
| --- | --- |
| Agent 清单配置 | 可编辑识别与协议连接参数；不等于在线成员；凭据仅保存安全引用 |
| Member | 稳定内部 ID、所属 workspace、peer、精确 conversation；名称可改，退出保留记录 |
| Live binding | host/runtimeEpoch/pane/进程实例/协议端点/sessionId/bindingRevision；重连需验证，不复制业务身份 |
| Group | workspace、成员 ID、可选既有组长、revision；持久服务权威 |
| Task / Delivery | 用户或 Agent 请求、来源/目标/父子关系、发送与实际执行状态、产物关联；原子持久受理 |
| Conversation events | 原始合法协议事件与规范化展示索引、本地持久序号、覆盖区间；不收集任意终端原文冒充消息 |

ACP/A2A wire 字段引用官方版本 schema，Commune 内部关联字段不伪称上游规范。服务暴露的具体 DTO/注册鉴权机制在实施前随权威接口提案定版，不能由各 UI 自行追加。一会话多 UI 共享一连接和执行队列；审批响应只能完成一次；断线不能无条件 exactly-once承诺。

## 8. 移除、迁移与实施顺序

1. 收敛协议合规：现有 ACP 包中 Content-Length 默认、私有通知与审批扩展不能作为通用 ACP 标准；以官方 schema/互操作证据校准。保留可复用协议解析、请求关联、取消、持久事件能力，退出 provider_spawn/自动创建会话路径。
2. 沿用用户清单，做通用发现/登记/握手/精确绑定；先验证两个不同实现、一个非预置自定义 Agent 都接入既有终端 session，零新 Agent 进程。
3. 建共同成员/会话/事件与任务事实，接通成员页；再接历史回放与覆盖标识，避免先做三套 UI 假数据。
4. 编组状态迁共同服务；旧 group/成员标识只在精确映射时迁移，歧义记录归档保留。最后补 ACP↔A2A 任务桥及共同 MCP 主动协作工具。
5. 归档右侧 Communion 展示、启动入口、默认模型/provider设置和对应专属协调 owner；不能只隐藏按钮，后台仍运行。通用协议库与有用测试保留，不整包误删。
6. 正式规格同步须更新 `L2-RIDGE-PRODUCT-M1`、M5.3 与精确绑定契约中 Ridge启动 Agent、PTY恢复和旧投递保证，迁移不得以保留旧已批准实现为由覆盖本次用户目标。保持 CHG-069 的输入/渲染/加载改动和 CHG-070 的展示层边界。

## 9. 必须证明的验收

- 两个同品牌同 CWD 的终端 Agent 与一个自定义未预置 Agent：同协议驱动、分别精确绑定；更名、PID复用、跨 workspace、断线重连不能串成员/历史/审批。
- 注册/派发/恢复/关闭 UI 均不启动、接管、杀死用户 Agent；不使用 PTY提示词、私有历史解析或逐品牌恢复适配器。普通 CLI无端点时准确显示未接入。
- 一个支持会话回放、一个不支持的 Agent：历史来源和覆盖率准确；回放不重发消息、不触发任务、不卡当前会话；旧记录仍可只读。
- 向组派发、改组、成员失联、部分失败与取消：目标快照与 revision冻结，各成员真实状态可查，回执成功不能冒充任务完成。
- 两个真实成员 A→B→A：通过共同 A2A入口/MCP调用，有关联任务、执行结果与产物；至少一项实际权限请求只批准一次。缺主动工具者可被调用但不显示自主派发。
- 官方 A2A 客户端发现成员卡并完成调用/查询/取消；版本、枚举与流能力一致；同会话不同context隔离不支持时明确拒绝。循环派发、伪造来源、越权workspace、重复消息、超时结果未知与队列溢出均有断言。
- 桌面关闭后 CLI/Remote仍读到同一 Commune 登记/编组/历史；右侧与其专属启动链路无活动引用。实施时运行相关 Rust、真实协议互操作、UI与 npm test/lint/typecheck，最后 SpecTree verify/complete。本设计轮未运行实施测试。

# 终端 Agent 侧栏基础交互 — 2026-10-16 已批准

批准依据：用户明确要求汇总所有工作区所有终端中的 Agent，查看最新内容、发送文字、选择与回复；在已展示“复用现有终端读写、ACP 为增强、不改变启动命令”的具体方案后，用户指令“那就继续实现吧”。本次是 070 的已批准修正，不要求再次审批同一方案。

1. 成员页默认聚合全部本地工作区，保留 workspaceId、paneId、终端运行代际与 Agent 进程实例。沿用用户可扩展的进程识别清单，不按品牌解析业务或私有历史。协议登记不是展示及人工操作的门槛。
2. 未登记协议的终端 Agent 是终端观察投影，不伪造 protocol session、Hub delivery identity、任务完成或审批能力。侧栏显示“终端最新内容”，数据直接复用 Runtime 现有终端解析画面；不冒称精确的最后一条模型消息。
3. 用户显式点击发送、方向键、回车、Tab、Escape 或 Ctrl+C，复用原终端的人工输入与顺序去重链路。Runtime 验证工作区、pane、终端代际和进程实例；拒绝过期、跨工作区、已退出或替换后的目标。仅返回“输入已写入终端”的真实 ACK，不冒充 Agent 理解、采纳或任务完成。未知结果保留原请求，禁止自动重发或改投同名进程。
4. 选择控件表示终端按键，不把终端文字解析成协议审批。文本粘贴及按键遵循当前终端模式；提供可展开的终端内容与原终端定位。读取不发输入、不改焦点/尺寸、不启动进程；保持原有 CLI 启动命令与系统 profile。
5. ACP/app-server/A2A 保留为结构化消息、真实权限请求、协议历史及自动协作增强。编组与协议历史仍遵守精确会话、workspace、revision 和真实能力边界。纯人工终端操作不进入 Hub 自动派发，也不是协议投递失败时的 PTY fallback；不把 LLM、MCP 或 A2A 派发改为终端提示词注入。
6. 此前试写的自动包装 Agent 启动入口退出活动产品链路并归档；不要求用户换用共享协议启动。已安装版本不自动修改，验收后仅产出新安装包，由用户手动安装。

验收：至少两个工作区、多个实际终端 Agent/自定义进程同时可见；原终端普通启动可操作；侧栏最新内容更新；同一 PTY 收到文字及选项按键且其他终端不收；读取无写入/无新进程；重复请求不重写，过期 epoch、进程实例及跨 workspace 拒绝；协议历史来源与语义不变。运行相关 Runtime 与 UI 验收、npm test/lint/typecheck、SpecTree verify，未满足完整 070 验收不得标完成。
