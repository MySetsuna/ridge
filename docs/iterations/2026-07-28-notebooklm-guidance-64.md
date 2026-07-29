# NotebookLM Guidance · 2026-07-28 · Iteration 64→65

来源：`REQUIREMENTS-SPEC`、`PROJECT-STATE`、`DEEP-RESEARCH-REMOTE-SMOOTH-2026-07-28`  
研究任务：`97967dfd-c412-4ed1-bde6-a6cfb0b4b60f`

## NLM 原始方向

NLM 建议按以下顺序推进：

1. 端到端固化 `(workspaceId,paneId)`；
2. 单连接内建立专职 writer 与有界优先队列；
3. 补齐后台 pane 保活、键盘 cursor-only 锚定与 scrollback 取消；
4. Agent Commune 重排、三 Tab、历史发现/恢复及 pane header 状态同步。

其核心判断正确：当前问题非单个 UI 动画，而是身份、生命周期与 socket sink 所有权不一致。PTY bytes
继续绕过 Query cache；同一认证连接内治理背压；第三方 Agent session 只读。

## 引用核查

- `PROJECT-STATE` 直接支持：manager 单 `paneId` 键、UI active workspace fallback、host 单任务
  `await ws_tx.send`、`rosterChanged` DTO 丢失。
- `REQUIREMENTS-SPEC` 直接支持：cursor-only、回底、复合身份、有界可取消历史、Agent 三 Tab/历史。
- 深研报告支持：sink 排他 writer、应用层优先调度、同步 focus 手势域、IME fallback。
- NLM 提及的 `priorityWriter.ts`、固定 DRR 权重、`translateY(-2000px)`、`order:99` 并非代码事实，
  仅属方案建议，不可直接照搬。

## 对抗评审与 reframe

| 原建议 | 更高价值可测切片 | 验收信号 | 结论 |
| --- | --- | --- | --- |
| 新建 TS `priorityWriter.ts` | Rust `remote_host_impl` 让独立 writer task 排他拥有 `ws_tx`；读循环只处理入站与有界入队 | 挂起低优先 sink 时 stdin handler 与 active enqueue 仍推进 | reframed 采纳 |
| DRR 固定权重 100/80/20 | 复用既有 `RAW_CHAN_CAP`，按 control→active→background/scrollback 严格优先；每次至多一个已开始低帧 | 无 magic 权重；active 最多越过一个低分片 | reframed 采纳 |
| Stdin 放 critical 出站队列 | stdin 是入站；从 sink await 中解耦读循环，PTY 写继续 `spawn_blocking` | 低 sink 挂起后仍收到/写入 stdin | 纠错采纳 |
| `translateY(-2000px)` 抑制 Safari | 用户手势栈内 `scrollToBottom→cursor/fallback anchor→focus({preventScroll})`；输入字号至少 16px | 不引魔法位移；DOM/grid/PTY 尺寸不变 | reframed 采纳 |
| 无 cursor 用整个 visual viewport 中心 | 用 terminal 可见内容 rect 中心，clamp 于容器；visual viewport 只参与可见 rect 交集 | header/底栏不把锚点推离 terminal | reframed 采纳 |
| `order:99` 移控制区 | 实际移动 DOM 至三 Tab 主体之后 | 结构测试比较 DOM 顺序与键盘可达性 | reframed 采纳 |
| rosterChanged 直接覆写 manager status | 保留后端 pane tree 为 SSOT；前端只把 `rosterChanged=true` 升格为现有 `teammate-layout-changed/{kind:state}` | 自动发现恰一事件；无变化零事件 | reframed 采纳 |
| 50MB/s 下 stdin ≤100ms | 不用硬件相关时间阈值；挂起 low writer 的结构化进度测试 | stdin/control/active 计数在 low 未完成时增长 | reframed 采纳 |
| 临时 shadow active workspace fallback | 只允许 hello 初次兼容；业务消息缺 workspace 明确拒绝 | grep/合同测证明业务路径无 fallback | 原形 non-goal；以强类型迁移替代 |
| 猜测更多 CLI resume | 发现/识别/恢复三能力分离；无官方/本机证据即诚实禁用 | executable/argv/cwd fixture；未知 CLI `canResume=false` | 原形 non-goal；以只读展示替代 |

## 锁定实现决策

1. `PaneRef={workspaceId,paneId}` 为客户端/transport/host 业务身份；序列化 key 仅由一只 helper 生成，
   不在调用点拼接。
2. LAN socket sink 由单 writer task 独占；控制、active raw、low 三只 bounded lane 复用现有容量。
   队列满：background 标 dirty；scrollback 返回 busy/cancelled；不得无界等待读循环。
3. 业务消息缺 `workspaceId` 拒绝；仅 connect/hello 可从 host 初始 active workspace 建立首个显式上下文。
4. Terminal kernel 与 scrollback cursor 均以复合 key 管理；切 workspace 只 park/unpark/转焦点与尺寸所有权。
5. 键盘显式唤起唯一入口调用 `scrollToBottom`，随后解析 cursor；无效则 terminal 可见区中心；touch
   坐标只留 TUI mouse/selection。
6. Agent 自动发现复用既有 layout state 事件；History 延续 iteration 64 的有界只读 DTO 与结构化恢复。

## 减法

- 删除 `attachedPaneWs` 影子归属 Map 与业务路径 `?? ui.activeWorkspaceId`。
- 删除 `replayedPanes: Set<paneId>`，其职责并入复合 session registry。
- 删除“最近回复”独立 UI/DTO 投影；History 成唯一历史入口。
- 删除 host 业务消息对 `active_ws_id` 的缺省补值。
- 不新增第二连接、PTY Query cache、无界 backlog、固定 FPS/延迟权重、猜测性 resume。

## 停机条件

- 需改变已批准产品语义；
- 需新增物理连接或无界缓存方可通过；
- CLI session 路径/恢复参数只能靠猜；
- composite 协议无法同时兼容 LAN/cloud/rdg 而需另造副本；
- 确定性测试无法复现用户现场同构故障。

