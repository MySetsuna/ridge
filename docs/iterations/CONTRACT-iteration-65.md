# Contract · Iteration 65 · Commune 连续性与 Remote 丝滑状态

状态：APPROVED  
日期：2026-07-28  
需求：`REQ-AGENT-COMMUNE-CONTINUITY-01`、`REQ-AGENT-HISTORY-01`、
`REQ-REMOTE-SMOOTH-STATE-02`

## 目标 1 · 复合 Pane 身份成为业务 SSOT

- 定义共享 `PaneRef { workspaceId, paneId }` 与稳定 key helper。
- `RemoteLink` 的 subscribe/stdin/claim/refresh/scrollback、metadata/raw/resize callback 全携带 PaneRef。
- `TerminalManager` kernel/park/unpark/feed/detach/scrollback cursor 以复合 key 管理。
- `MainApp` 删除 `attachedPaneWs` 与业务路径 active workspace fallback；异步闭包捕获请求发起 PaneRef。
- LAN host 对 subscribe/stdin/claim/scrollback 校验 workspace 中确有 pane；缺/错 workspace 明确 error。
- connect/hello 只可建立初始 workspace 上下文，不得替业务帧补值。

验收：

- `wsA/pane1` 请求未决时切 `wsB`，迟到 pane list/metadata/raw/scrollback 仍只更新 `wsA/pane1`。
- 同 paneId/不同 workspace fixture 生成两 kernel，不串 feed/detach/cursor。
- 缺 workspace 的 stdin/claim/scrollback/subscribe 被拒；错误 workspace 不写 PTY。

## 目标 2 · 单 sink 有界优先 writer

- 从 `remote_host_impl` 主 `tokio::select!` 移出所有 `ws_tx.send`；独立 writer task 排他拥有 sink。
- 三 lane：control、active raw、low（background raw/scrollback）；均 bounded，容量复用
  `ridge_remote::pane::RAW_CHAN_CAP`，不造无运行证据阈值。
- 严格优先；已开始的 WebSocket frame 不取消，之后立刻重查 control/active。
- background 满标目标 pane dirty；scrollback 满/取消返回有界失败，不阻塞 socket reader。
- 断连/取消关闭 sender，writer 退出，任务与计数归零。

验收：

- 人为挂起 low frame send，stdin 收取/PTY 写入与 control/active enqueue 仍推进。
- low backlog 前插 control/active，最多越过一个已开始 low frame。
- 取消 scrollback 后 low queue、pending request、writer task 计数归零。
- 无 `ws_tx.send` 遗留于入站/事件 select 分支。

## 目标 3 · 后台保活与历史分页原子化

- visited 集合、订阅、dirty/resync、kernel 与 paging cursor 全以 PaneRef 管理。
- 切 pane/workspace 只降级旧 pane 为 background、park UI、转焦点/尺寸所有权；不退订、不清 kernel。
- 重连先恢复 background，当前 pane 最后升 active；普通切换不得 RIS/全量 replay。
- scrollback 请求带 AbortSignal/请求代次；响应仅能向发起 PaneRef prepend，成功后才 commit cursor。

验收：

- `ws1/A → ws1/B → ws2/C → ws1/A` 后三订阅/kernel 存续；A 离开期间尾帧切回即见。
- 关闭 A 只清 A；关闭 ws1 只清 A/B；断连销毁全部。
- 快速滚顶/切 workspace/取消时旧页不提交，seq 不前进，pending 归零。

## 目标 4 · 软键盘 cursor-only 定位与显式回底

- `TerminalCanvas` 建立唯一 `openSoftKeyboard()`：
  `manager.scrollToBottom(ref) → resolve cursor/fallback → position textarea → focus({preventScroll})`。
- cursor 只来自 kernel input anchor；不可见/越界/未知时取 terminal 与 visualViewport 交集中心。
- pointer/touch 坐标只用于 TUI mouse/selection；不写输入锚点。
- `scrollToBottom` 补 `wake()`；hidden textarea 至少 16px，保持 1px/透明/不改 layout。
- 仅显式唤起触发回底；后台 viewport/历史浏览不强拉底。

验收：

- 不同触点得到同一锚点；cursor 有效精确投影，无效回退可见终端中心。
- spy 断言调用顺序；非底部唤起后 offset=0 且立即 redraw。
- keyboard 开合前后 canvas/container 高度、grid rows/cols、host claim 次数不变。

## 目标 5 · Commune DOM 与 pane 状态同源

- `TopologySnapshot` 保留 `rosterChanged`；仅 true 时转为现有 `teammate-layout-changed/state`。
- 无变化轮询不发事件；手工注册/暂停/恢复/退出继续复用既有后端事件。
- Agent Center 顶级 Tab 恰为“成员/编组/历史”；控制、文档、HITL、健康区实际 DOM 位于 Tab 主体后。
- pane header 与 Agent Tab 都由后端 topology/pane tree 状态投影，不另建持久状态。

验收：

- 自动发现 false→true 恰一 layout state；不变零；退出/暂停/恢复值一致。
- 结构测试证明三 Tab 与控制区 DOM 顺序、键盘可达、HITL 行为不变。

## 目标 6 · Agent 历史发现与诚实恢复

- 延续 `CONTRACT-iteration-64.md`：删除“最近回复”独立投影，新增有界只读 history DTO。
- adapter registry 分离 process evidence、discovery、resume；Claude/Codex/OpenCode 与损坏/未知 fixture。
- MiMo/Grok/中国主流 CLI 仅有证据者纳入；无可靠 resume 则展示并禁用。
- 按原生 session ID 与运行 roster 归并；不得凭 title/cwd 猜。
- 恢复只用结构化 executable/argv/cwd，在当前工作区新建 pane；单飞且无 shell 拼接。

验收：

- 三组混排、组/全部折叠、a11y、运行中替换/退出回退/同名隔离。
- 有界文件/字节/条数；损坏 adapter 不拖垮页面。
- 可恢复 adapter 捕获 argv/cwd/session id，恶意参数不经 shell；未安装者诚实禁用。

## 允许路径

- `src/remote/**`
- `packages/remote/src/shared/{terminal,transport,cloud}/**`
- `packages/ridge-remote/src/pane.rs`
- `src-tauri/src/remote_host_impl.rs`
- `src/lib/teammate/**`
- `src/lib/components/RidgePane.svelte`
- `src/routes/+page.svelte`
- `src-tauri/src/commands/{teammate,project,pane,terminal}.rs`
- 对应测试、fixture、E2E、本轮 docs

## 禁止路径

- ridge-cloud 部署、发布/version bump
- 第二 WebSocket/DataChannel
- 无界队列、固定 FPS/延迟/DRR magic 权重
- Query cache/localStorage 持有 PTY bytes
- 第三方 session 写入或猜测性 resume
- 后台 pane 抢尺寸所有权

## 验证闸

- `requirements_gate.py assert-executable`
- 聚焦 Vitest：identity、transport、manager、keyboard、Agent history/DOM/state
- Rust：remote writer/identity/scrollback、teammate/project/pane
- `pnpm check`
- `cargo test -p ridge --lib`
- desktop + mobile production build
- `scripts/remote-state-e2e.mjs` 加厚：复合身份、后台尾帧、键盘回底、scrollback 背压下连续输入

## 追踪

| 需求 | 代码主落点 | 验收 |
| --- | --- | --- |
| `REQ-REMOTE-SMOOTH-STATE-02` 复合身份 | RemoteLink/Manager/MainApp/host | 竞态与拒绝缺省 |
| 同上 writer/scrollback | `remote_host_impl.rs` | 挂起 low sink 进度 |
| 同上 keyboard | `TerminalCanvas`/manager | 顺序、center、no resize |
| `REQ-AGENT-COMMUNE-CONTINUITY-01` | AgentCenter/model/+page | DOM 与恰一事件 |
| `REQ-AGENT-HISTORY-01` | project/adapter/AgentCenter | 有界发现、诚实恢复 |

## 目标 7 · 审批归因与投递回执真实化

- Ridge 仅为 MCP/Agent 输入链时，审批卡必须展示具体执行者、策略来源、request id 与可执行下一步；不得将未证实的外部拒绝泛称为“Ridge/宿主策略”。
- 审批事件早于 renderer 挂载或 renderer 重启时，本地只读恢复快照必须重建卡片；提交裁决失败或已失效时卡片不得静默出队。批准既有挂起输入后，才可表述“继续原操作”。
- `ridge_send_to_teammate` 仅返回 `draft_injected`；`ridge_send_and_submit` / `ridge_delegate_task` 才返回 `submit_dispatched`。`terminalAccepted` 仅可在 host 的终端写入成功后报告（无头无法确认时为 false）；`agentAcknowledged` 仅能由 `ridge_acknowledge_receipt` 明确置真，任何状态均不得表述为「Agent 已执行」。
- Ridge 桌面 MCP 已内置；Codex 等 stdio 客户端须通过独立 `ridge-mcp` bridge 动态发现端点，不得把会漂移的 localhost URL/token 固化进配置，也不得以 `rdg` 冒充桌面 Ridge 的安装步骤。
- 外部执行网关的拒绝可经 `ridge_report_execution_rejection` 生成本地恢复卡；必须展示真实 executor/policy/request ID/reason/next step，且卡片不得把“知悉”冒充审批或自动重试。
- Ridge 桌面终端粘贴多行必须保持源顺序；每次粘贴作为一个原子 PTY payload，后续输入不得越过它，且输入必须路由至显式 `(workspaceId,paneId)`，不得落到当前激活工作区。

验收：

- Rust 测证明远端 pending 投影仍不含原 action，本地恢复快照可重建同一请求；前端类型/检查全绿。
- MCP 测证明 draft 与 submit 回执不同；桌面写入成功可报告 `terminalAccepted=true`，且显式确认前 `agentAcknowledged=false`，确认后才转为 `agent_acknowledged`。
- `ridge-mcp` 单测与 stdio→HTTP 冒烟证明请求/鉴权转发；Codex 配置指向 `ridge-mcp`，而非固定端口或 `rdg`。
- 测覆盖多行 PTY echo 经 delta mirror 后仍按源顺序落行；同 pane 异步写入 FIFO，显式 workspace 不随活动工作区漂移。
