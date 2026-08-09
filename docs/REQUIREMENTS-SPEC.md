# Ridge · 需求规范（REQUIREMENTS-SPEC）

> 本地只保留此一份需求文档。Pending 未获用户明确批准前，不改代码、不生成执行合同、不上传；
> NotebookLM 继续使用上一版已批准的 `REQUIREMENTS-SPEC` 来源。

- 需求版本:`v1.1`

## 正式需求 (Active Requirements)

### REQ-TERMINAL-PASTE-ORDER-02

- 状态:`ACTIVE`
- 版本:`v0.3.0`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 日期：`2026-07-29`
- 版本：`v0.3.0`
- 批准证据：用户授权“审核无问题即全部通过”；审计通过。
- 类型：`FIX`
- 关联：`CONTRACT-iteration-65.md` 目标 7。
- 原始意图：ridge 粘贴多行时，行序不得颠倒、交错或错乱。
- 当前证据：`RidgePane`、LAN/Cloud `RemoteLink` 现先以
  `paneInputGate` 占位异步 clipboard/image 读取，再以单一 bracketed-paste payload
  投递；键入、拖放、Agent/MCP 与既有 PTY/RPC FIFO 共用复合 `(workspaceId,paneId)` 出口。
  gate、PTY FIFO、Pane RPC、LAN/Cloud 与 host-topology 定向测已绿；真实 ConPTY/手机现场仍待补。
- 目标行为：本机、接入 pane、Remote pane 的一次粘贴皆保持源字符与行序；同 pane 后续键入、
  MCP 注入、拖放路径不得越过粘贴 payload。
- 范围：桌面/Remote 终端粘贴入口、每 pane 写队列、PTY stdin 与 delta mirror 测试。
- 非目标：改变 shell 自身对 bracketed paste 的解释；为跨 pane 输入建立全局队列。
- 边界：不得以逐行延时、固定 sleep 或全局串行掩盖；须保留显式复合 pane 身份。
- 假设/待确认：需采集失败发生于快捷键、右键、拖放还是 MCP 注入路径；各路径分别归因。
- 验收：编号多行含 CRLF/LF、Unicode、末尾无换行 fixture；PTY echo 与 delta mirror 字节序
  等于输入；并发后续键入只可位于 payload 之后；真实 Windows ConPTY 冒烟。
- 预期落点：`RidgePane.svelte`、PTY 写队列、Remote stdin 路由及对应 TS/Rust/E2E。

### REQ-TERMINAL-RASTER-01

- 状态:`ACTIVE`
- 版本:`v0.3.0`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 日期：`2026-07-29`
- 版本：`v0.3.0`
- 批准证据：用户授权“审核无问题即全部通过”；审计通过。
- 类型：`FIX`
- 原始意图：ridge-term 中 PowerShell/Codex 的细线、边框与字形须接近原生 PowerShell 的平滑度。
- 当前证据：共享 `TerminalManager` 默认优先 WebGPU、失败回退 Canvas2D，并按 DPR 量化 cell；
  `image-1.png` 仍显示观感差异，现无原生对照矩阵。
- 目标行为：同字体、字号、DPR 与缩放下，线框连续、基线稳定、字形不锯齿/发虚；WebGPU 与
  Canvas2D fallback 不得产生明显不同的 cell 几何。
- 范围：ridge-term 字形栅格、字体栈、glyph atlas、Canvas2D/WebGPU 设备像素映射。
- 非目标：重做终端协议解析；改变用户选定字体语义。
- 边界：不得以 CSS blur/transform、截图后处理或改 PTY rows/cols 假装修复。
- 假设/待确认：先判字体栈、hinting、device-pixel 对齐、WebGPU atlas 或 Canvas2D raster 哪层失真。
- 验收：DPR `1/1.25/1.5/2`、100%/125%/150% 缩放的原生 PS 对照图；同一 box-drawing fixture
  验证 cell、基线、1px 线连续；两 backend 均有可复核捕获。
- 预期落点：共享 terminal manager/font stack、ridge-term renderer 与视觉 fixture。

### REQ-CODEX-RENDER-STABILITY-01

- 状态:`ACTIVE`
- 版本:`v0.3.0`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 日期：`2026-07-29`
- 版本：`v0.3.0`
- 批准证据：用户授权“审核无问题即全部通过”；审计通过。
- 类型：`FIX`
- 原始意图：Codex 在 ridge-term 连续输出时，画面与光标不得到处闪烁。
- 当前证据：非活动 pane 已调用 `setFocused(false)` 隐藏光标；同步输出、dirty-row 与强制重绘
  亦有专门路径，但现场仍复现。尚无证据证明与 Claude 旧缺陷同根。
- 目标行为：活动 pane 仅真实终端光标按协议闪烁；非活动 pane 无光标；Codex 流式更新不得交替
  呈现半帧、旧帧或多位置 cursor。
- 范围：Codex/Claude 流式 PTY 录制、kernel dirty 状态、render/present 与 cursor focus 投影。
- 非目标：关闭所有动画；为单一 Agent CLI 写硬编码渲染分支。
- 边界：先以 kernel cell/dirty/render trace 分层归因；不得简单关闭所有 cursor blink 或降低刷新率。
- 假设/待确认：Codex 与 Claude 是否同根，须由同一录制回放矩阵裁决。
- 验收：同一 Codex JSONL/PTY 录制重放；逐帧断言单 cursor、单调 frame generation、无旧行复活；
  与 Claude fixture 对照，明确共同或独立根因。
- 预期落点：`RidgePane.svelte`、`TerminalManager`、ridge-term render loop 与录制回放测试。

### REQ-HISTORY-OVERLAY-GEOMETRY-01

- 状态:`ACTIVE`
- 版本:`v0.3.0`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 日期：`2026-07-29`
- 版本：`v0.3.0`
- 批准证据：用户授权“审核无问题即全部通过”；审计通过。
- 类型：`MODIFY` + `FIX`
- 原始意图：历史弹层依 pane 可见区域动态定位；pane 过窄时居中展示。
- 当前证据：现实现仅按 cursor row 选择上/下与可见行数，未把 pane 可见宽度、裁剪区、邻 pane
  覆盖与窄宽居中纳入模型。
- 目标行为：弹层完全落入当前 pane 可见 rect；优先锚定输入光标，空间不足则翻向/夹紧；宽度不足
  时在 pane 可见区居中并保持选择项可读。
- 范围：桌面 RidgePane 历史 overlay 几何、renderer 投影与键盘选择。
- 非目标：改 shell history 数据源、排序或去重语义。
- 边界：不得溢出到不可见 pane 或以 viewport 全窗代替 pane rect；滚动、键盘与焦点语义不变。
- 验收：四角光标、分屏裁剪、侧栏开合、窄 pane、DPR/缩放 fixture；断言 overlay rect 始终被
  pane visible rect 包含，窄宽时水平中心误差不超过 1 CSS px。
- 预期落点：`RidgePane.svelte`、ridge-term `HistoryOverlay` 几何与组件/renderer 测试。

### REQ-EXPLORER-FREE-RESIZE-01

- 状态:`ACTIVE`
- 版本:`v0.3.0`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 日期：`2026-07-29`
- 版本：`v0.3.0`
- 批准证据：用户授权“审核无问题即全部通过”；审计通过。
- 类型：`MODIFY` + `FIX`
- 原始意图：文件树展示域拖拽须“无拘无束”；指针可越过下方展开面板，拖动直接压缩其展示域；
  resize 跟手，空余空间自动占用。
- 当前证据：现有 `measureFreeFollowSpan`、window pointer listener、pointer capture、ResizeObserver
  与 stored-height reclamp 已发布；用户现场仍受下方面板 header/属性区阻塞。
- 目标行为：拖柄跟随整个 Explorer 可见纵轴；上区扩张时下区实时压缩至其最小可用高度，
  下区不得以自身 header/属性命中截断拖动；释放后各区自动填满可用高度，无空洞。
- 范围：桌面 Explorer cwd stack、展示域高度分配、pointer 生命周期与持久化。
- 非目标：重做全局 pane split 布局；移除面板最小高度。
- 边界：保留各面板最小可用高度、键盘/无障碍与滚动；不得无界 RAF/observer 或每帧持久化。
- 假设/待确认：“自动占用”解释为参与同一布局的可伸缩展示域按现有优先级吃满剩余空间。
- 验收：跨越下方面板 header 的连续 pointer 轨迹；每帧高度和等于容器可用高、无负值/空洞；
  60Hz 拖动无长任务，pointerup/cancel 后监听器与 dragging class 归零。
- 预期落点：`Explorer.svelte`、Explorer 高度 store 与交互/性能测试。

### REQ-EXPLORER-FILE-CONTINUITY-01

- 状态:`ACTIVE`
- 版本:`v0.3.0`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 日期：`2026-07-29`
- 版本：`v0.3.0`
- 批准证据：用户授权“审核无问题即全部通过”；审计通过。
- 类型：`MODIFY` + `FIX`
- 原始意图：已打开文件自动更新；新文件首次打开前刷新所属文件夹；复制、粘贴、移动完整可靠。
- 当前证据：fileEditor 已处理 fs-watch、clean 静默重载、dirty 冲突、图片 cache-bust；
  FileTree/Explorer 已有 copy/cut/paste/move 与跨列刷新，32 个聚焦测试通过；现场仍判“不完善”。
- 目标行为：
  - 外部改动的 clean tab 自动同步；dirty tab 明确保留/重载选择；删除/重建状态可恢复。
  - 首次打开树中节点前，先刷新其父目录并按规范路径重新解析目标；目标消失则不打开陈旧内容。
  - 单选/多选、文件/目录、同盘/跨盘、跨 cwd、冲突重命名、部分失败皆有原子可诊断结果；
    move 成功后源/目标所有可见列同步刷新。
- 范围：桌面 Explorer/FileTree、fileEditor/fileExplorer store、fs-watch 与本机文件命令。
- 非目标：自动解决用户未保存内容冲突；把 Remote/shared 绝对路径当本机路径。
- 边界：不得覆盖未保存编辑；不得信任过期树路径越出 cwd；批量失败不得静默吞项。
- 假设/待确认：用户尚未给出复制/移动的具体失败组合；关闭前须补真实复现步骤。
- 验收：fs-watch 与 dirty 冲突测试；“刷新父目录→解析→打开”顺序 spy；复制/移动矩阵覆盖冲突、
  权限拒绝、跨卷 fallback、部分失败与 refreshNonce；Windows 真文件系统冒烟。
- 预期落点：`FileTree.svelte`、`Explorer.svelte`、fileExplorer/fileEditor store 与 Rust fs 命令。

### REQ-EXPLORER-CONTEXT-ACTIONS-01

- 状态:`ACTIVE`
- 版本:`v0.3.0`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 日期：`2026-07-29`
- 版本：`v0.3.0`
- 批准证据：用户授权“审核无问题即全部通过”；审计通过。
- 类型：`FIX` + `MODIFY`
- 原始意图：cwd 节点与 pane header 右键菜单均提供复制路径、复制相对路径、在文件管理器中显示，
  且跳到正确位置。
- 当前证据：FileTree 普通节点已有两种复制；cwd 菜单缺二者；pane header 仅复制 cwd，
  且 `copyPaneCwd/revealPaneCwd` 以当前 active workspace 查目标 pane，存在跨工作区错配。
- 目标行为：动作始终绑定菜单打开时捕获的 `{workspaceId,paneId,cwd}`；绝对路径复制 canonical cwd，
  相对路径以该 workspace 根为基准；Reveal 对目录打开该目录，对文件选中该文件。
- 范围：cwd/FileTree/pane header 菜单与本机 reveal 命令。
- 非目标：让本机文件管理器打开无法映射的 Remote/shared 路径。
- 边界：不得在 action 执行时重新读取活动工作区替换捕获身份；Remote/shared 路径不冒充本机路径。
- 假设/待确认：cwd 自身相对路径以 workspace root 为基准，workspace root 本身显示 `.`。
- 验收：在 wsA pane 菜单打开后切到 wsB 再点击动作，仍操作 wsA；cwd/FileTree/pane header 菜单
  项一致；目录与文件 reveal 走正确 OS 参数；shared/remote 明确禁用或走 origin 能力。
- 预期落点：`Explorer.svelte`、`FileTree.svelte`、`+page.svelte`、reveal 命令与菜单测试。

### REQ-REMOTE-HOST-RETRY-01

- 状态:`ACTIVE`
- 版本:`v0.3.0`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 日期：`2026-07-29`
- 版本：`v0.3.0`
- 批准证据：用户授权“审核无问题即全部通过”；审计通过。
- 类型：`FIX`
- 关联：`REQ-REMOTE-HOST-TREE-01`。
- 原始意图：远端接入黑屏或 `list_workspaces` 超时时，在故障位置提供重试。
- 当前证据：Remote MainApp 有连接级 reload 重试；HostsPanel 仅顶栏全局刷新与错误文本，
  `image-3.png` 的 host/workspace RPC 超时行无就地动作。
- 目标行为：失败 host/树节点显示可点击“重试”；仅重放该 host 的认证连接与 topology 请求，
  单飞、可取消、保留最近成功树；连续失败展示首因与下一步，不黑屏。
- 范围：Hosts 树 topology 请求、错误投影、单 host 重试与连接恢复。
- 非目标：对鉴权拒绝无限重放；全页 reload 其他正常 host。
- 边界：不得无限自动重试、重复创建连接或清空其他 host；鉴权/停用类错误须引导重新登录，
  不伪装成网络重试。
- 验收：20 秒超时、断网、鉴权失败、恢复网络 fixture；单 host 重试不刷新其他 host；重复点击
  仅一请求；成功原位替换，失败仍保留错误与按钮。
- 预期落点：Hosts store/`HostsPanel.svelte`、RemoteLink topology 请求与组件/E2E。

### REQ-AGENT-INTERACTION-STATE-01

- 状态:`ACTIVE`
- 版本:`v0.3.0`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 日期：`2026-07-29`
- 版本：`v0.3.0`
- 批准证据：用户授权“审核无问题即全部通过”；审计通过。
- 类型：`MODIFY` + `FIX`
- 关联：`REQ-AGENT-01`、`REQ-AGENT-COMMUNE-CONTINUITY-01`。
- 原始意图：
  - 首次进入 Agent's Commune 不得窄折叠；无需切 tab 自愈。
  - 点击活跃 Agent，直接切到其工作区、聚焦并高亮对应 pane。
  - Agent 停止为红，等待审批为黄；Agent 卡片状态色条常驻，Pane Border 仅在需要用户介入时暂态显色，
    直至目标 pane 被用户接管或真正聚焦才清除。
  - 交互卡片实时接收动态标题。
- 当前证据：AgentMemberRow 已有等待审批黄点、运行绿点、OSC title 优先；失联用 muted，
  行本身无跨工作区导航，亦无“直到聚焦才消失”的统一暂态。
- 目标行为：状态与标题由 topology/pane tree 同一事实投影；导航使用显式 workspace/pane；
  Agent 卡片持续呈现运行/空闲语义，Pane Border 只映射需介入的暂态，聚焦或接管目标 pane 后原子确认并清除。
- 范围：桌面 Agent Center、AgentMemberRow、pane header、跨工作区聚焦与 topology 标题投影。
- 非目标：改变 Agent 进程生命周期；以颜色替代状态文本或安全审批。
- 边界：颜色不得成为唯一语义；须有文本/aria-label；轮询不变不得制造布局或标题事件风暴。
- 假设/待确认：“停止运行”映射 `Disappeared`/进程退出，不等同用户主动暂停。
- 验收：首次挂载真实宽度 fixture；wsA→点击 wsB Agent 后 active workspace/pane 唯一匹配；
  Working→Waiting→Focused 与 Working→Disappeared 状态机；卡片颜色/文本同源，Pane Border 仅在 Waiting/介入态出现并在 Focus/Claim 后清零；动态 title
  更新一次、无变化零事件。
- 预期落点：AgentCenter/AgentMemberRow、pane tree/header、`+page.svelte`、teammate model/tests。

### REQ-AGENT-HISTORY-SOURCE-02

- 状态:`ACTIVE`
- 版本:`v0.3.0`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 日期：`2026-07-29`
- 版本：`v0.3.0`
- 批准证据：用户授权“审核无问题即全部通过”；审计通过。
- 类型：`MODIFY` + `FIX`
- 关联：`REQ-AGENT-HISTORY-01`。
- 原始意图：历史须从各 Agent 最新原生 JSONL/会话源取得真实回复；一 session 一行，展示稳定标题
  与 session id；Claude/Grok 均覆盖；展示参考 Codex 桌面端作模块化、窄宽适配。
- 当前证据：后端仅扫描 Claude/Codex；前端 DTO 名为 `AgentRecentReply`，按每条回复分组/渲染，
  标题仍为“最近回复”，行展示 project/time/text，未形成 session 聚合；Grok 无 adapter。
- 目标行为：
  - adapter 独立声明 discovery/parser/title/recent-output/resume 能力；损坏一类不拖垮全页。
  - 以原生 session id 聚合，一 session 一行；标题、id、Agent、cwd、最近活动与最新真实 assistant
    回复分区展示；进行中 session 替换为成员页同款可交互卡片。
  - Claude、Codex、Grok 读取各自当前格式；无证据的 CLI 诚实禁用，不猜路径/字段。
  - 卡片在宽/窄侧栏均自适应，不因首次挂载宽度错误折叠。
- 范围：本机 Agent history adapter/DTO、Agent Center 历史与运行中交互卡片。
- 非目标：上传历史、索引全文、修改第三方 session 或伪造 resume。
- 边界：不上传历史，不修改第三方文件；不以 title/cwd 合并身份；resume 只用结构化 argv。
- 假设/待确认：“模仿 Codex 桌面端”解释为信息分区、渐进展开与响应式布局，不复制其品牌视觉。
- 验收：每 Agent 多 session/多回复/损坏/超大 JSONL fixture；最新回复与真实最后 assistant 项一致；
  session 数等于行数；标题/id 可见；运行中交集仅一交互卡；Grok 有本机或官方格式证据。
- 预期落点：project history adapter、Agent Center/history DTO、组件与真 CLI/fixture。

### REQ-AUTO-CONTRAST-RESEARCH-01

- 状态:`ACTIVE`
- 版本:`v0.3.0`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 日期：`2026-07-29`
- 版本：`v0.3.0`
- 批准证据：用户授权“审核无问题即全部通过”；审计通过。
- 类型：`NEW`（仅深研，不授权产品实现）
- 原始意图：文字与所在背景色差过低时，自动切换为可读主题色；若通用方案风险高则暂缓。
- 目标：先比较静态 design-token 对比约束、APCA/WCAG 对比选择、透明/渐变/图片背景采样与
  浏览器 forced-colors；提出最小可行且性能有界的方案。
- 范围：主题 token、普通 UI 文本与隔离视觉 fixture；终端 ANSI 面仅作风险评估。
- 非目标：本轮直接部署全局动态判色；改写第三方网页/终端内容色。
- 边界：本条仅授权研究与 fixture，不授权全局运行时逐像素监听、强改第三方终端颜色
  或牺牲语义配色。
- 假设/待确认：若静态 token 即可覆盖主要问题，则优先删去运行时检测方案。
- 停机条件：无法在不引入持续采样/布局抖动的前提下稳定判色，则归档为 deferred，不实现。
- 验收：候选方案含正确性反例、性能成本、a11y 标准、减法方案与可证伪原型。
- 预期落点：深研报告与隔离颜色纯函数/视觉 fixture；无业务代码默认。

### REQ-HEADLESS-DETECTION-02

- 状态:`ACTIVE`
- 版本:`v0.3.0`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 日期：`2026-07-29`
- 版本：`v0.3.0`
- 批准证据：用户授权“审核无问题即全部通过”；审计收紧为仅诊断，不预授权删除。
- 类型：`MODIFY`
- 关联：`REQ-AGENT-02`。
- 原始意图：确认当前无头 shell 检测为何仍失效；若可靠检测/召唤不可行，可放弃该功能。
- 当前证据：Agent Center 仅投影 `hostsStore.kind === "headless"` 的 Ridge native sessions；
  后端只承诺 Ridge 持有 PTY master 的 session 可召唤，任意脱离 PTY 的 OS PID 明确不可接管。
- 目标行为：先区分“Ridge-owned native session 未被列出”之 Bug 与“任意外部无头 shell”之
  非能力；前者修复 metadata/list/投影链，后者不得伪造可召唤。
- 范围：Ridge-owned native/tmux session 的创建、发现、归因、投影与召唤。
- 非目标：接管 Ridge 未持有 PTY master 的任意 OS PID。
- 决策点：若用户期望仅为任意外部 PID，建议移除/改名该入口；若为 Ridge-owned session，
  保留并补确定性发现与诊断。移除或改名须另建 Pending 明确批准，本条不授权删除。
- 验收：tmux shim 创建→list DTO→Agent Center 投影→召唤完整真进程链；外部 PID 明确显示
  “不可接管”或不展示；无误报。
- 预期落点：ridge-tmux/native session、terminal commands、hosts store、Agent Center。

### REQ-AGENT-COMMUNE-CONTINUITY-01

- 状态:`ACTIVE`
- 版本:`v0.2.4`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 日期：`2026-07-28`
- 类型：`MODIFY` + `FIX`
- 版本：`v0.2.4`
- 批准证据：用户明确“批准所有”。
- 原始意图：
  - Agent's Commune 面板当前位于顶部的操控按钮与文档入口区域，移至面板滚动内容最底部。
  - 已批准的“成员 / 编组 / 历史”三页签继续推进，不得因本轮 Remote 重构丢失或降级。
  - Agent Tab 已识别某 Agent 为运行中时，对应 pane header 必须同步为同一运行状态。
- 目标行为与用户可观察结果：
  - 面板顶级导航恰为“成员 / 编组 / 历史”；成员、编组、历史主体之后才显示 MCP 文档、连接信息、HITL、健康及暂停等操控区。
  - 自动发现、手工注册、暂停/恢复、退出等所有 roster 变更均驱动 pane tree 的同一 Agent 状态投影；禁止 Agent Tab 显示运行中而 pane header 仍为空闲/未知。
  - 状态同步须幂等；轮询未变化不得制造布局事件风暴或重复刷新。
- 范围：
  - `src/lib/teammate/**`、pane tree/layout 同步入口、必要的 Tauri topology DTO 与组件/单元测试。
  - `REQ-AGENT-HISTORY-01` 与 `CONTRACT-iteration-64.md` 保持有效并纳入同一大迭代。
- 非目标：
  - 改 Remote 协议；把 Agent 历史上传云端；改变第三方 CLI session 文件。
- 不可动边界：
  - pane header 与 Agent Tab 必须读取同一后端事实；不得另造第二份持久状态。
  - 控制区移底不得破坏键盘导航、滚动可达性、HITL 安全门或现有操作语义。
- 假设与待确认：
  - “展示在最底部”解释为 Agent's Commune 自身滚动内容底部，而非固定吸底悬浮层。
- 确定性验收：
  - 组件结构测试证明控制/文档区 DOM 顺序晚于三 Tab 全部内容，且键盘可达。
  - topology 自动发现由未运行变运行时，恰触发一次 pane layout 同步；无变化轮询触发零次；退出/暂停/恢复状态一致。
  - Agent Tab 与 pane header 对同一 pane/agent 的状态值逐项一致。
- 预期落点：
  - `src/lib/teammate/AgentCenterPanel.svelte`
  - `src/lib/teammate/teammateModel.ts`
  - `src/routes/+page.svelte` / pane tree 既有布局同步入口
  - 对应 Vitest/Svelte 组件测试

### REQ-REMOTE-SMOOTH-STATE-02

- 状态:`ACTIVE`
- 版本:`v0.2.5`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 日期：`2026-07-28`
- 类型：`MODIFY` + `FIX`
- 版本：`v0.2.5`
- 批准证据：用户明确“批准所有”。
- 关联：`REQ-MOBILE-REMOTE-STATE-01`、`REQ-REMOTE-03`
- 原始意图：
  - iteration 63 的手机 Remote 体验未达预期；已有“可唤起键盘”仅算部分实现。
  - 唤起键盘时完全按 terminal 光标定位，禁止受手指触点影响；光标不可解析时按屏幕/终端可见区中心处理。
  - 唤起键盘时若终端不在底部，自动滚动到底部。
  - pane 必须归属其真实 workspace，不得挂到当前激活 workspace；已访问 pane 真正在后台保活，手机与桌面浏览器切换丝滑。
  - 加载 scrollback 不得占住当前 pane 的输入/控制传输路径，尤其不得令终端无法文字输入。
- 目标行为与用户可观察结果：
  - 输入 textarea/IME 锚点只取 terminal cursor 的投影坐标；pointer/touch 坐标仅用于既有 TUI 鼠标报告，不参与键盘锚点。cursor 不可见、越界或未知时使用终端可见区中心。
  - 每次显式唤起软键盘前，当前 pane 原子滚至 live bottom，再按光标锚点聚焦；不得改变 PTY rows/cols 或底层 canvas/container 高度。
  - pane/session 身份端到端为 `(workspaceId,paneId)`；异步响应、push、scrollback、kernel、订阅及重连恢复均携带并校验 workspaceId。除首次兼容握手外，禁止以 `activeWorkspaceId` 给缺失归属的 pane 补值。
  - 已访问且仍存活的 pane 在不可见 workspace 中继续有界收流；切换只转移可见性、输入焦点与尺寸所有权，不销毁 kernel、不退订、不全量重放。切回立即呈现离开期间尾帧。
  - scrollback 请求、编码、排队与发送属于有界、可取消的低优先级工作；输入/control 与 active live raw 在调度上可抢占。一个历史页遇 WebSocket/DataChannel 背压时不得阻塞 stdin/control 接收，也不得阻塞 active live writer。
  - 快速重复滚顶、切 pane/workspace 或取消历史加载时，旧页不得提交到错误 pane；取消须释放队列容量、任务与计数。
- 范围：
  - `src/remote/**`
  - `packages/remote/src/shared/{terminal,transport}/**`
  - `src-tauri/src/remote_host_impl.rs` 及共享 Remote 协议/scrollback store 的必要收敛
  - 手机与桌面浏览器 Remote 的确定性 fixture/E2E
- 非目标：
  - 把 PTY 字节放入 Query cache；后台 pane 抢尺寸所有权；用无界缓存掩盖物理带宽不足；本轮发布。
- 不可动边界：
  - 复用同一已认证连接；是否增加内部 writer task/优先队列由深研与对抗评审决定，但不得靠第二 WebSocket/DataChannel 绕过调度缺陷。
  - 低优先级任务必须有有界队列、取消、清理与确定性计数归零；不得用固定 FPS/延时阈值冒充流畅。
  - 工作区归属不信任 UI 当前选择；服务端须校验复合身份，错误 workspace 明确拒绝。
- 假设与待确认：
  - “否则就当手指点击在屏幕中间”解释为无法取得有效 terminal cursor 时，把隐藏输入锚到终端可见区中心；不合成 TUI 鼠标点击。
  - “自动滚动到底部”只在用户显式唤起键盘时发生，不在浏览历史时被后台 viewport 变化强制拉底。
- 确定性验收：
  - 键盘纯函数/组件测：不同触点输入得到完全相同锚点；有效 cursor 精确投影；无效 cursor 回退中心；聚焦调用顺序为 `scrollToBottom → resolve cursor/fallback → focus`。
  - 复合身份竞态测：`wsA/pane1` 请求未决时切到 `wsB`，迟到的 list/push/scrollback 仍只更新 `wsA/pane1`；缺 workspaceId 的非握手消息被拒，不落入 active workspace。
  - 保活测：`ws1/A → ws1/B → ws2/C → ws1/A` 后 A/B/C 订阅与 kernel 均存续；A 离开期间尾帧切回即见，无 RIS/全量 replay；真关闭按复合身份精确清理。
  - 背压同构测：人为挂起 scrollback sink 时，stdin/control 与 active raw 仍可推进；取消历史页后低优先级队列、任务计数归零；active 最多等待一个已开始的有界低优先级分片。
  - 浏览器 E2E：手机 visualViewport fixture 与桌面浏览器均覆盖跨 workspace 快切、后台持续输出、键盘开合、非底部唤起回底、滚顶加载期间连续输入。
- 预期落点：
  - `src/remote/MainApp.svelte`
  - `src/remote/lib/TerminalCanvas.svelte`
  - `packages/remote/src/shared/terminal/manager.ts`
  - `src-tauri/src/remote_host_impl.rs`
  - Remote transport/session 既有 SSOT 与对应测试

### REQ-MOBILE-REMOTE-STATE-01 · Mobile Remote 连续状态、后台 pane 与软键盘视口

- 状态:`ACTIVE`
- 版本:`v0.2.2`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 日期：`2026-07-28`
- 类型：`MODIFY` + `FIX`
- 版本：`v0.2.2`
- 关联：`REQ-MOBILE-01`、`REQ-REMOTE-03`
- 原始意图：
  - 远端异步状态交由 Query 管理，前端交互状态交由 store 管理；刷新保留当前页面与旧数据，后台静默替换。
  - 已访问 pane 切走后仍保持连接、后台收取输出；切回即见最新内容，禁止白屏或重放卡顿。
  - 手机软键盘弹出时不得把页面推空或改变终端底层 canvas/容器 DOM 高度；恢复以 transform 与光标高度动态计算的有限可控位移。
  - 工作区/终端弹层中 pane 行右侧 Agent 与 Shell 均为无边框纯 icon；Agent 不带文案，只以图标颜色标状态。
  - 滚顶加载更早 scrollback 时须有明显状态，可用 shell 顶部加载光条；历史拼接须连续对齐，不得出现大片重复或合成空白。
- 目标行为：
  - `workspaces`、当前 workspace 的 `panes`、能力及其他远端快照以 `@tanstack/svelte-query` 为服务器状态真相；重取期间保留已成功数据，仅用 `isFetching` 表示后台刷新。WebSocket/WebRTC 推送以 `setQueryData` 合并，失效时再 `invalidateQueries`。
  - 当前 workspace/pane、侧栏、弹层、选择模式、主题等纯 UI 状态留在 Svelte store；query cache 不保存 DOM、TerminalManager/kernel 或输入焦点。
  - 手动刷新与重连不清空已渲染 workspaces/panes/terminal；请求失败保留最近成功快照并显示非阻断错误。
  - 同一 Remote 连接内所有已激活且仍存活的 pane 跨 workspace 保持有界订阅；controller 维护 session 级 visited `(workspace_id,pane_id)` Set。LAN `remote_host_impl.rs` 保留 `current_pane` 作为 Files/Git/Search 当前 cwd 上下文，`subscribe-pane` 可显式带 `workspaceId` 并以 Set 管后台流；Cloud 复用既有每-pane listener Map。`TerminalManager` kernel 即使 workspace/pane 不可见且 parked 亦持续 `feed`。切 pane 或 workspace 只变可见/焦点、当前 cwd 与尺寸所有权，不退订旧 pane、不重新全量 replay。
  - Remote 短暂重连须按 visited Set 恢复全部后台订阅，最后恢复当前 pane；断连期间有缺口者按既有有界 resync 补齐一次。不得只恢复当前 pane，亦不得为每次普通切换重放。
  - pane 真关闭仅清其订阅/kernel；workspace 真关闭清其全部 pane；Remote 断连、组件销毁清全部。普通 workspace 切换不得清订阅、listener、kernel 或输出。
  - 弱网 active QoS 不增第二 WebSocket、PeerConnection 或 DataChannel；同一已认证链路内以 active 高优先、background 低优先双队列形成逻辑保留通道。controller 经现有 `subscribe-pane` 可选 active 标记声明当前 `(workspaceId,paneId)`；发送器每发至多一个已开始的低优先帧即重查高队列，后台积压不得排在新 active 帧之前。
  - LAN 高低队列复用既有 `RAW_CHAN_CAP` 边界并 biased drain；Cloud background 仅在既有 `BUFFERED_LOW_WATERMARK` 以下准入，active 可使用其上至既有 `BUFFERED_HIGH_WATERMARK` 的保留容量。不得另造未经弱网实验校准的 magic threshold。
  - 物理带宽不足时后台订阅与 host scrollback 仍存续；低队列满仅将对应 pane 标 `dirty`，不建无界 backlog、不退订。dirty pane 重新 active 时，经同一 ordered 链路 barrier 恰一次取得有界 canonical resync；barrier 前旧 live 丢弃、snapshot 原子落入、barrier 后 live 续接。不得常态全量 replay。active 自身超过硬上限时沿用有界背压恢复，不宣称突破物理带宽。
  - 软键盘仅改变视觉投影：保持 `.term-stage`、`.container`、canvas 与 PTY rows/cols 不变；根据 `visualViewport`、终端可见区、当前光标行/行高计算 `translateY`，位移夹在 `[-maxShift, 0]`，只保证输入光标位于键盘上方且至少保留一段上下文。键盘收起归零。
  - 位移期间 pointer/touch 命中须使用变换后同一坐标原点；不得触发 PTY resize、无界 observer/轮询或连续 claim。
  - `WorkspaceTree` pane 行的 Agent/Shell 触发器均维持既有 `title`/`aria-label`、键盘语义与不缩小的透明触控命中区；视觉上仅 icon，无 border、pill 或 Agent 文案。Agent 未标记用 muted，已标记只变 accent 色；Shell 打开态亦仅变色。
  - `onNearTop` 发起历史请求后，当前 pane 顶部显示 absolute、且不改变布局的细加载光条；容器同步 `aria-busy`，光条具 indeterminate `role="progressbar"` 与可读 label。每 pane 独立 loading，成功、空页、失败或 pane 关闭皆确定性结束。快速重复触发沿用单飞，不闪多条、不叠请求。
  - Scrollback 页以单调 seq 区间拼接：transport/pager 先返回待提交页，新页须满足 `startSeq < endSeq == 请求时 oldestSeq`；仅在对应 kernel prepend 成功后 commit 新 cursor。重复、过期、倒退或不连续页不 prepend、不推进 cursor。
  - 页边界由 host 共享 scrollback store 单源选择，须落在 UTF-8 与完整终端行安全点；LAN/cloud 不各写分界。跨页的 CRLF、常规 ANSI/SGR 与宽字符不得裂成重复行或大片空白；极长无换行记录不得靠插入合成空行处理。
  - 请求期间即使切 pane/workspace，返回页仍只 prepend 至请求发起 pane 的 parked kernel；不得因“已非 active”丢页却先推进 cursor。live grid、cursor、selection 与用户正在阅读的 viewport 锚点保持不变；sandbox 页解析结束即释放。
- 范围：`src/remote/**`、`packages/remote/src/shared/{terminal,transport}/**`、必要的 LAN host 订阅语义与移动端 Vitest/Playwright fixture；`WorkspaceTree.svelte`、`PaneShellPicker.svelte`。
- 非目标：迁移 PTY 字节流进 Query cache；让后台 pane 抢占 PTY 尺寸所有权；改桌面本机 pane；新增协议副本；本轮发布。
- 不可动边界：
  - Query 只管可序列化远端服务器状态；高频 PTY bytes 直达既有 kernel。
  - 后台 pane 数受该 Remote 连接已访问 workspace 的 live pane 并集约束，kernel scrollback 沿用 `DEFAULT_SCROLLBACK = 5000` 硬上限；普通 workspace 切换不清，pane/workspace 真关闭或连接销毁后不得遗留订阅、listener 或 kernel。不得另造无运行证据的字节/pane 数魔法阈值。
  - 不得以第二物理连接/DataChannel、pane 数、固定时长、FPS 或内存启发式换取 active 流畅；后台洪泛不得占用 active 保留容量。物理带宽不足时只允许 per-pane dirty + 有界恢复。
  - 仅当前可见 pane 拥有尺寸；后台 parked pane 保持最后 grid，不随不可见布局 resize、不发送 claim。切回时以当前容器 fit/claim，禁止以全量 replay 换取尺寸更新。
  - 键盘修复不得改变 canvas/terminal container DOM 高度或 host PTY rows/cols。
  - 历史 loading 仅视觉状态，不改 terminal 高度；空页为 no-op。分页不得用内容字符串去重替代 seq 区间真相。
- 假设与待确认：
  - Query 首版覆盖 workspaces/panes/capabilities；文件、Git、Agent 各自既有 provider 暂不整体迁移，避免大爆炸重构。
- 确定性验收：
  - Query 测：刷新/推送/短暂失败时旧 workspaces/panes 不归零；成功后原位替换；重复推送不生重复 ID。
  - pane 测：LAN/cloud 中 `ws1/A → ws1/B → ws2/C → ws1/A` 后 A/B/C 均保持订阅，parked kernel 继续收字节且各自 scrollback 不越 5000 行；切回 A 无新增全量 replay且立见离开期间尾帧。关闭 A 仅清 A；关闭 ws1 清 A/B；普通 workspace 切换集合不减；短暂重连恢复 A/B/C 恰一次且当前 pane 最后恢复；终止连接后集合归零。
  - 弱网 QoS 测：LAN/Cloud 多 background pane 洪泛时，active 输出/控制最多等待一个已开始的低优先帧；低队列有界，background 不令 active dirty。background dirty 后仍订阅；切为 active 恰一次 barrier 恢复，无重复、空洞或 RIS 循环。
  - 尺寸测：后台 A 收流期间不 claim/resize；切回 A 仅按当前可见容器发最终一次有界 claim，kernel 内容与尾帧不清空。
  - 几何测：多组 `innerHeight/visualViewport/offsetTop/cursorRow/cellHeight` 输入皆得有界 transform；键盘开合前后 DOM 高度、grid rows/cols 与 PTY claim 次数不变。
  - 移动浏览器 E2E：唤起真实输入键盘等价 visualViewport fixture 后页面不空、光标可见、上下文保留、点击命中；快速 A→B→A 切 pane 后输出连续且无白屏。
  - UI 测：Agent/Shell trigger 皆为纯 icon，无 border/pill/background；Agent 无文字节点，off/on 仅颜色状态有别；现有透明命中区不缩小，动态 `title`/`aria-label` 保留。
  - Scrollback 测：单飞 loading 光条、`aria-busy/progressbar` 开/关；A 请求后切至 B，页仍入 A 且成功后才推进 cursor；过期/重叠/空洞页皆拒且 cursor 不动；相邻两页只拼一次。fixture 覆盖页界邻近 CRLF、UTF-8 宽字符、ANSI/SGR 及长行，断言无重复行、无新增空白 slab、live grid/cursor/selection/viewport 锚点不变。
- 预期落点：`src/remote/MainApp.svelte`、`src/remote/lib/{TerminalCanvas,keyboardOffset,WorkspaceTree,PaneShellPicker}*`、新增最小 remote query/store、`packages/remote/src/shared/terminal/manager.ts`、`src-tauri/src/remote_host_impl.rs` 及对应测试。

### REQ-AGENT-HISTORY-01 · Agent 历史会话分组、折叠与一键恢复

- 状态:`ACTIVE`
- 版本:`v0.2.3`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`APPROVED`
- 日期：`2026-07-28`
- 类型：`MODIFY` + `NEW`
- 批准证据：用户明确要求“添加任务并按 NLM 流程自动审批通过”。
- 原始意图：
  - Agent 侧栏顶级导航统一为“成员 / 编组 / 历史”；现“最近回复”改为“历史会话”。
  - 历史会话按 Agent 类型分组；各组可独立折叠/展开，并提供全部折叠/展开。
  - 每条列 session 标题及恢复按钮；点击后在当前工作区新建 pane，以该会话原启动参数恢复。
  - 扩展 MiMo、OpenCode、Grok 及中国主流 Agent CLI 的运行中识别与历史会话发现。
- 目标行为：
  - “历史”只投影已发现 session，不复制 session 内容为第二状态源；分组键来自统一 Agent adapter/identity，未知类型进入“其他”，不得因单个解析器失败拖垮整页。
  - 组头显示 Agent 名与会话数；组级折叠互不影响，页级动作可一次折叠或展开全部。折叠态属本地 UI store，不写回 session 文件。
  - 会话行至少展示稳定 session 标题、Agent、最近活动时间与来源工作目录；无原生标题时采用确定性回退，不显示大段最近回复冒充标题。
  - 若历史 session 当前正在 Ridge pane/native session 中运行，历史页不再渲染静态历史行，改为复用“成员/编组”同款可交互 Agent 项；保留其切 pane、暂停/恢复、状态与既有安全门禁。优先以 adapter 原生 session ID 关联；无稳定 ID 时不得仅凭标题或 cwd 猜测合并。
  - “恢复”在当前工作区新建 pane，不占用或改写现有 pane；以结构化 executable + argv + cwd 恢复同一 session。须复用该 Agent adapter 的启动/恢复参数生成器，不拼接未经转义的 shell 字符串。
  - 恢复失败须保留原 session 与新 pane 可诊断状态；不得删除、迁移或篡改历史文件。重复点击须有单飞/禁用态，避免同一操作误生多个 pane。
  - 识别与恢复能力分离：能识别但尚无可靠 resume 协议者仍可列历史，但恢复按钮明确禁用并说明原因，不猜测参数。
  - 首批显式覆盖 MiMo、OpenCode、Grok；中国主流 CLI 名单及其 session 路径、进程特征、resume 参数须经本机/官方证据进入 adapter registry，禁止仅按模糊进程名误判。
- 范围：桌面 Agent 侧栏、Agent roster/session discovery、pane 创建与 Agent CLI adapter；必要的 session fixture、组件测试与真进程 E2E。
- 非目标：把历史会话上传云端；在旧 pane 内强行接管进程；伪造无原生恢复能力的 CLI resume；修改第三方 session 文件格式。
- 确定性验收：
  - 顶级页签恰为“成员 / 编组 / 历史”；“最近回复”入口与标题消失。
  - 至少三类 Agent fixture 混排时按类型准确分组；单组折叠、全部折叠、全部展开状态确定，键盘与辅助名称完备。
  - 同一 session 在历史与运行中集合相交时仅出现一个同款可交互 Agent 项；退出后确定性退回普通历史行；同标题/同 cwd 不同 session 不得误合并。
  - 每条显示稳定标题；缺标题、损坏 session、未知 Agent 皆有确定性回退且不泄露大段正文/凭据。
  - 对每个宣称可恢复的 adapter，点击后当前工作区恰新增一个 pane；捕获的 executable、argv、cwd 与该 session 原启动/恢复契约一致，session ID 无丢失、无 shell 注入。
  - MiMo、OpenCode、Grok 与经证据纳入的中国主流 CLI 均有“运行中识别 + 历史发现”fixture；有 resume 能力者再具真 CLI/等价进程 E2E。
- NLM 路由：R63 完成后进入下一轮状态刷新、来源替换、规划与对抗评审；本条已获批准，无 Pending 闸。

### REQ-REMOTE-HOST-TREE-01 · 公网同账号 / LAN 直连主机与三层拓扑

- 状态:`ACTIVE`
- 版本:`v0.2.0`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 类型：`FIX`
- 原始意图：接入公网或局域网远端主机后，读取现有 pane，以“主机 → 工作区 → pane”树展示并管理。
- 目标行为：
  - 公网 cloud “接入主机”仅允许同一 Ridge 账户；LAN 不限制账户归属，沿用 LAN 自身的 TOTP/session/E2EE 认证。
  - 主机菜单：刷新、添加工作区、断开/忘记；工作区菜单：打开、添加 pane、重命名、保存、分享、关闭；pane 菜单：接入/聚焦、标记 Agent、切 shell、删除。
  - “删除 pane”仅删除目标远端 pane/PTY，须二次确认；不得连带删除 workspace 或其他 pane。
  - 删除成功后，以该控制端当前接入的同源 host pane 为引用计数：若为 `0`，断开该控制端与 host 的连接；若仍有至少一个 pane，保持连接。删除失败不减计数、不触发断开。
  - 菜单逐项受 host capability 与调用者 scope 门控；不支持项不展示。
- 范围：桌面 Hosts 侧栏、LAN/cloud controller 连接、host topology DTO、远程 workspace/pane CRUD 与确定性测试。
- 非目标：跨主机拖拽 pane、远端 pane 像素预览、绕过 LAN TOTP/E2EE、让不同账户经公网 cloud 取得整机视图。
- 不可动边界：relay 不见 PTY 明文；公网同账号整机门禁由 cloud 与 host 双重校验；LAN 不以 cloud 账户关系作门禁。
- 确定性验收：
  - 两台 host fixture 各含多 workspace/pane，树投影隔离且层级正确。
  - 公网同账号 token 可列全树；异账号普通 user token 在 relay 与 host 两端皆拒绝。
  - LAN 不要求账户匹配；有效 TOTP/session fixture 可接入，无效 LAN 凭据仍拒绝。
  - 菜单动作路由到选定 host/workspace/pane，跨 host 污染为零；删后尚有第二个同源接入 pane 时连接保留，删掉该控制端最后一个同源接入 pane 时仅断开一次。
- 预期落点：`packages/remote/**`、`src-tauri/src/hosts/**`、`src/lib/stores/hosts.ts`、`src/lib/components/hosts/**`、`ridge-cloud/src/ws/**`。

### REQ-WORKSPACE-SHARE-01 · 跨账号单工作区分享

- 状态:`ACTIVE`
- 版本:`v0.2.0`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 类型：`NEW`
- 原始意图：用户可只分享一个工作区；它区别于接入整台主机，可授权给不同 Ridge 用户，并统一显示在主机侧栏。
- 目标行为：
  - owner 从工作区菜单创建/撤销邀请；受邀用户登录后，只见该工作区及其 pane，不得枚举同 host 其他工作区、设备或会话。
  - 共享工作区打开时，其每个 pane 的 `cwd` 与本机 `cwd` 同层、同样式出现在资源管理器；可展开目录并打开文件。数据从 origin host 的同一 scoped provider 读取，不把远端绝对路径当成本机路径，亦不投影成本机资源。
  - 共享工作区须同时显示该区对应的 Git 管理与 Agent tab；Terminal、Explorer、Git、Agent 皆绑定同一 `grant_id + workspace_id`。
  - 角色两级：`viewer` 可读 pane 输出、文件树/文件、Git 状态/差异/历史、Agent roster/状态/最近回复；`operator` 另可 stdin、resize、pane CRUD/切 shell、文件写入、既有 Git 写操作与既有 Agent 操作。危险写仍沿用 HITL。
  - Explorer 写权限与主机 tab 同口径：`viewer` 可打开文件但不可保存、新建、重命名、删除或粘贴；`operator` 可用现有 Remote 文件能力。UI 收敛写入口，origin host 仍须逐请求拒绝越权。
  - 若首版无法可靠贯通 `viewer` 只读门禁，桌面端可仅提供 `operator`，其体验等同正常 Remote、但范围仍限获授 workspace；不得提供名为 `viewer`、实可写入的伪只读角色。
  - 二者均不得添加/关闭工作区、转分享、改 host 设置，亦不得在控制端借 Remote/Hosts 功能二次转发、导出或分享该资源。
  - Hosts 侧栏统一投影：整机接入显示普通 host 全树；分享显示“共享：工作区名 · owner/host”受限根节点，只含一个 workspace。
  - Ridge 桌面 owner 入口共用一个 `WorkspaceShareDialog`：本机工作区在 Explorer 工作区标题 `⋯ → 分享工作区` 与 WorkspaceTree 工作区行 `⋯ → 分享工作区`；已接入主机则在 Hosts 侧栏 owner 工作区 `⋯ → 分享工作区`。共享投影自身不显示分享入口。
  - 分享邀请替代 TOTP 知识传递；controller 取得短期 `workspace_share` capability token，relay 与 host 依据 `grant_id + workspace_id + role` 双重门控。
  - ridge-cloud 负责 grant/invite 持久化、账号解析、owner/device/workspace 绑定校验、短期 scoped token、WS room 路由、邀请/撤销事件与到期/撤销踢线；不读取文件、Git、Agent 或 PTY 明文。桌面 host 负责 workspace 存在性与逐 RPC/事件授权。
- 范围：ridge-cloud grant/invite 数据模型与 API、scoped JWT、WS room 入场、host 每-controller scope、Remote RPC/事件过滤、Hosts 树与权限菜单、Explorer/Git/Agent scoped adapter、撤销/过期与非转授门禁。
- 非目标：匿名公开链接、无账户访客、转分享、分享整个 host、跨 workspace 文件访问、首版 LAN P2P 优化。
- 不可动边界：
  - grant 绑定 `owner_user_id + device_id + workspace_id + grantee_user_id`；短 token 不作为数据库真相源。
  - host 不信任 relay 单点裁决；每条 RPC、pane 订阅、事件广播均按 controller scope 再校验。
  - 首版 workspace share 走 cloud relay；即使双方同 LAN，亦先完成 cloud 身份/授权，不降级匿名直连。
  - 文件/搜索路径由 origin host canonicalize 后强制位于该 workspace root；symlink、`..`、绝对路径不得越界。Git repo 亦须属于该 root。
  - shared workspace 只存 remote projection，禁止写入控制端 `AppState.workspaces`、host export inventory 或任一 Remote host room；capability token 固定 `delegable=false`。
- 假设/待确认：
  - 优先交付 `viewer/operator`，默认 `viewer`；若只读资源面无法可靠闭环，则首版只开放 `operator`，后续补 `viewer`。
  - 邀请按 Ridge username/email 定向，不提供 bearer link；默认 7 天待接受、已接受 grant 可设到期或永久。
  - owner 关闭 workspace 或撤销 grant 时，服务端立刻踢出对应 controller，host 清订阅。
- 确定性验收：
  - grantee A 仅能列获授 workspace；访问 sibling workspace、创建 workspace、转分享均返回稳定 `SCOPE_DENIED`。
  - viewer 可见该区 Explorer/Git/Agent 读面；写/改操作全拒；operator 仅获该区既有写白名单；owner 原能力不回归。
  - 共享 cwd 与本机 cwd 同层渲染且文件可打开；viewer 保存/新建/重命名/删除/粘贴皆拒，operator 写入仅落 origin workspace root。若采用 operator-only 降级，UI/API 均不得出现 viewer。
  - 路径遍历、workspace 外 symlink、跨 repo Git、跨 workspace Agent 操作皆稳定拒绝。
  - 控制端启用自身 LAN/public Remote 或 Hosts 服务后，shared workspace 不出现在其任何 host topology；使用 share token 充当 host/controller owner 皆拒绝。
  - grant 撤销/过期后既有连接被踢、重连失败；token 重放不恢复权限。
  - UI 同屏展示同账号 full-host 与跨账号 shared-workspace，菜单随 role 精确收敛。
  - 桌面 Explorer、WorkspaceTree、Hosts owner 工作区三入口打开同一分享对话框；同一 grant 的创建、改角色、撤销结果一致。
- 预期落点：`ridge-cloud/migrations/**`、`ridge-cloud/src/{api,auth,db,ws}/**`、`packages/remote/src/shared/cloud/**`、`src/lib/components/hosts/**`、`src/lib/stores/hosts.ts`、host bridge policy/tests。

### REQ-WORKSPACE-SAVED-01 · 已保存工作区可重开、可删除及滚动条统一

- 状态:`ACTIVE`
- 版本:`v0.2.0`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 类型：`FIX`
- 原始意图：已保存工作区打开并使用、关闭后须能再次载入；已保存列表须能删除文件，滚动条须与应用一致。
- 行为：
  - 关闭工作区须同时清理其 pane 的前端 PTY bridge、terminal kernel 与标题等 runtime；再次打开同一 `.ridge` 内复用的 pane UUID 时必须创建新 PTY，不得误复用 parked runtime。
  - “已保存工作区”弹层可删除默认保存目录内的 `.ridge`，无论该文件是否仍关联已打开工作区；删除前确认，成功后原位刷新列表。
  - 弹层滚动区复用应用通用 `rg-scroll` 样式。
- 边界：按路径删除仅允许默认 `~/ridge-workspaces/` 的直接 `.ridge` 文件；拒绝目录外、子目录、其他扩展名及 symlink 越界；不删除工作区 cwd 内项目文件。
- 验收：前端测证明关闭两 pane 工作区逐一 teardown/detach；Rust 测证明仅默认目录直接 `.ridge` 可删；`svelte-check`、Rust check 通过。
- 追踪：`src/lib/stores/paneTree.ts`、`src/routes/+page.svelte`、`src-tauri/src/commands/ridge_file.rs`、`src/app.css`。

### REQ-REMOTE-03 · 桌面浏览器 Remote pane 尺寸与指针命中一致

- 状态:`ACTIVE`
- 版本:`v0.2.0`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 类型：`FIX`
- 原始意图：桌面浏览器经 LAN 或公网 Remote 时，pane 展示尺寸、终端实际 rows/cols 与鼠标点击位置须一致。
- 行为：
  - LAN 与公网共用一套 pane 几何真相：以最终内容区 `getBoundingClientRect()`、实际 padding、cell 尺寸与 `devicePixelRatio` 计算 viewport、rows/cols、resize/claim。
  - 鼠标/触控坐标须用同一内容区原点与缩放换算映射至终端 cell；不得混用 CSS 像素、设备像素、窗口坐标或旧布局缓存。
  - 首次挂载、分屏拖拽、侧栏变化、浏览器 resize、DPR/缩放变化及重连后均重算；LAN/public transport 不各写几何算法。
- 范围：桌面浏览器 Remote 的 terminal container、共享 renderer/manager、resize/claim 协议与鼠标编码；LAN/public 回归测。
- 非目标：改动原生桌面本机 pane 产品尺寸、重做终端渲染器、以 CSS transform 掩盖后端 rows/cols 错误。
- 不可动边界：几何 SSOT 位于共享 terminal 层；发送给 host 的 rows/cols 与本地 renderer grid 必须同源；修复不得增加无界 ResizeObserver/轮询或 resize 风暴。
- 验收：
  - 纯函数测覆盖 DPR `1/1.25/1.5/2`、padding、分数像素、边界点击，断言 viewport→grid 与 pointer→cell 同源且 clamp 正确。
  - 浏览器 E2E 分别走 LAN/public fixture；初始、分屏拖拽、侧栏开合、窗口缩放后，DOM pane rect、renderer viewport、host rows/cols 一致，鼠标报告 cell 与点击目标一致。
  - resize 合并有界、最终尺寸必达；不得以截图视觉近似代替协议断言。
- 预期落点：`src/remote/**`、`packages/remote/src/shared/terminal/**`、`src/lib/components/RidgePane.svelte`、LAN/cloud Remote resize 与鼠标协议测试。

### REQ-REMOTE-01 · rdg Remote 入口与启停语义

- 状态:`ACTIVE`
- 版本:`v0.2.0`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 行为：rdg 仪表盘 LAN 地址只显示根地址（如 `https://172.21.130.235:9527`），不附 `/login`；LAN Remote 默认停止，须用户显式启动；TUI 内须有名称明确的公网 Remote 启动/停止入口。
- 边界：不改变 `rdg remote` 子命令兼容性；退出 TUI 时仍须回收由本 TUI 启动的服务。
- 验收：dashboard 单测证明根 URL、初始 stopped、无启动 action；菜单文本与动作测试证明 LAN/公网入口明确。
- 追踪：`packages/ridge-cli/src/tui/dashboard.rs` → dashboard tests。

### REQ-REMOTE-02 · rdg LAN 桌面浏览器真正接入

- 状态:`ACTIVE`
- 版本:`v0.2.0`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 行为：桌面浏览器访问 rdg LAN Remote 时直接走 LAN TOTP/session 启动链，完成 WebSocket、workspace、pane、PTY 接入；不得只显示空白桌面壳。
- 边界：公网租户域仍走 Cloud WebRTC/E2EE；LAN 与 Cloud 启动判定不得依赖远端 cloud API 成败。
- 验收：启动判定纯逻辑测试覆盖 LAN IP/localhost 与 cloud 租户/query；LAN host 协议 probe 或等价集成测试证明握手、订阅、stdin 回显。
- 追踪：`src/routes/+layout.svelte`、新增启动判定 helper/tests、`packages/ridge-cli/src/tui/lan_host*`。

### REQ-CLOUD-01 · 公网 Remote 设备配额不得误停 rdg

- 状态:`ACTIVE`
- 版本:`v0.2.0`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 行为：host 与 controller 均按数据库实时用户组计算设备配额；配额自动停用与用户手动停用须分因记录；额度恢复时仅自动启用“因配额停用”的设备。
- 边界：不绕过会员 Remote 权限、设备归属、WS 并发或 controller 数量门禁；手动停用不可被后台 daemon 自动撤销。
- 验收：ridge-cloud 单测覆盖会员 host 不按免费额度降级、quota/manual 两种停用区分、额度恢复只恢复 quota-parked；WS 门控回归全绿。
- 追踪：`ridge-cloud/src/ws/handler.rs`、`src/db/device_quota.rs`、`src/db/device_repo.rs`、顺序 migration。

### REQ-MOBILE-01 · Mobile Remote 弹层、图标与按钮

- 状态:`ACTIVE`
- 版本:`v0.2.0`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 行为：工作区/终端类型切换层通过 portal 挂到 `body`；顶部 Agent 协作入口使用小机器人；Agent 图标后不跟“标记/运行中/启动中”等标记文案；手机 Remote 右侧功能按钮不显示 border 外壳。
- 边界：保留按钮的 `title`/`aria-label`、触控尺寸、焦点与点击行为。
- 验收：Svelte/Vitest 断言 portal action、Bot 图标及无标记文案；移动构建与 svelte-check 全绿。
- 追踪：`src/remote/lib/WorkspaceTree.svelte`、`src/remote/MainApp.svelte`、`src/remote/lib/RemoteSidebar.svelte`、`src/lib/components/SplitContainer.svelte`。

### REQ-AGENT-01 · 全局 Agent Center、pane 状态与最近回复

- 状态:`ACTIVE`
- 版本:`v0.2.0`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 行为：Agent tab 聚合所有已打开工作区的 agents，不以焦点工作区过滤；原顶部操作移入可滚动内容；自动识别的 agent 进程须同步 pane header 与 roster 状态；最近回复从 Claude/Codex JSONL 会话历史提取并显示。
- 边界：工作区目标、编组编辑等写操作仍须显式落到所属工作区；JSONL 扫描须有文件数、单文件读取量与返回条数上限，不上传会话内容。
- 验收：聚合模型与进程识别单测；Rust JSONL fixture 测试覆盖 Claude/Codex assistant 文本、项目过滤与有界排序；pane header UI 不显示尾随标记文案。
- 追踪：`src/lib/teammate/AgentCenterPanel.svelte`、`src/lib/components/RidgePane.svelte`、`src-tauri/src/commands/project.rs`。

### REQ-AGENT-02 · Agent 启动的无头 Shell 发现与唤起

- 状态:`ACTIVE`
- 版本:`v0.2.0`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 行为：经 Ridge tmux shim 由 pane/agent 启动的 native 无头 session 记录创建工作区与 pane；Agent Center 自动列入对应 agent，支持一键召唤到当前工作区；未能归因的普通无头 session 仍可单列。
- 边界：仅承诺 Ridge 持有 PTY master 的 native session 可召唤；任意已脱离 PTY、仅剩 OS PID 的后台进程不可伪装成可接管会话。
- 验收：ridge-tmux 测试证明 creator metadata 从 HTTP header 入 session/list DTO；tmux shim 测试证明工作区/pane header 传播；前端测试证明按 agent 归组与 attach 调用。
- 追踪：`src-tauri/src/bin/tmux.rs`、`src-tauri/src/commands/terminal.rs`、`packages/ridge-tmux/src/{http.rs,lib.rs}`、`src/lib/stores/hosts.ts`、`AgentCenterPanel.svelte`。

### REQ-AGENT-COMMUNE-MCP-SUBMIT-03

- 状态:`ACTIVE`
- 版本:`v0.3.1`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

- 状态：`ACTIVE`
- 日期：`2026-07-29`
- 版本：`v0.3.1`
- 批准证据：用户明确“批准”，要求载入本迭代并修复。
- 类型：`FIX`
- 关联：`REQ-AGENT-COMMUNE-CONTINUITY-01`；取代 `CONTRACT-iteration-65.md`
  中 `ridge_send_to_teammate` 默认仅注入草稿的旧语义。
- 原始意图：Ridge Agents Commune MCP 的发送方法须让目标 Agent 真正收到提交；不得只把提示词
  留在目标输入框。
- 根因证据：共享 MCP 将 `ridge_send_to_teammate` 默认映射为 `submit=false`；
  旧 HTTP `delegate-task` 与 `send-keys` 提交路径另使用 LF。Claude/Codex raw-mode TUI
  需 CR 表示 Enter，LF 只会留在编辑框。
- 目标行为：
  - `ridge_send_to_teammate` 默认写入并提交；仅显式 `submit:false` 时注入草稿。
  - `ridge_send_and_submit`、`ridge_delegate_task` 继续强制提交。
  - 所有提交型 MCP/HTTP/tmux 路径统一去除尾随 CR/LF 后追加单一 CR。
  - 回执只声明 `submit_dispatched`、`terminalAccepted`；未经目标 Agent 明确确认，
    `agentAcknowledged` 仍为 false。
- 边界：不启动、终止或干预宿主 Ridge；不以 PTY 接受写入冒充 Agent 已执行；保留显式草稿能力。
- 验收：Rust 测证明默认 send 进入提交态、显式草稿仍不提交、Enter 字节恒为 CR 无 LF；
  legacy delegate/send-keys 复用同一规范化函数；相关 crate 测试与 `git diff --check` 通过。
- 预期落点：`packages/ridge-mcp/src/{registry,server}.rs`、
  `src-tauri/src/teammate/server.rs`。

### REQ-MOBILE-REMOTE-WORKER-AUTHORITY-01

- 状态：`ACTIVE`
- 日期：`2026-07-30`
- 版本：`v0.3.3`
- 批准证据：用户批准将 Mobile Remote 的 Web Worker、pane 切换与输入链路问题纳入下一迭代并推进。
- 类型：`MODIFY` + `PERFORMANCE`
- 原始意图：Remote 渲染移出主线程；pane 切换丝滑，后台 pane 持续消费输出，切回不白屏、不跳帧。
- 目标行为：支持 `Worker + OffscreenCanvas` 时，单例 render Worker 为唯一可见 painter；主线程 kernel 仅保留输入语义与故障回退。pane 停放仅释放 canvas，kernel 与订阅继续；真实关闭方销毁。
- 范围：共享 terminal manager、render Worker、host bridge、生命周期与故障回退。
- 非目标：第二条 WebSocket/DataChannel、第二份 pane cache、重写 Rust renderer。
- 边界：请求须有界并具超时；Worker 崩溃、超时或 bind 失败后 pending 归零，恢复主线程且不丢当前 pane 内容。
- 验收：真实 Worker `init → feed/apply → bind → resize → park/unpark → destroy`；仅一个 painter；`A → B → C → A` 保留状态；失败回退后 pending/worker 计数归零。
- 预期落点：`packages/remote/src/shared/terminal/{manager,renderWorker,workerHostedRenderer,workerRendererBridge,workerRendererSingleton}*`。


- 状态:`ACTIVE`
- 版本:`v0.3.3`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

### REQ-MOBILE-REMOTE-INPUT-FEEDBACK-01

- 状态：`ACTIVE`
- 日期：`2026-07-30`
- 版本：`v0.3.3`
- 批准证据：用户批准将唤起键盘滚底、输入框置顶、输入高速健壮性及相邻 UI 问题纳入迭代。
- 类型：`MODIFY` + `FIX`
- 原始意图：触屏输入、系统 IME、虚拟键盘及弱网反馈须相互独立，输入路径保持低延迟、可恢复。
- 目标行为：系统 IME 唤起固定执行 `scrollToBottom → cursor/fallback anchor → focus`；输入锚点仅取 cursor/fallback，禁用 pointer 坐标；虚拟键盘显示与系统 IME 聚焦分离；scrollback 错误属 pane 本地并可重试。
- 范围：Mobile Remote 终端输入、键盘偏移、scrollback retry、脏/重同步状态反馈。
- 非目标：改变 PTY rows/cols、凭空新增 freshness 状态源、绝对延迟/FPS 承诺。
- 边界：stdin/control 与 active raw 不得被 scrollback 或后台输出阻塞；重试按钮事件不得穿透终端；挂起与取消后队列归零。
- 验收：IME 顺序测试、键盘偏移测试、弱网优先级/取消测试、pane-local retry 测试及 Dev CDP 实测通过。
- 预期落点：`src/remote/lib/TerminalCanvas.svelte`、共享 terminal/input helpers 与相关测试。


- 状态:`ACTIVE`
- 版本:`v0.3.3`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

### REQ-AGENT-COMMUNE-LAUNCH-PROFILE-01

- 状态：`ACTIVE`
- 日期：`2026-07-30`
- 版本：`v0.3.4`
- 批准证据：用户确认 MCP 需求须补充跨工作区创建及发送消息给 Agent，并批准继续。
- 类型：`ADD` + `FIX`
- 原始意图：Ridge Agents Commune MCP 能安全创建指定类型 Agent，跨工作区定位并真正提交消息；长任务可把 checkpoint 交给新 pane 后停止旧 Worker。
- 目标行为：
  - 提供可发现、受控的 `launch_profile` 与 capability；model/reasoning effort 仅能取 profile 明示允许值。
  - `initial_cmd` 与结构化 profile 互斥；命令使用安全 argv，不经 shell 拼接提示词。
  - 枚举 workspace/Agent，并以 `(workspaceId,paneId/agentId)` 复合身份创建、定位与发送。
  - 仅省略 `workspaceId` 时保留当前工作区兼容语义；显式身份不匹配、伪造或歧义须失败关闭。
  - profile 支持时可传递 checkpoint；新 pane 成功后再停止精确指定的旧 Worker。
- 范围：`ridge-mcp` 工具注册/协议、桌面 `McpHost`、workspace/pane 解析与 launch argv。
- 非目标：猜测宿主未声明的模型清单；用 shell/剪贴板模拟 Agent 提交；跨工作区隐式搜索同名 pane。
- 验收：capability gating；跨工作区 create/send 仅到目标复合身份；raw string/number 目标规范化；checkpoint 替换先新后旧；CR 提交链路与拒绝路径均有确定性测。
- 预期落点：`packages/ridge-mcp/src/{registry,server}.rs`、`src-tauri/src/teammate/mcp.rs`。


- 状态:`ACTIVE`
- 版本:`v0.3.4`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

### REQ-RIDGE-MCP-INSTALLER-01

- 状态：`ACTIVE`
- 日期：`2026-07-30`
- 版本：`v0.3.4`
- 批准证据：用户批准本迭代需求，并授权 Dev 验收、提交推送与全量发布。
- 类型：`ADD`
- 原始意图：用户安装 Ridge 后即可稳定注册 `ridge-mcp`，无需另装仓库工具或保存易失 endpoint/token。
- 目标行为：所有桌面安装包捆绑与版本一致的 `ridge-mcp` companion；路径、执行权限、签名、升级和卸载跟随 Ridge；提供 `--print-config` 输出不含 endpoint/token 的 stdio 配置；运行时发现/轮换 endpoint 后重试。
- 范围：bridge CLI、Tauri external binary、构建脚本、release matrix 与 MCP 集成文档。
- 非目标：把短期端口/token 写入配置；以空 tag 或无资产 Release 代替安装包。
- 边界：构建、检查外部进程均须具墙钟超时；目标 triple 与包版本须匹配；缺 companion 时打包失败。
- 验收：bridge 测试；sidecar `--check --require-built`；clean-machine 包体检查；endpoint rotation；各平台 release job 校验 companion。
- 预期落点：`packages/ridge-mcp-bridge/**`、`scripts/build-ridge-mcp-sidecar.mjs`、`src-tauri/tauri.conf.json`、`.github/workflows/release.yml`。


- 状态:`ACTIVE`
- 版本:`v0.3.4`
- 行为:见本条既有原始意图与目标行为。
- 边界:见本条既有范围、非目标及不可动边界。
- 验收:见本条既有确定性验收与用户可观察结果。
- 追踪:见本条既有预期落点或追踪。

### REQ-20260730-01 · 下一迭代：Remote、桌面端与跨窗口稳定性重构

- 批准依据:`全部批准，现在立刻快速推进。可以使用subagent 或者 用ridge-mcp启动队友高速迭代`
- 状态:`ACTIVE`
- 版本:`v1.1`
- 行为:拉取并冻结最新代码基线，完整纳入最近两次项目 NLM 对话、上一迭代半成品/未完成/回归项及本次 Remote/桌面稳定性与性能需求；建立三源去重追踪后，按依赖、风险、优先级与可验证性逐轮推进。其中 `REQ-RDG-REMOTE-CONNECT-01`（rdg 公网/LAN × 桌面/手机浏览器四路径真接通）为本 goal 内 P0 硬闸，与其余项并行推进、不得因本条而暂停 goal 其余交付；四格冒烟未过不得宣称 Remote 可用。须消除 Remote 公网高频重复 RPC、超时队列、非 Git 轮询、销毁 Pane 残留请求与重复日志；约束桌面终端内存和 Scrollback，统一真正清空语义；补齐 Agent's Commune 可见性核查、桌面多窗口与 Remote 工作区单例、远程 Host 接入/反馈/拖拽/Resize 全链路。
- 边界:保留用户工作与 Git 历史；不自动发布、不推送、不删除用户数据、不作无关重构；NotebookLM 只供战略建议，不裁决代码事实；窗口允许多开但 Remote 工作区跨窗口全局单例；终端输入不得丢字节、乱序或改变交互语义；外部进程统一经具墙钟超时、取消、Windows 进程树清理及同生死许可的出口；低优先级浏览器 Warning 不阻塞核心交付。
- 验收:① 三源矩阵逐项标注已完成/半完成/未完成/回归并附代码、提交/状态和测试证据；② 迭代任务逐项列现状、目标、方案、验收、回归、依赖、风险、优先级、顺序；③ 同场景 Console 重复错误较基线减少至少 90%，同错聚合计数；④ 非 Git 目录每目录生命周期至多探测一次，确认后停 status/branch/stash 轮询，目录切换才重检；⑤ 同键幂等 RPC 未完成时合并，Resize/Input 调度有界且输入不丢不乱，超时指数退避、暂停阈值、队列上限、统计均有确定性测试；⑥ Pane Destroy 后 Pending RPC/计数归零，陈旧 Pane fail-closed，真挂起替身验证超时/取消后进程树回收；⑦ Scrollback 有明确上限和自动释放测试，右键清空及 clear 均清页面、Scrollback、后台缓冲；⑧ 多窗口并存且 Remote 工作区全局单例，重复打开聚焦旧窗口；⑨ 远程 Host 可列出/创建/拖拽接入，弹窗立即关闭、面板反馈阶段，接入后按实际尺寸自动 Resize，按钮可重同步；⑩ Agent's Commune 以可见 E2E 或明确未落地证据验收；⑪ 相关单测、集成/E2E、typecheck、lint、build、运行冒烟均记录命令、退出码和证据；⑫ 性能同场景 A/B 报告 RPC 数、延迟、CPU、网络、WebView2 内存、正确率。
- 追踪:`REQ-20260730-01 → .iteration/context.json 单目标执行包 → RPC/SCM/PTY/terminal/window/remote-host/commune symbols and paths → deterministic unit/integration/E2E/performance regressions`

#### 执行顺序

0. **P0（不暂停其余项）**：落地并验收 `REQ-RDG-REMOTE-CONNECT-01` 四路径真接通；与 1–6 并行/穿插，不单开平行 goal、不因本条停 goal。
1. 同步并冻结基线；读取最近两次 NLM 对话与上一迭代状态，只作事实盘点。
2. 建三源去重矩阵，先识别既有护栏、半成品、缺测与回归，避免重复建设。
3. 先立可观测基线与统一 RPC/PTY 生命周期护栏；继而 SCM 非 Git 缓存、日志聚合；再处理终端内存/清空。
4. 生命周期和观测护栏稳定后，分波推进跨窗口工作区单例与远程 Host 接入。
5. Agent's Commune 先核需求—实现—展示链：已有则修展示/回归，未有则最小实现。
6. 核心闸通过后方处理浏览器 Warning。

#### 回归矩阵下限

- 本地桌面、Remote 局域网、Remote 公网等价受控场景；正常网络、延迟、丢包、断连、重连。
- Git 仓库、非 Git 目录、目录切换；watcher/heartbeat/pane/SCM 多触发同源共闸。
- Pane 创建、输入、Resize、销毁、恢复；销毁/超时/取消竞态与队列饱和。
- 长时输出、超限 Scrollback、重复 clear/右键清空、Pane 关闭后的 WebView2 内存回落。
- 两窗口争用同 Remote 工作区、旧窗口最小化/后台、焦点切换、新窗口打开空闲工作区。
- 远程 Host 慢发现、空列表、已有工作区、新建失败、拖拽接入、接入后尺寸变化与手动重同步。

### REQ-MOBILE-REMOTE-RUNTIME-LASTERROR-01 · 手机 Remote 端 Chrome Messaging 通道关闭报错根因消除

- 批准依据:`批准 PENDING-REQ-20260730-02`
- 状态:`ACTIVE`
- 版本:`v1`
- 行为:定位手机 Remote 页面长期高频 `Unchecked runtime.lastError: A listener indicated an asynchronous response by returning true, but the message channel closed before a response was received` 的真实来源。项目监听器仅在确需异步响应时保持通道；成功、失败、异常、超时、提前退出、页面/Tab/Frame/Port 销毁、切后台、导航与重连均恰一次响应或取消；无需响应者不保持通道。发送端消费并分类无接收方、通道关闭与 runtime.lastError，不无意义重试或重复监听。若项目未使用 Chrome Extension Messaging，则以移动浏览器隔离实验确认第三方来源，业务代码不作遮掩性修改。
- 边界:仅限手机 Remote 端及其启动、PWA/service worker、共享 transport/terminal 与实际移动浏览器运行环境；先证明 API 归属再改码；普通 Worker 同名 sendMessage 不等于 Chrome API；源存在时不改 generated/minified 产物；不屏蔽 Console、不篡改第三方扩展、不顺带清其他告警；响应至多一次，监听器/timer/pending 与页面生命周期同生死且清理归零；重试有界。
- 验收:① CodeGraph 与精确源码/清单查询列全手机 Remote Chrome Messaging 注册点、发送点及调用链，并排除普通 Worker 同名方法；② 每个 return true/等价异步监听器具成功、失败、throw、timeout、early-return、background、navigation、目标销毁分支测，恰一次完成/取消且计数归零；③ 无需响应者不 return true，Promise 模式仅在实际 API/Manifest 支持时采用；④ 发送端覆盖 no receiver、channel closed、runtime.lastError、页面销毁、切后台、超时，消费错误、重试有界、不重复注册；⑤ 同一手机 Remote 场景 A/B 中项目来源重复错误降为零，终端输入、后台保活、重连与失败反馈不回归；⑥ 若仓库无 Extension Messaging，则以干净 Profile/无扩展环境及逐个禁用对照隔离第三方来源，业务代码 diff 为零；⑦ 输出根因、修改文件、关键调用链、移动自动化/适用真机验证命令、退出码、浏览器版本与证据。
- 追踪:`REQ-MOBILE-REMOTE-RUNTIME-LASTERROR-01 → src/remote startup/service-worker/transport or third-party mobile-browser isolation evidence → unit/integration/mobile-browser console regression`

#### 停机条件

- 未证明手机 Remote 端 API 归属前，不修改业务代码。
- 实际移动浏览器/Manifest 对 Promise listener 支持不明时，先核官方契约与现有 manifest。
- 只能在含用户数据的常用移动 Profile 复现且无痕/隔离环境不可用时，停止扩展归因并请求安全诊断窗口。

### REQ-RDG-REMOTE-CONNECT-01 · rdg 公网/LAN Remote 真接通（桌面+手机浏览器）

- 批准依据:`批准。然后此前将其推进设置到此前目标中（goal），不要暂停goal`
- 状态:`ACTIVE`
- 版本:`v1`
- 行为:rdg 作 host 时，本迭代须四路径真正接通可用会话（非空白壳/半开服务）：(1) LAN×桌面浏览器：显式启 LAN 后打开 dashboard 根 URL，TOTP/session/E2EE+WS 握手，workspace/pane 列表、PTY 订阅、stdin 回显、resize/claim；(2) LAN×手机浏览器：同根 URL 与 mobile 产物等价接入；(3) 公网×桌面浏览器：rdg 启 public host 后同账户经 cloud relay/WebRTC/E2EE 完成 hello/拓扑/pane/PTY；(4) 公网×手机浏览器：mobile 产物同路径可用。真接通=控制器见 host 在线/拓扑，至少一 pane 可订阅并有输出或输入回显；失败须可行动错误。本条挂入 `REQ-20260730-01` 为 goal 内 P0 硬闸，与 goal 其余项并行推进、不得暂停 goal。
- 边界:范围含 packages/ridge-cli（TUI 启停、LAN/public 生命周期、dashboard 根 URL）、LAN host、packages/remote transport/provider、src/remote/** 与 remote-dist/{desktop,mobile} 启动链、cloud host daemon/artifact 加载、接通相关确定性测与受控真连冒烟。非目标：不重做浏览器完整桌面 IDE；不改会员计费；不开放异账号公网整机；不以 VNC 作路径；未接通前不做无关 UI 大重构；不自动发版/推送。不可动：LAN 不以 cloud 账户作门禁（TOTP/session/E2EE）；公网同账户双校验且 relay 不见 PTY 明文；LAN/Cloud 启动判定不得因 cloud API 成败互相误杀；退出 TUI 回收本 TUI 启动的服务；产物线 remote-dist/{desktop,mobile} 单真相。公网真连无凭证时以协议集成测+runbook 标用户轨。
- 验收:① 启动判定与握手→subscribe→stdin 测绿；② rdg dashboard 根 URL/默认 stopped/显式 start-stop，exit 后端口关闭；③ pnpm build:remote 出 desktop+mobile 且 rdg 可提供静态资源；④ 四格冒烟均有命令+退出码+证据或明确 blocked；⑤ 至少一条 LAN 真浏览器与一条公网（fixture 或真连）E2E 证据，禁只绿单测仍空白壳。
- 追踪:`REQ-RDG-REMOTE-CONNECT-01` → ridge-cli remote/lan_host/dashboard → packages/remote provider → src/remote bootstrap → remote-dist → handshake/subscribe/stdin 测与 smoke；归属 goal `REQ-20260730-01` 执行序 0

### REQ-AGENT-CATALOG-01 · Agent 识别表 / 历史 / 恢复 YOLO / 设置

- 批准依据:`GOAL OBJECTIVE: complete agent catalog history resume yolo ridge-mcp`
- 状态:`ACTIVE`
- 版本:`v1`
- 行为:运行中 Grok 进程须被 agent 发现路径识别；历史须提取 Codex 与 Grok 会话；点击恢复须切 cwd 并自动 resume；恢复旁 YOLO 开关按 agent 配置注入 yolo 参数；设置-智能体可配置进程名/启动/yolo 参数；内置 claude/codex/grok 等默认，识别以该表为准。
- 边界:不伪造 NLM 内容；不自动发版；冷门 agent 靠用户自定义。
- 验收:cargo test agent_catalog/parses_grok/parses_codex 绿；KNOWN_AGENT_NAMES 含 grok；plan_agent_resume 可 YOLO。
- 追踪:`REQ-AGENT-CATALOG-01` → teammate/agent_catalog.rs → project.rs history → AgentCenterPanel/SettingsPanel

### REQ-MCP-JOIN-GROUP-01 · ridge_join_group 参数/宿主校验与可观测落地

- 批准依据:`我批准所有项`
- 状态:`ACTIVE`
- 版本:`v1`
- 行为:桌面 host 在 `group_name`+(agent_id|target_pane_id) 合法且目标在花名册时 emit TEAMMATE_GROUP_ADD_MEMBER 并由前端编组 store 消费；非法参数稳定 -32602+可读 message；companion-only 宿主返回明确 capability 错误（非 silent OK），文档与 tools/list 一致；fire-and-forget+前端 localStorage SSOT 为已知限制直至另批迁后端。
- 边界:范围 packages/ridge-mcp、src-tauri teammate mcp/join_group 桥、AgentCenterPanel 编组加成员、docs/mcp-integration。非目标:不重做整套编组 SSOT 迁后端。不可动:不得静默落到错误 pane/0 号分屏；越界 target 必须失败。
- 验收:合法/非法/companion 三路径有 MCP 调用证据；失败码与 message 写入 checklist；非法不得 silent OK。
- 追踪:`REQ-MCP-JOIN-GROUP-01` → mcp.rs join_group → AgentCenterPanel group event → SCRATCH smoke

### REQ-NLM-OPENPLAN-01 · 开放规划 post-v0.1.3 优先项入迭代队列

- 批准依据:`我批准所有项`
- 状态:`ACTIVE`
- 版本:`v1`
- 行为:将 NLM 开放规划 post-v0.1.3 中 R-VERIFY、R-CDP-150、R-INCR、R-WSLEG、R-RDG-INCR 等开放项登记为可跟踪迭代条目，并在 checklist 标注来源笔记本 66919cb9 与 note 4b8db248；与 Active goal 对账后按优先级推进。
- 边界:范围 docs/PENDING 已提升后的 Active 追踪、iteration checklist、PROJECT-STATE 索引；不在本条内强制完成全部真机验收。非目标:不把已实现闭环索引 note 当未完成需求。NLM 仅建议层；验收仍以代码/测试/证据为准。
- 验收:nlm-extracted-requirements.md 或等价证据含开放规划表；checklist 有来源标注；本条 Active 存在且可被 gate 引用。
- 追踪:`REQ-NLM-OPENPLAN-01` → nlm note 4b8db248 → checklist / PROJECT-STATE

### REQ-RIDGE-KERNEL-HOST-01 · 内核进程与外壳生命周期（深根模式）

- 批准依据:`全数批准`
- 状态:`ACTIVE`
- 版本:`v1`
- 行为:内核是用户会话内可独立于外壳存活的明确进程（非 Windows 服务/非开机永驻）；可检测单实例（socket/pid 等）。桌面托盘/菜单：原「彻底退出」改为「退出桌面端」（只关桌面 UI，内核继续）；新增「彻底退出」结束内核且不弹确认窗，菜单文案须提示将一并退出仍连接的 rdg 等外壳。rdg 增加「彻底退出」结束内核；若此时桌面/内核仍在运行须**命令行 Y/N 二次确认**（默认 N），取消则不杀内核；仅退 rdg UI 则保留内核。一旦内核退出（桌面彻底退出 / rdg 确认后 / CLI），所有已连接外壳（桌面与 rdg）自动退出。桌面与 rdg 启动时先检测内核：在则接入、不在则自启，默认禁止静默双开两套内核。提供 CLI 结束内核（与 GUI 彻底退出同效）。
- 边界:内核进程模型与发现/attach、桌面托盘/菜单文案与行为、rdg 退出菜单与命令行 Y/N 确认、启动 detect-or-spawn、CLI 杀内核、外壳在内核死亡时的自退 不做成 Windows 服务/开机自启默认；不重做业务功能 UI；不引入 CRDT；不自动发版 退出桌面不得误杀用户未选择「彻底退出」时的内核；彻底退出后不得残留孤儿外壳占资源；不丢 PTY 字节；默认单内核实例避免静默双开 原始意图摘要:综合 NLM 对话与用户修订：Ridge 为「内核进程 + 薄外壳」。内核**不是**系统级常驻服务（非 Windows Service / 非开机永驻），而是用户会话内可独立存活的明确进程；有清晰启动与**彻底退出**入口。外壳（桌面 Tauri / rdg / Web）可单独退出而内核仍运行（深根模式）；也可彻底退出内核，则所有外壳一并结束。桌面重开时先检测内核是否已在跑：在则接入，不在则自启。rdg 二次确认由系统 MessageBox 改为终端 `[y/N]`（用户 2026-07-31 修订）。
- 验收:① 退出桌面后端内核进程仍在、rdg 仍可 attach；② 桌面「彻底退出」无确认框结束内核，rdg 自动退出；菜单文案含一并退出 rdg 提示；③ rdg 彻底退出且桌面仍在时终端 Y/N 确认，取消则内核与桌面均在；确认后内核与桌面均退出；④ 杀内核后桌面与 rdg 均退出；⑤ 冷启动桌面：无内核则拉起，有内核则接入（测双启场景）；⑥ CLI 可结束内核
- 追踪:PENDING-REQ-RIDGE-KERNEL-HOST-01 → NLM notes 1dd91891+27be8446 + 用户修订 → tray/rdg/lifecycle

### REQ-RIDGE-KERNEL-DOMAIN-01 · 领域能力 SSOT 在内核（外壳只投影）

- 批准依据:`全数批准`
- 状态:`ACTIVE`
- 版本:`v1`
- 行为:下列能力的权威实现与调用出口在内核进程（或 packages 纯库由内核暴露），桌面/rdg 仅 UI：① 工作区文件/目录与路径沙箱；② Git/SCM 与 process 护栏；③ 远端 Host 接入/拓扑/会话；④ Agent 发现、花名册、编组、历史与 resume 计划；⑤ 与运行时相关的设置。深根模式下退出桌面后，rdg 仍能调用上述能力。
- 边界:ridge-core/teammate/git/remote/settings 出口迁入或固定在内核；各外壳改道调用 不在本条做大爆炸 UI 重写；纯外观主题可仍在外壳；不伪造无头已全绿 外部进程墙钟超时+杀树+同生死许可；双端并发 cap 同常量；复合身份不回退 activeWorkspace 猜 原始意图摘要:用户要求文件系统、Git、远端接入、Agent 名册/编组/历史、设置等均为**内核能力**，Tauri/rdg 只是外壳投影。能力若仍挂在 Tauri 命令上，无头 rdg 与「退出桌面、内核仍跑」的深根模式都会假死或不可用。本条与 HOST 生命周期配套：HOST 定义进程与退出；本条定义**哪些能力必须挂在内核进程内**。
- 验收:深根（桌面已退、内核在）下 rdg 至少完成 FS/Git/Agent 花名册读/Remote 之一的真实路径；同路径桌面可复现；内核退出后上述调用 fail-closed
- 追踪:PENDING-REQ-RIDGE-KERNEL-DOMAIN-01 → note 27be8446 → core 能力矩阵

### REQ-RIDGE-MCP-AS-KERNEL-API-01 · ridge-mcp 接内核而非 Tauri 随从

- 批准依据:`全数批准`
- 状态:`ACTIVE`
- 版本:`v1`
- 行为:ridge-mcp 默认发现并连接**当前内核**（与桌面/rdg 同发现机制），不依赖 Tauri invoke 作为唯一后端；可 split/launch agent、roster 寻址、跨 agent 消息/委派；内核不在时明确错误；内核被彻底退出后 MCP 连接断开且不假成功。
- 边界:packages/ridge-mcp、teammate、ridge-tmux、发现/attach 与 HOST 一致、文档 不删除可选桌面捆绑安装；不重做 Commune 全部 UI 非法目标稳定失败码；禁止静默落到错误 pane 原始意图摘要:ridge-mcp 被 Tauri 接管即做错。应基于 teammate 服务或 tmux 垫片，直接拉起终端并启动 agent，支持相互定位与交流——是**内核**暴露给外部 agent 的 API 面。深根模式下桌面退出后 MCP 仍应能连内核（若内核在），而非依赖桌面进程。
- 验收:① 无 Tauri 仅内核时 MCP initialize+tools/list+至少一类协作工具可测；② 桌面退出、内核仍在时 MCP 仍可用或可重连；③ 内核退出后 MCP 失败可观测；④ 文档写明拓扑
- 追踪:PENDING-REQ-RIDGE-MCP-AS-KERNEL-API-01 → note a7962b2f → ridge-mcp/teammate

### REQ-AGENT-COMMUNE-UI-02 · Agent's Commune interactive cards, status projection, and history recovery

- 批准依据:`预审批刚刚需求`
- 状态:`ACTIVE`
- 版本:`v1`
- 类型:`MODIFY` + `FIX`
- 关联:`REQ-AGENT-INTERACTION-STATE-01`、`REQ-AGENT-HISTORY-SOURCE-02`、`REQ-AGENT-HISTORY-01`、`REQ-AGENT-COMMUNE-CONTINUITY-01`
- 原始意图:强化 Agent's Commune 侧栏交互，使 Agent 状态、历史会话、恢复入口和需介入时的暂态 Pane Border 形成可验证闭环。
- 行为:(1) 卡片展示稳定 Agent 身份、运行中/空闲/等待审批/停止/失败状态、标题与最近活动；左侧竖色条、文本和 `aria-label` 同源，颜色非唯一语义。(2) 点击可聚焦正确 workspace/pane，等待审批有明确入口，状态变化不造重复布局事件。(3) 从有证据的 Agent adapter 读取 JSONL/会话文件，按稳定 Agent identity（不按 CWD）分组，一 session 一行，展示标题、session id、Agent、原始 CWD、最近活动和真实最新 assistant 内容；损坏/超大/未知源局部失败且可诊断。(4) 运行中与历史以原生 session id 关联，禁止标题/CWD 猜测。(5) 恢复使用 adapter 结构化 executable/argv/CWD/session identity，在当前 workspace 新建唯一 pane，保留错误与取消语义，不拼接未转义 shell 字符串。(6) Pane Border 只映射需用户介入的暂态 attention/HITL 状态；正常运行/空闲无 Border，用户接管或聚焦后立即清除；Pane 销毁、跨 workspace 聚焦幂等。
- 边界:范围 `src/lib/teammate/**`、AgentCenter/Commune 卡片与状态模型、Agent history adapter/DTO、JSONL/会话解析、CWD/session identity、pane tree/header/border 投影、结构化恢复入口及必要 Tauri topology/历史 DTO 和确定性测试；不上传历史、不修改第三方会话文件、不按标题/CWD 猜测身份、不伪造无可靠 resume 契约、不引入第二持久状态源、不改 Remote 协议；pane header、Agent Tab、历史恢复读取同一后端事实；复合 `(workspaceId,paneId)` 与原生 session identity 不得回退模糊匹配；键盘导航、HITL 安全门、多窗口/工作区单例语义不得回归。
- 验收:多 Agent、多 CWD、多 session、多回复及运行/空闲/等待审批/停止/失败、损坏/超大 JSONL fixture 下，卡片按稳定 Agent 分组且一 session 一行；左色条、状态文本、`aria-label` 同源，Pane Border 仅在 attention/HITL 时出现并在接管/聚焦后清除；无变化轮询零重复事件；点击恢复在当前 workspace 恰新增一个 Pane 且捕获 executable/argv/CWD/session identity，重复点击单飞；跨 workspace 只聚焦正确 pane；Pane destroy/取消不留 pending；至少一类真实 Agent/等价进程 E2E 证明 CWD 恢复与状态闭环。
- 追踪:`REQ-AGENT-COMMUNE-UI-02` → `.iteration/intakes/` → NLM 深研决策包 → AgentCenter/Commune card + teammate model/adapters + history parser + pane state/border projection + structured resume → Vitest/Svelte/adapter fixtures、Rust/TypeScript 集成及真进程等价 E2E → 迭代归档

### REQ-MOBILE-REMOTE-KEYBOARD-QOS-02 · Mobile Remote keyboard stable visual offset

- 批准依据:`这条需求也预审批通过`
- 状态:`ACTIVE`
- 版本:`v1`
- 类型:`MODIFY` + `FIX`
- 关联:`REQ-MOBILE-REMOTE-STATE-01`、`REQ-MOBILE-REMOTE-INPUT-FEEDBACK-01`、`REQ-REMOTE-SMOOTH-STATE-02`
- 原始意图:修复手机 Remote 唤起系统键盘时输入域上移抖动、偏移漂移或被键盘遮挡的问题，同时保留既有终端核心实现。
- 行为:仅在现有 visualViewport/光标锚点/键盘投影链上做稳定化；以键盘顶部、safe-area、输入域实际 rect 和终端 cursor/fallback anchor 计算有界视觉偏移，键盘显示、viewport resize、旋转和字体布局收敛后再有限次数校正；输入域底部须稳定位于键盘顶部安全间距之上。保持 `.term-stage`、`.container`、canvas、PTY rows/cols、核心输入/渲染/传输语义不变；pointer 坐标不参与键盘锚点。
- 边界:范围 `src/remote` 的 keyboard offset/viewport adapter、输入域布局与其确定性测试、移动浏览器等价 E2E；不得改写终端核心、PTY 尺寸、Remote 协议或以无界 RAF/定时器追逐 viewport；所有 visualViewport/resize/focus 监听须可取消且生命周期单飞。
- 验收:覆盖 iOS/Android 等价 visualViewport fixture、键盘开合、旋转、safe-area、缩放、长输出、非底部 pane 和快速 A→B→A；每次收敛时输入域 bottom ≤ keyboard top−安全间距，偏移有界且无抖动/累积漂移，键盘收起归零；DOM 高度、PTY claim/rows/cols 和核心渲染调用次数不变；监听器、RAF、定时器在销毁后归零；真实移动浏览器或 CDP 证据通过。
- 追踪:`REQ-MOBILE-REMOTE-KEYBOARD-QOS-02` → `keyboardOffset`/MobileRemoteUiState → keyboard geometry fixtures + mobile E2E → iteration archive

### REQ-REMOTE-RUNTIME-PERF-MEMORY-02 · Remote runtime performance, robustness, and memory reclamation

- 批准依据:`这条需求也预审批通过`
- 状态:`ACTIVE`
- 版本:`v1`
- 类型:`MODIFY` + `FIX`
- 关联:`REQ-RDG-REMOTE-CONNECT-01`、`REQ-REMOTE-SMOOTH-STATE-02`、`REQ-MOBILE-REMOTE-STATE-01`
- 原始意图:降低 Remote 手机端长期运行的 UI/RPC/订阅开销，避免 pane、scrollback、worker、canvas、listener、timer 和 pending task 持续增长或无法被 GC 回收。
- 行为:沿现有连接、pane、scrollback、worker 和 scheduler 生命周期补齐单一释放出口；workspace/pane/connection 销毁、重连和切换时取消 pending、退订 listener、停止 timer/RAF、清空过期快照与有界 scrollback 引用、释放 worker/canvas 资源并让不可达对象自然可被 GC；输入与 active raw 优先级不受低优先级历史加载影响；重复订阅/轮询/渲染与 stale callback 必须幂等、可观测且有界。不得以生产环境强制 GC、无限重试或隐藏 Console 错误伪造修复。
- 边界:范围 `src/remote`、`packages/remote` 生命周期/调度/scrollback/worker 资源管理及确定性性能测试与移动/公网等价运行证据；不得改变 Remote 核心协议、pane 身份、终端核心渲染算法或用第二连接绕过资源问题；所有取消须落到真实任务/监听器/进程释放。
- 验收:长时多 pane、多 workspace、后台切换、断线重连、滚动历史和键盘开合 soak 下，listener/worker/timer/RAF/pending RPC/订阅计数在销毁后归零或回到基线；scrollback 达上限自动清理且 clear/右键清空释放页面与后台引用；Heap/对象快照无持续线性增长，输入延迟、RPC 数和 CPU/网络不回归；重复订阅和 stale callback 有确定性测试，移动真实机或等价 CDP/公网运行记录证据。
- 追踪:`REQ-REMOTE-RUNTIME-PERF-MEMORY-02` → RemoteLink/CloudRemoteConnection + pane scheduler + scrollback/worker lifecycle → soak/heap/resource counters → iteration archive

### REQ-MOBILE-REMOTE-BACKPRESSURE-01 · Mobile Remote bounded render/input backpressure

- 批准依据:`用户明确要求预审批通过并纳入本次迭代：Remote 手机端偶发卡死，需设置排队缓冲，防止大量卡死渲染端和输入端。`
- 状态:`ACTIVE`
- 版本:`v1`
- Type:`MODIFY` + `FIX`
- Related:`REQ-REMOTE-RUNTIME-PERF-MEMORY-02`、`REQ-REMOTE-SMOOTH-STATE-02`、`REQ-MOBILE-REMOTE-STATE-01`
- Current evidence:the input/RPC path already has `PaneRpcScheduler` and `paneInputGate` bounds, while `TerminalManager` time-slices PTY parsing. However, `_enforceDeferredFeedCap` synchronously drains a deferred render queue with `Infinity`, and repeated `concatU8` growth can make a PTY flood monopolize the mobile main thread.
- 行为:the mobile Remote terminal must use a per-pane bounded render queue with a fixed byte cap and frame-time budget; render output is lower priority than terminal input and control RPCs. Deferred bytes are queued as chunks, never repeatedly concatenated, and never synchronously drained without a budget. On cap pressure, output admission is bounded and observable; input FIFO is not silently discarded. Pane/connection destruction cancels timers/RAF and releases every queued chunk. Resize remains latest-wins and input remains FIFO through the existing scheduler.
- 边界:scope is `packages/remote/src/shared/terminal/manager.ts`, `packages/remote/src/shared/terminal/terminalFeedPolicy.ts`, mobile TerminalCanvas/Remote routing as needed, and focused deterministic tests. Do not change the terminal protocol or silently hide errors.
- 验收:deterministic flood tests prove queue bytes never exceed the cap, no drain call uses an unbounded budget, per-frame kernel work stays within the configured budget, input ordering/latency is preserved while render output floods, overflow counters are reported, and destroy/clear/reconnect return pending buffers and timers to zero. Mobile/PWA and public Remote soak evidence must show no repeatable render/input freeze, unbounded queue growth, or post-destroy callback.
- 追踪:`REQ-MOBILE-REMOTE-BACKPRESSURE-01` → bounded feed queue + PaneRpcScheduler/input gate → flood/lifecycle tests → mobile/public soak evidence → iteration archive

### REQ-MOBILE-REMOTE-LIVE-TAIL-01 · Mobile Remote live-tail authority and incremental scrollback

- 批准依据:`用户明确要求 Remote 端始终展示最新实时渲染、保持可输入；历史仅在上滑时按量加载。`
- 状态:`ACTIVE`
- 版本:`v1`
- 行为:attach the live PTY listener before the bounded visual seed; render live bytes immediately while retaining only a bounded FIFO replay copy. If the seed completes, apply it as a reset and replay the retained live tail in order; if the replay cap overflows, discard the stale seed instead of blocking or retaining unbounded output. Treat the seed as a small latest-screen visual bootstrap, not as the live source. Fetch older scrollback only from the explicit upward-scroll query; its loading/cancellation cannot block live rendering or input/control RPCs.
- 边界:scope `src/remote/lib/cloudRemote.ts`, `src/remote/MainApp.svelte`, `packages/remote/src/shared/terminal/manager.ts`, and scrollback query/worker paths. Do not change terminal protocol semantics or replay unbounded host history on every subscribe/resume.
- 验收:deterministic live-during-seed ordering proves live output is visible before a slow seed resolves, and a successful seed reset replays the bounded tail without a gap; seed/replay bytes stay within configured caps; overflow skips the stale seed; upward paging is incremental and cancellable; input remains usable during seed/history fetch; reconnect/pane destruction removes listeners and pending work; mobile/PWA/public soak shows the live tail stays current while scrolled history remains available.
- 追踪:`REQ-MOBILE-REMOTE-LIVE-TAIL-01` → CloudRemote subscribe/live FIFO → bounded TerminalManager feed → scroll-up query paging → ordering/lifecycle/mobile soak → iteration archive

### REQ-REMOTE-PANE-GRID-INVARIANT-01 · Pane box and terminal grid invariant

- 批准依据:`用户明确要求 term 大小永远跟随 Pane 大小；外部或内部 stale 参数不得取消跟随，Pane Resize 必须可强制修复。`
- 状态:`ACTIVE`
- 版本:`v1`
- 行为:the pane content box is the single geometry authority. Every settled ResizeObserver, orientation, safe-area, keyboard, reconnect, and manual refresh path re-measures the pane, derives rows/cols from current cell metrics, resizes the local kernel/PTY through the existing deduped claim path, and reprojects the renderer. A host `pty-resized` notification is a trigger to refit, never permission to inject a smaller stale grid. The force-fit button bypasses debounce and verifies/retries a failed claim.
- 边界:scope `packages/remote/src/shared/terminal/manager.ts`, `src/remote/lib/TerminalCanvas.svelte`, `src/remote/MainApp.svelte`, and pane geometry tests. Preserve protocol, renderer backend, and shared multi-viewer semantics; only local-authority mobile panes may claim automatically.
- 验收:geometry fixtures map pane pixels to exact rows/cols across DPR/PWA/orientation; a stale smaller host grid is corrected on the next settled fit and by the manual force-fit; shell canvas/scissor/backing dimensions match the pane after resize; failed/stale callbacks are cancelled; resize RPCs remain latest-wins and bounded with no storm; cross-host and mobile soak show no persistent shell-smaller-than-pane state.
- 追踪:`REQ-REMOTE-PANE-GRID-INVARIANT-01` → TerminalManager fit/claim + TerminalCanvas force-fit → pane geometry/resize regression tests → mobile/PWA/LAN soak → iteration archive

### REQ-MOBILE-REMOTE-PWA-SAFE-AREA-01 · Mobile Remote browser/PWA safe-area UI parity

- 批准依据:`用户明确预审批通过：下一迭代 Remote PWA safe-area、Query 管理、Git 提交推送/GitGraph、Agent 侧栏桌面 parity；补充 Remote Agent 编组管理与 Agent 历史 Tab。`
- 状态:`ACTIVE`
- 版本:`v1`
- 行为:普通移动网页与 PWA standalone 安装版共享同一 Remote UI 语义；刘海屏、挖孔屏、底部手势区、横竖屏旋转、键盘唤起及 viewport 变化时，侧边抽屉的功能按钮与关闭按钮均落在可见且可点击安全区内，safe-area 与 visualViewport 变化只影响布局投影，不改变终端核心、Pane 身份或 Remote 协议。
- 边界:范围 `src/remote` 抽屉/导航/viewport adapter、CSS env safe-area、display-mode 适配、键盘与旋转 fixture、浏览器/PWA E2E；不得以 UA 特判复制两套 UI、不得牺牲无障碍焦点顺序或引入不可取消监听器。
- 验收:Chromium mobile 与 iOS/Android 等价 fixture 覆盖 browser 与 standalone、刘海 safe-area、旋转、键盘开合和快速打开/关闭；按钮可见、可点击、焦点可达，点击命中率 100%，无 drawer listener/timer 泄漏，普通网页与 PWA 截图/交互合同一致。
- 追踪:`REQ-MOBILE-REMOTE-PWA-SAFE-AREA-01` → drawer/viewport/safe-area → browser+PWA E2E + geometry tests → iteration archive

### REQ-REMOTE-QUERY-CACHE-01 · Remote Git and file request Query management

- 批准依据:`用户明确预审批通过：下一迭代 Remote PWA safe-area、Query 管理、Git 提交推送/GitGraph、Agent 侧栏桌面 parity；补充 Remote Agent 编组管理与 Agent 历史 Tab。`
- 状态:`ACTIVE`
- 版本:`v1`
- 行为:Git 与文件远程请求统一进入 Query 管理，query key 必含 host/workspace/pane/path/branch 等稳定身份；同一请求单飞去重，drawer/tab 重开优先复用缓存，按 staleTime、显式刷新和 mutation 精确失效；断线时保留可辨识 stale 数据并显示状态，不发送无意义重试。
- 边界:范围 `src/lib/stores`、`src/lib/remote`、`src/remote`、`packages/remote` 的 Git/File query hooks、缓存策略、失效与 loading/error 投影；不得新增第二缓存源、以全局 clear 代替精确失效、绕过 RPC 去重或改变 Git/File 权限与沙箱。
- 验收:同一 host/workspace/path 连续打开只产生一次请求；切换 workspace/branch/path 不串数据；提交、推送、文件写入与重命名只失效相关 key；断线、超时、取消、Pane/Host 销毁后无 pending 请求和 stale callback；Query/RPC 计数与现有基线相比不回归。
- 追踪:`REQ-REMOTE-QUERY-CACHE-01` → Query key/fetcher/invalidation → Vitest + Remote LAN/cloud E2E → iteration archive

### REQ-GIT-INTERACTIVE-PUBLISH-GRAPH-01 · Interactive Git commit/push and GitGraph tab

- 批准依据:`用户明确预审批通过：下一迭代 Remote PWA safe-area、Query 管理、Git 提交推送/GitGraph、Agent 侧栏桌面 parity；补充 Remote Agent 编组管理与 Agent 历史 Tab。`
- 状态:`ACTIVE`
- 版本:`v1`
- 行为:Git UI 提供状态、变更选择、提交信息、提交与推送交互；权限/能力门、二次确认、进度、取消、冲突、超时、非 Git 与失败错误均有明确反馈；新增 GitGraph Tab 展示分支、提交节点、父子连线、当前 HEAD 与选中提交详情，数据经 Query 缓存并支持精确刷新。
- 边界:范围 Git/SCM domain 与 RPC capability、`src/lib` Git 面板/Tab、GitGraph 投影与无障碍交互、远程 Host 同源能力和确定性测试；不得拼接未转义 shell、绕过 process guard/capability、自动 push 未确认内容、以图形层掩盖后端错误或在非 Git 目录轮询。
- 验收:真实临时 Git 仓库覆盖 clean/dirty、提交、推送成功/拒绝、无 upstream、冲突、非 Git、超时/取消；提交/推送成功后仅失效相关 Query；GitGraph 正确绘制分支/merge/HEAD/选中详情，键盘与移动触控可用；RPC、子进程并发、日志噪音与既有护栏不回归。
- 追踪:`REQ-GIT-INTERACTIVE-PUBLISH-GRAPH-01` → ridge-core Git SSOT + Git panel/graph → Rust/TypeScript/real-repo E2E → iteration archive

### REQ-AGENT-COMMUNE-REMOTE-PARITY-01 · Remote Agent groups and history Tab parity

- 批准依据:`用户明确预审批通过：下一迭代 Remote PWA safe-area、Query 管理、Git 提交推送/GitGraph、Agent 侧栏桌面 parity；补充 Remote Agent 编组管理与 Agent 历史 Tab。`
- 状态:`ACTIVE`
- 版本:`v1`
- 行为:Remote 手机网页/PWA 提供与桌面同源的 Agent 侧栏能力：状态卡片（每张卡展示真实 CWD，按稳定 `paneId`/Agent identity 映射并安全截断）、审批入口、历史展示 Tab、Agent 编组管理（创建/重命名/成员增删/排序/删除及状态）、会话 CWD/session 定位、结构化恢复、workspace/pane 聚焦与仅待用户介入的暂态 Pane Border 反馈；移动端布局遵循 safe-area、键盘和触控无障碍。
- 边界:范围 `src/remote` Agent Tab/历史/编组交互、共享 Agent DTO/query/cache、teammate/AgentCenter 后端能力与权限门、跨窗口/Host 身份映射和确定性测试；不得复制桌面私有状态、按标题/CWD 猜 Agent、把历史写回第三方会话文件、绕过 HITL 或 Remote capability。
- 验收:多 Agent、多编组、多 CWD/session、运行/空闲/等待审批/停止/失败及损坏历史 fixture 下，Remote 与桌面展示同一状态/历史/成员及每卡真实 CWD；编组 CRUD、审批、历史 Tab、恢复和跨 workspace 聚焦均可在手机触控完成且重复操作单飞；刷新/断线/重连/窗口切换不丢组、不串 Pane、不留 pending RPC；至少一条 LAN/cloud mobile E2E 与桌面 parity 对照通过。
- 追踪:`REQ-AGENT-COMMUNE-REMOTE-PARITY-01` → AgentCenter/Commune shared DTO + remote tabs/groups/history → Vitest/Svelte/Rust + mobile E2E → iteration archive

### REQ-MOBILE-REMOTE-PWA-INSTALL-01 · Browser-native PWA install compatibility (no in-app install control)

- 批准依据:`用户补充：定位 PWA 安装按钮未显示根因并实现真实可验证 PWA 安装能力；纳入已预审批下一迭代。`
- 状态:`ACTIVE`
- 版本:`v1`
- 行为:Remote 不显示“添加到主屏幕”或 Install App 按钮，不拦截、不消费、不伪造 `beforeinstallprompt` 或安装状态；安装由浏览器原生 UI 完成。业务仅保证 manifest、service worker、scope、icons、HTTPS/安全上下文与 standalone/display-mode 链路可诊断，并使普通网页与 PWA 使用同一 safe-area、旋转、键盘、主题和抽屉交互语义。
- 边界:范围 PWA manifest/service-worker registration、安装资格只读诊断、display-mode 与布局投影、browser/PWA E2E 与兼容性说明；不得引入永久安装按钮、重复监听/重复 prompt、阻塞首屏、屏蔽 Console，或把第三方 WebView 当成支持 PWA。
- 验收:Chromium mobile HTTPS fixture 可检查 manifest/service worker/scope/icons 与 standalone/display-mode；普通网页和已安装 PWA 的抽屉按钮均在 safe-area 内可见可点，旋转/键盘/主题切换不闪烁；无 `beforeinstallprompt` 业务监听、无安装状态伪造；浏览器原生安装入口不纳入业务 E2E。
- 追踪:`REQ-MOBILE-REMOTE-PWA-INSTALL-01` → manifest/SW/display-mode diagnostics → browser+PWA layout E2E → iteration archive

### REQ-INTERACTION-PARITY-01 · Access/share onboarding, terminal input fidelity, TUI mouse, Explorer resize, and Remote mobile parity

- Status:`ACTIVE`
- Version:`v1`
- Behavior:完善接入 Tab 与工作区分享入口；修复 Remote 手机空格输入、桌面中文标点输入；补齐 Grok 等 TUI 点击/拖拽鼠标转发；让 Explorer 上下自由拖拽且高度自适应填满；使 Remote Agent 与桌面 Agent 同源同数据并移除历史 Tab icon；统一移动端 icon/pending/间距；让文件/图片查看适配手机屏幕且不产生横向滚动。接入 Tab 展示主机、工作区、Pane、共享工作区与连接进度，慢接入不阻塞页面，创建/已有工作区可见，点接入与拖拽接入复用 attach/resize 语义；工作区 Tab 右键、Explorer、Hosts 三入口复用同一分享授权合同。
- Boundary:范围 `src/lib/components/WorkspaceTabs.svelte`、`WorkspaceSidebar.svelte`、`Explorer.svelte`、`HostsPanel.svelte`、share/cloud adapters、`src/remote/**` Agent/File/Image surfaces、`packages/remote/src/shared/terminal/**` input/mouse seams、必要的 shared DTO/query 与确定性测试；不新增 UI 私有 Agent/Workspace/Pane SSOT，不绕过 Kernel/Remote capability、process guard 或分享授权，不猜 Grok 私有历史格式，不改变终端协议身份/生命周期语义；Kernel/Remote transport、Pane identity、workspace singleton、bounded queue/cancellation、safe-area 与现有 share scope 为冻结边界。
- Acceptance:① 接入 Tab 单元/集成测试覆盖空列表、慢/失败/重试、已有/创建工作区、点接入、拖入 Pane、进度与取消，同一工作区 attach 单飞并按实测尺寸 resize；② 工作区 Tab 右键、Explorer、Hosts 三入口调用同一分享 dialog/授权函数，scope/revoke 权限通过；③ Remote 空格、桌面中文引号/标点断言原字节且无重复；④ TUI mouse fixture 覆盖 press/motion/release、capture/cancel、Ctrl/Alt/Shift、无 mouse reporting 的 shell selection；⑤ Explorer drag 断言上下方向连续、总高度守恒、最小边界可达、无空洞与 listener 清理；⑥ Remote/desktop Agent parity 断言同一 roster/history/group/CWD/status，历史 Tab 无 icon，尺寸 token 一致；⑦ FileViewer/ImagePreview mobile viewport 断言 `scrollWidth <= clientWidth`、软换行/contain 与安全区；⑧ `pnpm check`、聚焦 Vitest、Rust input/mouse tests 与现有 release/worktree gates 全绿。
- Traceability:`REQ-INTERACTION-PARITY-01` → existing share/attach/input/mouse/resize/Agent/mobile viewer seams → deterministic tests + LAN/mobile evidence → iteration archive

- Approval evidence:`进行nlm调研后，然后你自行予审批。通过现有pending需求。`

### REQ-AGENT-COMMUNICATION-REGISTRY-01 · Deterministic Agent lifecycle and communication registry

- Status:`ACTIVE`
- Version:`v1`
- Behavior:Kernel/teammate authority owns a stable `agent_id` record containing session, workspace, pane, CWD, lifecycle generation/lease, status, online state, and last-seen. Commit create only after spawn/attach success; commit destroy only after destroy/lease closure success. Failed or partial attempts remain diagnostic-only. Before communication, sender takes a fresh bounded roster snapshot, validates target identity plus generation/lease and online state, performs at most one refresh when stale, then sends once or returns typed missing/offline.
- Boundary:Kernel Agent/teammate registry and lifecycle APIs; ridge CLI/MCP adapters; desktop and Remote Agent/Commune projections; roster lookup, online/lease validation, idempotency/single-flight, disconnect cleanup, and deterministic unit/integration/multi-Agent tests. No second UI-local directory, identity inferred from title/CWD, silent respawn, unbounded retry, or Remote wire-protocol change before a bounded contract. Existing PTY, pane, workspace-singleton, Remote, and MCP transport contracts remain authoritative.
- Acceptance:Successful create appears exactly once; failed create leaves no active entry; successful destroy closes exactly one generation; failed destroy is visible and blocks unsafe reuse; stale/offline target rejected before send; one bounded refresh cannot spawn duplicates; concurrent sends use one idempotency key with no duplicate communication; reconnect generation races cannot deliver to old Agent; teardown cancels pending calls; real/equivalent multi-Agent E2E proves CWD/session-independent discovery.
- Traceability:`REQ-AGENT-COMMUNICATION-REGISTRY-01` → kernel roster/lifecycle contract → CLI/MCP and desktop/Remote adapters → deterministic tests and multi-Agent E2E → iteration archive

- Approval evidence:`进行nlm调研后，然后你自行予审批。通过现有pending需求。`

### REQ-REMOTE-LINK-FLUIDITY-01 · Remote direct links and measured low-latency interaction

- Approval evidence: `我都接受方案`
- Status:`ACTIVE`
- Version:`v1`
- Behavior: Remote terminal URL/path hits open on a bare primary click without requiring keyboard focus or a modifier. Non-link clicks retain TUI mouse forwarding or host selection. Synchronous terminal input reaches the existing bounded scheduler in the same turn, remains ordered with paste, and is not blocked by render/scrollback work. Pane switching reuses parked kernel/renderer state, drains bounded live catch-up before first paint, and keeps active tail/input priority. Client, host, and relay timing/bytes/CPU are measured separately before selecting code or infrastructure changes.
- Boundary: One authenticated transport remains the source of truth; no second WebSocket/WebRTC connection, unbounded output/history queue, protocol rewrite, global TUI mouse disable, or hidden error. Direct open applies only to a positively hit and validated link/path on primary click; non-link and secondary/drag gestures retain existing semantics. Active input/control preempts low-priority render/history work; all queues and retained resources remain bounded and cancellable.
- Acceptance: Link decision tests prove bare primary click opens URL/path with and without keyboard focus while non-link/TUI/selection paths remain correct. Input tests prove same-turn enqueue, FIFO ordering, bounded queue, and no render-flood starvation. Pane-switch tests prove A→B→A shows the latest tail before or with first paint without full replay and accepts a keystroke. Diagnostics expose per-stage latency/bytes/queue depth and a reproducible report identifies the dominant limiter. Full Remote regression and `pnpm check` pass.
- Traceability: `REQ-REMOTE-LINK-FLUIDITY-01` → `REQ-REMOTE-SMOOTH-STATE-02` / `REQ-REMOTE-RUNTIME-PERF-MEMORY-02` → link/input/pane transport changes → deterministic tests + public/mobile soak → iteration archive.

### REQ-NLM-ITERATION-01 · NLM 愿景与未修复问题迭代闭环

- Approval evidence:`批准`
- Status:`ACTIVE`
- Version:`v1`
- Behavior:`从 NLM 对话/愿景提取候选项，经本地代码、CodeGraph、运行测试核验后，逐项完成已批准的实现与 bug 修复；当前范围覆盖 pane、ridge-term、workspace、Remote 接入及其测试/质量工作流。`
- Boundary:`NLM transcript 仅作候选假设，不作需求或验收；保留当前 dirty worktree；不写入/删除/上传 NLM source/note；不执行 git push、tag、Release、Remote/Cloud 激活；不代替用户完成物理设备、公网 TURN、双真实窗口等用户轨验证；现有 Active REQ、协议/E2EE/TOTP 与发布授权闸不擅改。`
- Acceptance:`request/intake 通过 requirements_gate；每个实现项有对应 CodeGraph trace、测试命令与退出码；全量测试/check、Sonar 新问题为 0/Quality Gate OK、适用 E2E 通过；未满足项进入本地下一轮证据清单。`
- Traceability:`REQ-NLM-ITERATION-01 → NLM read-only candidate → local symbol/path → unit/integration/E2E evidence → iteration archive`

### REQ-NLM-CLOSURE-20260809 · 现场验收与下一轮 NLM 愿景闭环

- Approval evidence:`用户明确要求将 Sonar 测试覆盖率提升到 80% 以上并纳入下一迭代目标。`
- Status:`ACTIVE`
- Version:`v1`
- Behavior:`闭环 Cloud/Postgres 真实 E2E、物理 DPR 验收、Windows 跨卷权限现场、移动端新 profile 问题与 Sonar 覆盖率基线；随后从 NotebookLM 来源、Note、近期及新发起对话提取下一批候选愿景/bug，经本地代码与运行证据核验后落地，并完成对应测试与质量流程。`
- Boundary:`不执行 git push、tag、Release、Remote/Cloud 发布或激活；不向 NotebookLM 写入/删除/上传 source/note；保留用户既有 dirty worktree；真实现场项必须有运行产物或明确阻塞证据，不以单测替代物理验收。`
- Acceptance:`request/intake 与 requirements_gate 通过；Cloud/Postgres、DPR、跨卷权限、mobile profile、Sonar baseline 各有匹配的运行/扫描证据；下一批 NLM 候选有来源或明确无来源记录、CodeGraph trace、实现 diff、目标测试；全量测试/check、CodeGraph 复核、Sonar 复扫与迭代归档完成。
- Traceability:`REQ-NLM-CLOSURE-20260809 → local intake → NLM read-only candidate → runtime/CodeGraph evidence → implementation → test/quality evidence → iteration archive`

### REQ-SCM-GIT-SCAN-DEPTH-01 · Git discovery hard cap

- 批准依据:`用户预审批通过；NLM 对话 a47d3199-c1f9-47f1-927c-ff2c4875b77d 第 10 轮明确提出`
- 状态:`ACTIVE`
- 版本:`v1`
- 行为:`find_git_repos_below` 保留协议参数兼容性，但实际向下扫描深度永远不超过 1；调用方传入更大值或省略值均不得触发 deep scan。
- 边界:仅约束 SCM 仓库发现；不改变 Git 根向上解析、已命中仓库内部不递归、跳过大型目录及并发/超时/进程生命周期护栏。
- 验收:直接子目录仓库可发现；二级及更深仓库在 `Some(2)`、`Some(99)`、`None` 下均不可发现；SourceControl 传入深度 1。
- 追踪:`REQ-SCM-GIT-SCAN-DEPTH-01` → `find_git_repos_below_sync` → SourceControl → Rust/TS tests

### REQ-SCM-PANE-ROOT-PILL-01 · Pane branch pill follows cwd Git root

- 批准依据:`用户预审批通过；NLM 对话 a47d3199-c1f9-47f1-927c-ff2c4875b77d 第 10 轮明确提出`
- 状态:`ACTIVE`
- 版本:`v1`
- 行为:pane 的 branch/diff pill 仅展示 `find_git_repo_root(cwd)` 返回的自身或祖先 Git 根；cwd 非 Git 仓库时，即使其子孙目录含仓库，也不得展示子孙分支 pill。
- 边界:不改变 SourceControl 全局侧栏对工作区仓库的独立发现；不按 UI 本地猜测 Git 根；pane 选择状态不得跨 cwd/root 泄漏。
- 验收:非 Git 父目录含多个子仓库时 pane store 为 `null`；Git 仓库子目录解析到祖先根且 `availableRepos` 仅含该根；离开仓库立即清空 pill；确定性 Vitest 覆盖上述分支。
- 追踪:`REQ-SCM-PANE-ROOT-PILL-01` → `find_git_repo_root` → `resolveInfoForPane` → PaneGitPill/PaneDiffPill → Vitest

### REQ-SONAR-COVERAGE-80-01 · Sonar 全项目测试覆盖率 ≥80%

- Status:`ACTIVE`
- Version:`v1`
- Approval evidence:`用户明确要求将 Sonar 测试覆盖率提升到 80% 以上并纳入下一迭代目标。`
- Behavior:`下一迭代建立可复现的 Sonar coverage 基线，补齐测试并使 Sonar 项目级测试覆盖率（非仅 new_coverage）达到至少 80%。`
- Boundary:`覆盖率提升须由真实 Sonar 扫描与 Quality Gate/API 结果证明；不得以受限文件扫描、局部 new-code coverage、静态估算或排除未测代码冒充全项目达标；保留既有 dirty worktree，不借机改写历史基线。`
- Acceptance:`扫描退出成功；Sonar project status 为 OK；coverage >= 80.0%；扫描输入、测试命令、覆盖率报告、项目级指标与失败原因均留有脱敏证据；若本轮受环境/基线阻塞，须记录差距分解，不得宣称完成。`
- Current iteration evidence:`完整扫描 scanner/CE 均 SUCCESS，802 文件；Sonar coverage 40.3%、line 41.2%、branch 38.9%，Quality Gate ERROR（new_coverage 47.6%、new_violations 133）。证据：docs/iterations/2026-08-09-sonar-full-scan-evidence.md；故本需求仍 ACTIVE。`
- Traceability:`REQ-SONAR-COVERAGE-80-01 → coverage baseline → uncovered-code test waves → Sonar scan/Quality Gate → iteration archive`

### REQ-AGENT-COMMUNICATION-ARCH-REBUILD-01 · Agent 通信架构重构：Kernel/Teammate 权威、类型化消息与跨端一致投影

- Status:`ACTIVE`
- Version:`v1`
- Approval evidence:`用户明确要求深化 NotebookLM 笔记“Ridge 项目现状、愿景与规划基线（2026-07-21）”中的来源“Agent 通信架构重构”，并将其归纳为一个详尽需求。`
- Source evidence:`NotebookLM source 9516749e-c317-4f13-9cda-b64b00cec465（Agent 通信架构重构）；关联笔记 66919cb9-1329-4ddf-955c-f426d15a9fe6；临时对话 source df4d5dcc-9813-4c61-ae9f-1e9199cb7555，关联笔记 f6ffd900-708d-44ee-9818-1a3269c533fc，作为同等重点来源。临时来源补充 cookie/API 边界、MIME/大小/SHA256、轮询/重试上限、token/cookie 脱敏与取消穿透约束。NLM 仅作候选与架构假设，代码、测试、运行事实为最终依据。`
- Behavior:`将 Agent 的身份、生命周期、通信、历史、恢复、编组与状态投影收敛为一个 Kernel/Teammate 权威的通信平面；桌面、Remote、rdg CLI、ridge-mcp 与 headless host 均通过同一契约访问，禁止 UI、CWD、pane 标题或单一 Tauri invoke 路径自行推断身份或维护第二份事实。Domain model：(1) 稳定 Agent identity 必须至少包含 agent_id、session_id、workspace_id、pane_id、CWD、executable/argv、lifecycle generation、lease、status、online、last_seen、capabilities；(2) 生命周期采用可观测状态机 discovered → spawning/attaching → online → working/waiting/attention → completed/stopped/failed，创建仅在 spawn/attach 成功后提交，销毁仅在 destroy/lease closure 成功后提交，部分失败保留 diagnostic-only 记录；(3) 每次重连/复用递增 generation 并以 lease fencing 拒绝旧 Agent 的迟到消息；(4) 编组成员、leader、顺序、加入/移除均以显式 group_id 与 agent_id 维护，不按标题/CWD 猜测；(5) 所有 task/event/control/artifact/reply 进入统一 envelope，至少包含 message_id、idempotency_key、conversation/task_id、from/to identity、workspace/pane、generation/lease、kind、sequence、timestamp、priority、deadline/cancellation、payload/artifact 引用、ack/nack 与 typed error；(6) 发送前取得有界 roster 快照并校验 target identity、generation、lease、online，过期时最多一次有界刷新，随后单次发送或返回 missing/offline/stale/generation_mismatch/capability_denied/timeout 等稳定错误；(7) Kernel/Teammate service 为 SSOT，ridge-mcp、rdg CLI、桌面 AgentCenter/Commune、Remote roster/history 与 headless API 均为适配器/投影，复用同一 identity/lifecycle/routing/ack/error/idempotency 语义；(8) 通信、恢复、历史、PTY/渲染回调均有界、可取消、可观测，输入/控制/HITL 优先于 history/render，恢复 single-flight，销毁取消 pending RPC/timer/listener/worker/queue，重连不得向旧 generation 交付；(9) 历史为独立 cold path，按稳定 identity/session_id 分组，失败局部可见；恢复使用结构化 executable/argv/CWD/session identity，不拼接未经转义 shell 字符串；(10) 桌面与 Remote 对同一状态显示相同 status/label/attention/aria-label/identity/history 语义，跨 workspace 必须携带 workspace_id；(11) 入口校验 workspace/agent/generation/lease、能力与权限，日志不得记录 cookie/token/浏览器存储或未脱敏敏感 payload，并暴露 trace_id/message_id、ack latency、queue depth、cancel、stale rejection、teardown residue 与恢复结果。`
- Source-derived design details:`MCP 保留为通信控制面，不再直接写 PTY；Message Hub 负责 inbox/topic/task/event/artifact 与 delivery，第一阶段可采用 Rust + SQLite 持久化、内存事件总线/WebSocket，业务模型不得与具体 NATS 实现耦合。MCP 工具契约至少覆盖 ridge_send_message、ridge_create_task、ridge_publish_event、ridge_fetch_inbox、ridge_task_update、ridge_list_agents；工具返回 message/task/delivery ID 与 typed error，不返回“已写入终端”即算完成。投递优先级固定为 Runtime API/SDK/app-server/HTTP/ACP → A2A（跨系统/远程）→ MCP pull → PTY fallback；来源中对各 CLI 官方接口的判断须在本仓以 capability probe/运行证据复核，不得硬编码未经验证的能力。PTY 仅在 agent.status=idle、terminal.mode=agent_prompt、pending_approval=false、shell_foreground_process=target_agent 且无用户输入竞争时尝试，记录 delivery_adapter=pty、delivery_reliability=best_effort；否则保留 Inbox，取消/退避，不打断用户。消息进入 Hub 与 Agent 开始处理必须分离，UI 关闭后 Hub/任务/订阅仍可恢复；A2A 只作外部适配器，不替代内部高频事件总线。迁移分三阶段：先剥离 MCP→PTY 建立 Hub pipeline，再建立 Agent Runtime 状态矩阵与 Delivery Engine，最后接入 A2A/跨系统 Artifact。`
- Boundary:`范围包括 packages/ridge-kernel、packages/ridge-mcp、src-tauri/src/teammate、rdg/teammate adapters、src/lib/teammate、AgentCenter/Commune、src/remote Agent roster/history/group surfaces、共享 DTO 与确定性测试；复用现有 PTY、pane、workspace-singleton、Remote transport、capability/process-guard、E2EE/TOTP 合同；不新增 UI-local identity directory，不按标题/CWD 猜测 Agent，不改变 Remote wire protocol，不绕过 Kernel/capability/process guard，不写回第三方 session 历史，不引入无界 retry、silent respawn 或第二持久化事实源。`
- Acceptance:`(1) CodeGraph trace 覆盖 Kernel registry/lifecycle → ridge-mcp/rdg adapters → desktop/Remote projections → tests；(2) deterministic tests 证明成功 create/destroy 各一次、spawn/attach/destroy 失败不产生错误 active entry、旧 generation/lease/离线目标在发送前拒绝、一次 refresh 不重复 spawn、相同 idempotency_key 并发发送只产生一条消息、取消/销毁后 pending/timer/listener/worker/queue 归零；(3) 多 Agent、多 workspace、多 CWD、多 session 的 desktop/Remote/headless fixture 证明 identity 不依赖标题/CWD，group CRUD、leader、顺序、history tab 与 status/aria-label 同源；(4) history cold scan 不阻塞 live roster/input，损坏超大 JSONL 局部失败且可诊断，resume 结构化恢复在当前 workspace 创建唯一 pane，重复操作单飞；(5) ridge-mcp 在无 Tauri 桌面时可 initialize/tools/list 并执行至少一种 roster/communication 工具，Kernel 退出后返回可观察 typed failure；(6) 实际或等价 multi-Agent E2E 覆盖 create→communicate→ack→reconnect generation→destroy，证明无重复、无旧消息投递、无 pending 泄漏；(7) 全量测试/check、相关 Rust/TypeScript/Remote 回归与 Sonar 质量闸通过，新增问题为 0，失败原因与运行产物留档。`
- Current iteration evidence:`Kernel/Teammate SSOT、typed envelope、generation/lease fencing、SQLite Hub、MCP tools、adapter probe/priority/outcome、desktop/Remote projections 与 lifecycle E2E 已有确定性证据；Kernel/桌面/headless 已接入容量 256、try_send、generation/lease fencing 的 host-owned Runtime API/A2A registry。共享 `GET /api/v1/agent-events/ws` bridge 现已支持 token + 当前 roster 身份授权、跨进程注册、真实 Hub envelope 投递、ACK 与断连注销，等价 multi-Agent TCP E2E 已通过；第三方 CLI 私有 Runtime/A2A 协议仍未冒充或宣称兼容。Kernel PTY destroy 会注销被移除 identity 的路由；PTY 五条件原子运行时证明及 Sonar 80%/Gate 仍未闭环。最新测试与差距见 docs/iterations/2026-08-09-agent-communication-qa-handoff.md 与 docs/iterations/2026-08-09-agent-communication-wave15-handoff.md。`
- Traceability:`REQ-AGENT-COMMUNICATION-ARCH-REBUILD-01 → NLM source 9516749e-c317-4f13-9cda-b64b00cec465 → Kernel/Teammate SSOT → typed envelope/lifecycle/registry → ridge-mcp/rdg/desktop/Remote/headless adapters → deterministic unit/integration/multi-Agent E2E → Sonar/iteration archive`
## 本轮质量证据补充（2026-08-10 Wave35）

- `src/lib/stores/paneTree.test.ts` 新增 DOM 几何、同轴吸附、junction 去重、拖拽更新与释放清理的确定性测试；聚焦测试 `81/81` 通过。
- 全量 `pnpm test:coverage:sonar`：`187` 个测试文件，`1759 passed / 1 skipped`；本地 V8/LCOV statements `11495/18537 = 62.01%`、branches `6373/11542 = 55.21%`、functions `2233/3527 = 63.31%`、lines `10386/15831 = 65.60%`。
- 该结果距 80% 尚缺 `3335` 条已覆盖 statements；`.mjs` 解析告警、本机无 Sonar scanner/token/host 仍阻塞真实 project metric 与 Quality Gate。故 `REQ-SONAR-COVERAGE-80-01` 仍 ACTIVE，不宣称达标。

## 本轮质量证据补充（2026-08-10 Wave36）

- `src/remote/lib/cloudRemote.test.ts` 新增 7 个确定性状态机测试，覆盖初始化降级、metadata payload 校验、双 seed 失败后的 live 保活、订阅注册失败重试、resume seed 跳过、历史页异常/at-oldest 与历史 RPC 恢复；聚焦 `47/47` 通过。
- 全量 `pnpm test:coverage:sonar`：`187` 个测试文件，`1766 passed / 1 skipped`；本地 V8/LCOV statements `11507/18537 = 62.07%`、branches `6391/11542 = 55.37%`、functions `2232/3527 = 63.28%`、lines `10398/15831 = 65.68%`。
- 相较 Wave35，covered statements 增加 `12`；距 80% 尚缺 `3323` 条。`.mjs` `PARSE_ERROR` 及本机缺 `SONAR_TOKEN`/`SONAR_HOST_URL`、真实 Sonar CE/Gate 证据仍未闭合，故 `REQ-SONAR-COVERAGE-80-01` 继续 ACTIVE，不宣称达标。
- 质量门：`pnpm check` 0 errors / 0 warnings；`cargo fmt --all -- --check`、`git diff --check` 通过；coverage 与 `.iteration` 运行态产物保留 dirty，不纳入提交。

## 本轮质量证据补充（2026-08-10 Wave37）

- `src/lib/stores/hostsPublic.test.ts` 新增共享工作区投影、远端 mutation 失败闭闸测试；聚焦 `10/10` 通过。
- `packages/remote/src/shared/terminal/manager.test.ts` 新增 shared-host resize 边界、workspace invalidate、shell snapshot 异常与 TUI cursor anchor 测试；聚焦 `13/13` 通过。
- 全量 `pnpm test:coverage:sonar` 通过；最新本地 V8/LCOV statements `11603/18537 = 62.59%`、branches `6453/11542 = 55.91%`、functions `2247/3527 = 63.71%`、lines `10471/15831 = 66.14%`；相较 Wave36 新增 `4` 个测试，距 80% 尚缺 `3227` 条 covered statements。
- `.mjs` `PARSE_ERROR`、本机缺 `SONAR_TOKEN`/`SONAR_HOST_URL` 与真实 Sonar CE/Gate 仍未闭合，故 `REQ-SONAR-COVERAGE-80-01` 继续 ACTIVE，不宣称达标。PTY 五条件保持 fail-closed，宿主完整运行时快照仍待补齐。

## 本轮架构证据补充（2026-08-10 Wave38）

- `packages/ridge-mcp/src/delivery.rs` 新增 host-owned PTY safety proof 注册表：proof 同时绑定 `agent_id/generation/lease`，同代换 lease、旧代注册、旧代注销均拒绝；同代刷新原子替换安全快照。
- proof 设有 `PTY_SAFETY_MAX_AGE=3s` 新鲜度闸；过期或 identity 不匹配即恢复默认空 probe，不能选择 PTY，MCP pull 仍为可恢复路径。
- `McpSessionState` 暴露注册/注销入口；Kernel `remove_agent_identity_for_pty` 在 PTY destroy 成功后同时撤销 Runtime API、A2A 与 PTY proof，避免旧代安全证明残留。
- 确定性证据：`ridge-mcp` `92/92`；`ridge-kernel` `49/49`；`cargo fmt --all -- --check` 通过。该波仅补齐“证明存储、刷新与销毁 fencing”基础设施，未伪造宿主五条件的原子运行时快照，PTY 生产放行仍保持 fail-closed。
- Sonar 80% 与 Quality Gate 仍未闭合：本机缺 scanner/token/host，且当前本地 coverage 仅 `62.59%`；不得以本地 LCOV 代替 Sonar project metric。

## 修订账本 (Revision Ledger)

| 版本 | 日期 | Pending ID | 变更 | 关联/取代 | 批准证据 |
| --- | --- | --- | --- | --- | --- |
| v0.3.68 | 2026-08-04 | `<ITERATION-167-CLOUD-PANE-BINDING>` | Detached Cloud Remote now persists a non-secret kernel PTY binding, passes its path explicitly to `rdg remote --daemon`, reattaches by exact identity/profile/deterministic CWD, and removes the first-live-PTY fallback; lease persistence failure detaches the lease | Closes the code portion of the multi-pane/Cloud binding residual under `REQ-20260730-01`, `REQ-RDG-REMOTE-CONNECT-01`, and `REQ-RIDGE-KERNEL-HOST-01`; physical/public four-path Remote, force-kill phone reconnect, WebView2 soak, dual-window/Host, and full Kernel authority remain external | Iteration 167 archive; ridge-cli 134 unit + 3 integration, ridge-kernel 46, Tauri check, `pnpm check` 0/0 |
| v0.3.69 | 2026-08-04 | `<ITERATION-167-MOBILE-BACKPRESSURE>` | Pre-approved Mobile Remote bounded render/input backpressure added to the active iteration; existing scheduler/input gates retained, while the synchronous deferred-feed catch-up and contiguous queue-growth risk are targeted for removal | Adds `REQ-MOBILE-REMOTE-BACKPRESSURE-01`; no release closure is claimed until flood/lifecycle tests and mobile/public soak pass | User direct pre-approval; implementation and deterministic tests in progress |
| v0.3.70 | 2026-08-04 | `<ITERATION-167-MOBILE-LIVE-TAIL>` | Mobile Remote now treats the live PTY tail as authoritative: listener attaches before the bounded visual seed, live bytes render immediately with a bounded FIFO replay copy for seed completion, and older history loads only on upward scroll | Adds `REQ-MOBILE-REMOTE-LIVE-TAIL-01`; no replay/live-gap closure claimed until ordering, reconnect, and mobile soak evidence pass | User direct approval in current iteration; implementation and deterministic live-before/after-seed test added |
| v0.3.71 | 2026-08-04 | `<ITERATION-167-PANE-GRID-INVARIANT>` | Pane content-box geometry is the local terminal-grid source of truth; stale host pty-resize dimensions trigger a local re-fit/claim instead of overwriting the pane grid, with a manual force-fit recovery path | Adds `REQ-REMOTE-PANE-GRID-INVARIANT-01`; no geometry closure claimed until resize/orientation/PWA and cross-host evidence pass | User direct approval in current iteration; geometry fixture and resize-path guards added |
| v0.3.72 | 2026-08-05 | `<ITERATION-167-RELEASE-GATE-REMEDIATION>` | Release test gate builds the target Linux `rdg` sidecar; Intel macOS matrix job explicitly stages `rdg-x86_64-apple-darwin` before Tauri | Closes deterministic missing `externalBin` blockers only; versioned release still requires the full matrix and asset audit | Failure logs `30922915073`, `30930418932`; retry `30934352592` canceled after bounded duplicate sidecar build and is pending final rerun |
| v0.3.73 | 2026-08-05 | `<ITERATION-167-PROCESS-GUARD-BUSY-RETRY>` | The single external-process spawn gate retries only transient `ExecutableFileBusy` for a bounded 85 ms window, preserving wall-clock timeout, cancellation, and process-tree kill semantics | Hardens the Git/SCM process-lifecycle guard exposed by the release test gate; no release closure until the corrected matrix and asset audit pass | `cargo test -p ridge-core --lib -- commands::git process_guard`: 42 passed; CI failure `30937863004` recorded as transient spawn error |
| v0.3.74 | 2026-08-05 | `<ITERATION-168-INTERACTION-PARITY>` | Access/share onboarding contracts remain unified; Remote spaces/punctuation and TUI touch mouse paths are repaired; desktop IME fallback, free Explorer resize, Agent parity/spacing, narrow file/image viewing, and confirmed Agent registry preflight are implemented | Advances `REQ-INTERACTION-PARITY-01` and `REQ-AGENT-COMMUNICATION-REGISTRY-01`; physical mobile/PWA, real TUI, public WebRTC, WebView2, and multi-window/Host evidence remain external; no versioned release made | NotebookLM transcript `a47d3199-c1f9-47f1-927c-ff2c4875b77d`; `pnpm check` 0/0; Vitest 33 + shared terminal helpers 46 passed; Rust profile/input suites passed |
| v0.3.75 | 2026-08-05 | `<ITERATION-171-REMOTE-LINK-FLUIDITY>` | Remote validated URL/path hits now open on bare primary click, mobile Pane switching drains only a bounded live catch-up slice with resync on overflow, Cloud active-pane promotion is latest-wins, and bounded stage/latency telemetry separates input, transport, feed, and first paint | Advances `REQ-REMOTE-LINK-FLUIDITY-01`, `REQ-REMOTE-SMOOTH-STATE-02`, and `REQ-REMOTE-RUNTIME-PERF-MEMORY-02`; Remote/Cloud artifact is active from `e94d8c5`; physical/mobile soak and Desktop release remain open | `pnpm check` 0/0; full Vitest 153/1577/1; workflow `30987238096` success; public bundle signature verified; NLM query returned `RESOURCE_EXHAUSTED`, no note created |
| v0.3.76 | 2026-08-05 | `<ITERATION-171-CLOUD-INPUT-PRIORITY>` | Cloud ordered transport now bounds active pane buffering at 256 KiB (64 KiB drain), splits pane bursts into 32 KiB frames before E2EE sealing, and sends control/input through a bounded priority queue without breaking strict E2EE counters | Advances the input-first portion of `REQ-REMOTE-LINK-FLUIDITY-01`, `REQ-MOBILE-REMOTE-BACKPRESSURE-01`, and `REQ-REMOTE-RUNTIME-PERF-MEMORY-02`; code `67417a9` is pushed but deliberately not artifact-published today because the daily release cap is exhausted; physical phone/PWA soak remains open | Priority queue/cloud bridge deterministic tests; `pnpm check` 0/0; next Remote/Cloud artifact must contain `67417a9` |
| v0.3.77 | 2026-08-05 | `<ITERATION-171-CLOUD-DUAL-LANE>` | Cloud Remote now uses an optional `ridge-pane` ordered DataChannel on the same authenticated PeerConnection for PTY bulk output; `ridge` remains the control/input lane, each lane has independent E2EE counters and chunk IDs, and legacy peers fall back to `ridge` only after authenticated probe/ready negotiation | Removes the remaining ordered-channel head-of-line path for `REQ-REMOTE-LINK-FLUIDITY-01` and `REQ-MOBILE-REMOTE-BACKPRESSURE-01`; code `150272a` is pushed but not artifact-published today because the daily release cap is exhausted; physical phone/PWA soak and online activation remain open | Host/controller routing, controller outbound lane, probe/ready negotiation, and legacy-fallback tests; full Vitest 154/1584/1; `pnpm check` 0/0; both mobile and desktop Remote builds passed |
| v0.3.78 | 2026-08-05 | `<ITERATION-171-FOREIGN-PANE-QOS>` | Desktop foreign Remote Pane bindings now mark the initial subscription active and promote the focused pane through an idempotent `subscribe-pane(active:true)` notification; reconnect clears the promotion guard so the next focus crosses the new Host session | Closes the remaining Host-topology active-pane starvation path under `REQ-REMOTE-LINK-FLUIDITY-01`; code `f5e9c2b0` is pushed but not artifact-published today because the daily release cap is exhausted; physical phone/PWA soak and online activation remain open | `cloudHostTopologyLink.test.ts` promotion/reconnect coverage; full Vitest 154/1585/1; `pnpm check` 0/0 |
| v0.3.79 | 2026-08-05 | `<ITERATION-171-FOREIGN-PANE-RESUBSCRIBE>` | Desktop foreign Remote bindings retain only actually attached Pane subscriptions and replay them after a full WebRTC reconnect; the focused Pane is restored with `active:true`, while panes merely discovered during layout projection are not subscribed | Closes the reconnect-induced live-tail starvation path under `REQ-REMOTE-LINK-FLUIDITY-01`; code `7bbcae00` is pushed but not artifact-published today because the daily release cap is exhausted; physical phone/PWA soak and online activation remain open | `cloudHostTopologyLink.test.ts`: 7 passed; full Vitest 154/1586/1; `pnpm check` 0/0 |
| v0.3.80 | 2026-08-05 | `<ITERATION-171-FOREIGN-PANE-CLOSE-ROLLBACK>` | Failed `closePane`/`closeWorkspace` operations now restore the attached-pane subscription snapshot and focused-pane QoS state along with live lifecycle state, so a rejected close cannot silently lose live output after reconnect | Closes the rollback half of the reconnect subscription lifecycle under `REQ-REMOTE-LINK-FLUIDITY-01`; code `c9b8540d` is pushed but not artifact-published today because the daily release cap is exhausted; physical phone/PWA soak and online activation remain open | `cloudHostTopologyLink.test.ts`: 8 passed; full Vitest 154/1587/1; `pnpm check` 0/0 |
| v0.3.81 | 2026-08-05 | `<ITERATION-171-REMOTE-NETWORK-ATTRIBUTION>` | Source and wire evidence separate the bottlenecks: signaling relay carries only offer/answer/ICE, E2EE PTY/input uses WebRTC DataChannel, TURN is optional, and public static assets are gzip/immutable; proxy/route throughput is measured while WebRTC candidate/bitrate remains an explicit physical-soak gate | Advances the attribution/acceptance portion of `REQ-REMOTE-LINK-FLUIDITY-01`; no TURN capacity or device upgrade is justified until the trace records candidate type (`relay` vs `host`/`srflx`), RTT, bitrate, queue latency, and Pane first paint | `ridge-cloud` source audit; public `Server: nginx` + `Content-Encoding: gzip`; local proxy/direct fetch comparison; physical phone/PWA trace still open |
| v0.3.82 | 2026-08-05 | `<ITERATION-171-REMOTE-SUBSCRIBE-RETRY>` | Mobile Cloud `subscribePane` now retains intent through transient listener/registration failure, retries at 100/200/400/800 ms with a four-retry cap, and cancels retry state on Pane destruction, disconnect, or reconnect; active promotion remains serialized | Closes the transient “Pane switch succeeded but live tail never attaches” path under `REQ-REMOTE-LINK-FLUIDITY-01` and `REQ-MOBILE-REMOTE-BACKPRESSURE-01`; code `e8316548` is pushed but not artifact-published today because the daily release cap is exhausted; physical phone/PWA soak remains open | `cloudRemote.test.ts`: 40 passed; full Vitest 154/1589/1; `pnpm check` 0/0; `pnpm build:remote:mobile` passed with 38-entry PWA precache |
| v0.3.66 | 2026-08-04 | `<ITERATION-162-KERNEL-GIT-STASHES>` | Remote `git_stash_list` now uses authenticated kernel `/v1/domain/git/stashes`; non-Git detection precedes `git stash list` and returns a typed empty projection | Advances `REQ-RIDGE-KERNEL-DOMAIN-01`, SCM non-Git suppression, and Remote stability; remaining Git writes/graph, physical Remote/WebView2, dual-window/Host, and complete kernel authority evidence remain open | Kernel stash decoder/non-Git guard passed; ridge 256, kernel 44, core 315; Vitest 148/1541/1; `pnpm check` 0/0 |
| v0.3.67 | 2026-08-04 | `<ITERATION-163-KERNEL-GIT-ALL>` | All desktop Git mutations and graph/history reads now use authenticated tagged kernel domains; Tauri signatures remain compatible, non-Git preflight is fail-closed, and no direct desktop Git child remains for these paths | Advances `REQ-RIDGE-KERNEL-DOMAIN-01` and the SCM/process convergence portion of `REQ-20260730-01`; physical/public Remote, WebView2 heap soak, dual-window/Host, mobile attribution, and full domain convergence remain external | Kernel read/mutation decoder and non-Git guards passed; desktop compile passed; full matrix recorded in the archive |
| v0.3.65 | 2026-08-04 | `<ITERATION-161-KERNEL-GIT-FAST-STATUS>` | Remote `get_scm_status_fast` now uses authenticated kernel `/v1/domain/git/status?path=…&fast=1`; kernel preserves no-numstat fast semantics and rejects confirmed non-Git roots before Git work | Advances `REQ-RIDGE-KERNEL-DOMAIN-01`, Remote first-paint latency, and SCM process convergence; remaining Git writes/graph, physical Remote/WebView2, dual-window/Host, and complete kernel authority evidence remain open | Kernel fast-status non-Git guard and URL contract passed; ridge 256, kernel 42, core 315; Vitest 148/1541/1; `pnpm check` 0/0 |
| v0.3.64 | 2026-08-04 | `<ITERATION-160-KERNEL-GIT-DIFF-SUMMARY>` | Desktop `git_diff_summary` now travels through authenticated kernel `/v1/domain/git/diff-summary`; stage/unstage/commit/checkout/push/push-branch use the tagged `/v1/domain/git/mutate` route; repository detection precedes Git work and non-Git roots return typed negative results | Advances `REQ-RIDGE-KERNEL-DOMAIN-01`, SCM process lifecycle, and startup performance; remaining Git writes/graph, physical Remote/WebView2, dual-window/Host, and complete kernel authority evidence remain open | Kernel diff-summary/mutation decoder and non-Git guard tests passed; ridge 256, kernel 41, core 315; Vitest 148/1541/1; `pnpm check` 0/0 |
| v0.3.63 | 2026-08-04 | `<ITERATION-159-SIDEBAR-LAZY-MOUNT>` | Desktop sidebar Git/Search/Remote/Agents/Hosts/Files panels now mount only after first visit and retain visited instances across tab switches, preventing hidden panel queries, listeners, and projections from competing during first paint | Advances `REQ-20260730-01` startup/tab responsiveness; physical frame traces, black-screen reproduction, WebView2 heap soak, Remote/Host, dual-window, and complete kernel-domain evidence remain open | `SidebarLazyMount.test.ts`: 2 passed; `pnpm check`: 0 errors / 0 warnings |
| v0.3.62 | 2026-08-04 | `<ITERATION-158-SETTINGS-LAZY-LOAD>` | Desktop `SettingsPanel` is removed from the static route module graph and loaded on first open; the resolved component stays mounted for later opens, while theme/wallpaper generation guards remain unchanged | Advances `REQ-20260730-01` startup/settings responsiveness; physical WebView2 startup, tab-switch black-screen, heap-soak, Remote/Host, dual-window, and full kernel-domain evidence remain open | `SettingsPanel.test.ts`: 5 passed; `pnpm check`: 0 errors / 0 warnings |
| v0.3.61 | 2026-08-04 | `<ITERATION-157-KERNEL-WATCHER-HEALTH>` | Desktop kernel watcher now combines exact PID/process and authenticated endpoint health, tolerates transient failures, and exits after three consecutive health failures; tray kernel shutdown marks `quitting` before shutdown and rolls back on failure | Advances `REQ-RIDGE-KERNEL-HOST-01` lifecycle/restart safety; physical tray/restart and health-fault evidence, orphan recovery UX, public/physical Remote, WebView2 soak, dual-window/Host, and complete domain-authority evidence remain open | `cargo test -p ridge --lib kernel_lifecycle::tests`: 7 passed |
| v0.3.60 | 2026-08-04 | `<ITERATION-156-KERNEL-REATTACH-HISTORY>` | Desktop restart reattach now starts kernel output leases at the bounded retained window, preserving pre-restart output; unmatched kernel PTYs are counted and warned without implicit destruction | Advances PTY lifecycle/restart/memory safety under `REQ-RIDGE-KERNEL-HOST-01` and `REQ-20260730-01`; physical same-PID reattach, orphan recovery UX, health-aware watcher, public/physical Remote, WebView2 soak, dual-window/Host, and complete domain-authority evidence remain open | `cargo test -p ridge --lib pty_lifecycle_contract_tests`: 3 passed |
| v0.3.59 | 2026-08-04 | `<ITERATION-155-KERNEL-GIT-BRANCHES>` | Desktop branch-list reads now use authenticated kernel `/v1/domain/git/branches`; repository detection precedes `git branch`, confirmed non-Git roots return a negative result without repeated children, and existing UI cache/signature remain stable | Advances `REQ-RIDGE-KERNEL-DOMAIN-01` and SCM polling/process-lifecycle convergence; Git mutations, physical reattach, orphan reporting, health-aware watcher, public/physical Remote, WebView2 soak, dual-window/Host, and complete domain-authority evidence remain open | Kernel client branch contract 1 passed; kernel non-Git domain test 1 passed; `cargo check -p ridge --lib` exit 0 |
| v0.3.58 | 2026-08-04 | `<ITERATION-154-KERNEL-BOOT-SINGLE-FLIGHT>` | Desktop kernel bootstrap is process-local single-flight across detect/spawn/readiness; an existing live PID receives a bounded same-PID health wait before attach, preventing duplicate kernels and startup false failures | Advances `REQ-RIDGE-KERNEL-HOST-01` and PTY restart safety; physical reattach, orphan reporting, health-aware watcher, public/physical Remote, WebView2 soak, dual-window/Host, and complete domain-authority evidence remain open | `cargo test -p ridge --lib kernel_lifecycle::tests`: 6 passed |
| v0.3.57 | 2026-08-04 | `<ITERATION-153-KERNEL-GIT-STATUS>` | Desktop `get_scm_status` now reads the authenticated kernel Git domain with source-checked typed decoding, confirmed non-Git detection, and URL-safe path transport; Query/slot callers retain their existing contract | Advances `REQ-RIDGE-KERNEL-DOMAIN-01` and SCM polling/process-lifecycle convergence; branch/list mutation, physical/public Remote, WebView2 soak, dual-window/Host, and complete domain-authority evidence remain open | `cargo test -p ridge-kernel --lib client::tests::domain_git_status`: 2 passed; `cargo check -p ridge --lib`: exit 0 |
| v0.3.56 | 2026-08-04 | `<ITERATION-152-KERNEL-PTY-AUTHORITY>` | Desktop pane PTY creation is kernel-authoritative in production; bootstrap/list/create/attach failures are surfaced, `initial_command` is rejected without structured argv/env, and native pending-spawn remains a test-only seam | Advances `REQ-RIDGE-KERNEL-HOST-01`, `REQ-RIDGE-KERNEL-DOMAIN-01`, and PTY lifecycle/memory safety; physical restart/reattach, public/physical Remote, WebView2 soak, dual-window/Host, and complete domain-authority evidence remain open | `cargo check -p ridge --lib` exit 0; `cargo test -p ridge --lib` 253 passed; focused PTY lifecycle contract 2 passed; `git diff --check` passed |
| v0.3.55 | 2026-08-03 | <ITERATION-151-REMOTE-PEEK-ERROR-FEEDBACK> | Mobile Remote non-active workspace peeks retain the last good panes and show a per-workspace error when pane discovery fails, instead of silently presenting an empty terminal list | Advances REQ-RDG-REMOTE-CONNECT-01 and Remote Host discovery/feedback reliability; physical/public Host latency and full Kernel authority gates remain open | focused Host/Cloud/WorkspaceTree Vitest: 50 passed; full Vitest: 147 files / 1538 passed / 1 skipped; pnpm check 0/0 |
| v0.3.54 | 2026-08-03 | `<ITERATION-150-REMOTE-LEGACY-RESPONSE-GUARD>` | LAN legacy `workspace-panes` replies now pass an echoed-workspace guard; stale replies are ignored and cannot erase another workspace as an empty snapshot | Advances `REQ-RDG-REMOTE-CONNECT-01` and Remote Query/transport lifecycle reliability; physical/public Host latency and full Kernel authority gates remain open | focused LAN scheduler Vitest: 11 passed; full Vitest: 147 files / 1537 passed / 1 skipped; `pnpm check` 0/0 |
| v0.3.53 | 2026-08-03 | `<ITERATION-149-REMOTE-WORKSPACE-CREATE-FEEDBACK>` | Remote Cloud workspace creation now propagates RPC/auth failures and mobile UI reports empty-ID results, removing the silent no-op path after a create tap | Advances `REQ-RDG-REMOTE-CONNECT-01` and Remote workspace operation feedback; physical/public Host latency and full Kernel authority gates remain open | focused Host/Cloud/WorkspaceTree Vitest: 44 passed |
| v0.3.52 | 2026-08-03 | `<ITERATION-148-AGENT-IDLE-IDENTITY>` | Cloud Host and mobile Cloud pane projections retain Agent identity for `idle`, `starting`, and `busy` runtime states instead of exposing it only while busy | Advances `REQ-AGENT-COMMUNE-UI-02`, `REQ-AGENT-COMMUNE-REMOTE-PARITY-01`, and Pane attention continuity; physical/public mobile and full authority gates remain open | focused Cloud topology/Remote Vitest: 39 passed |
| v0.3.51 | 2026-08-03 | `<ITERATION-147-REMOTE-HOST-ERROR-VISIBILITY>` | Cloud Remote workspace and pane discovery now propagate RPC failures instead of converting them to empty lists, so Host/Query UI can retain last-good state and expose actionable loading/error/retry feedback | Advances `REQ-RDG-REMOTE-CONNECT-01` and remote Query/lifecycle reliability; physical/public Host and full Kernel authority gates remain open | `pnpm exec vitest run src/remote/lib/cloudRemote.test.ts`: 35 passed |
| v0.3.50 | 2026-08-03 | `<ITERATION-146-REMOTE-AGENT-LIVE-TITLE>` | Remote Agent roster now carries and renders the host-authoritative live PaneHeader/OSC title while retaining stable Agent identity and CWD fields | Advances `REQ-AGENT-COMMUNE-UI-02` and `REQ-AGENT-COMMUNE-REMOTE-PARITY-01`; physical/public mobile and full authority gates remain open | focused Remote Vitest 24 passed; `pnpm check` 0/0 |
| v0.3.49 | 2026-08-03 | `<ITERATION-145-REMOTE-GIT-NEGATIVE-CACHE>` | Remote/mobile sidebar now keeps confirmed non-Git roots in a bounded transport-session cache across provider/tab remounts; root changes retain fresh detection, and legacy adapters without session identity remain provider-local | Advances `REQ-REMOTE-QUERY-CACHE-01` and non-Git SCM polling/noise reduction; public/physical Remote, WebView2 soak, and full Kernel authority gates remain open | `pnpm exec vitest run src/remote/lib/sidebarProvider.test.ts`: 15 passed |
| v0.3.48 | 2026-08-03 | `<ITERATION-144-KERNEL-AGENT-LAUNCH>` | Structured Agent PTYs now prefer authenticated `ridge-kernel` argv/env creation with stable Pane identity, teammate/TMUX metadata, bounded launch payloads, and local pending-spawn fallback on Kernel unavailability | Advances `REQ-RIDGE-KERNEL-HOST-01`, `REQ-RIDGE-KERNEL-DOMAIN-01`, and PTY lifecycle/memory safety; physical restart, public Remote, WebView2 soak, and dual-window evidence remain external gates | `cargo test -p ridge-kernel --lib`: 33 passed; `cargo test -p ridge --lib`: 252 passed; full Vitest 1529 passed / 1 skipped; `pnpm check` 0/0 |
| v0.3.47 | 2026-08-03 | `<ITERATION-143-PTY-CLOUD-LIFECYCLE>` | PTY bridge attach single-flight, Pane destroy cancellation, Cloud raw-pane listener-before-subscribe, serialized unsubscribe, and source-failure containment | Advances `REQ-REMOTE-RUNTIME-PERF-MEMORY-02` and `REQ-RDG-REMOTE-CONNECT-01`; physical phone, public, WebView2 soak, and full Kernel authority remain external gates | focused Vitest 80 passed; full Vitest 1529 passed / 1 skipped; `pnpm check` 0/0; Rust rdg 129 and kernel 31 passed |
| v0.3.9 | 2026-08-02 | `<ITERATION-85>` | Mobile Remote Worker 冷启动监听、WASM fallback、Pane init/bind/resize 生命周期与 runtime noise 断言纳入实现闭环 | 收敛 `REQ-MOBILE-REMOTE-KEYBOARD-QOS-02`、`REQ-REMOTE-RUNTIME-PERF-MEMORY-02`、`REQ-MOBILE-REMOTE-RUNTIME-LASTERROR-01` 的确定性部分；外部闸门仍待补 | 用户预审批通过；Iteration 85 contract 与 CDP/Vitest 证据 |
| v0.3.10 | 2026-08-02 | `INTAKE-20260802-REMOTE-PWA-GIT-AGENT-01` | 下一迭代纳入 Remote browser/PWA safe-area 与真实安装能力、Git/File Query 缓存、Git commit/push + GitGraph、Agent Commune 移动端编组/历史 Tab 与桌面 parity | 新增 `REQ-MOBILE-REMOTE-PWA-SAFE-AREA-01`、`REQ-MOBILE-REMOTE-PWA-INSTALL-01`、`REQ-REMOTE-QUERY-CACHE-01`、`REQ-GIT-INTERACTIVE-PUBLISH-GRAPH-01`、`REQ-AGENT-COMMUNE-REMOTE-PARITY-01` | 用户明确下一迭代预审批，并补充 Remote Agent 编组/历史 Tab 与 PWA 安装根因闭环 |
| v0.3.11 | 2026-08-02 | `<DIRECT-REVISION>` | PWA 安装改由浏览器原生 UI；Remote 业务不显示安装按钮、不监听/消费 `beforeinstallprompt`，仅负责 standalone/PWA safe-area 与布局适配；补充手机 Agent 卡片真实 CWD 展示 | 修订 `REQ-MOBILE-REMOTE-PWA-INSTALL-01`；补充 `REQ-AGENT-COMMUNE-REMOTE-PARITY-01` 的卡片 CWD 观察项 | 用户明确“Remote端不要显示添加到主屏幕这个按钮”及“Agent卡片也需要展示Cwd” |
| v0.3.12 | 2026-08-02 | `<DIRECT-REVISION>` | Pane Border 改为仅待用户介入的暂态提示；正常运行/空闲不描边，聚焦、接管、输入或 Resize 后清除；Agent 状态色条/文本仍常驻 | 修订 `REQ-AGENT-INTERACTION-STATE-01`、`REQ-AGENT-COMMUNE-UI-02`、`REQ-AGENT-COMMUNE-REMOTE-PARITY-01` 的 Pane 反馈语义 | 用户明确“只有需要用户介入时，才高亮 Pane 的 Border” |
| v0.3.13 | 2026-08-02 | `<ITERATION-87-CONTINUATION>` | Scrollback/link-span/Worker 失败路径回收与 LAN 探针并发隔离纳入确定性闭环；LAN 桌面/手机真实浏览器冒烟覆盖 `write_to_pty`、`resize_pane`；公网四格仍因凭据缺失保持外部闸门 | 收敛 `REQ-REMOTE-RUNTIME-PERF-MEMORY-02`、`REQ-RDG-REMOTE-CONNECT-01` 的本地可证部分；不关闭 WebView2 heap、实体设备、公网 WebRTC、真实 Git push | 135/1433 Vitest、`pnpm check`、Rust 48/17/8、LAN E2E 与 Remote workflow `30738272039` |
| v0.3.14 | 2026-08-02 | `<ITERATION-87-RUNTIME-ISOLATION>` | LAN 桌面/手机浏览器冒烟显式禁用扩展及组件扩展后台页，并将隔离模式写入证据；受控无扩展路径 `browserErrors=[]` | 收敛 `REQ-MOBILE-REMOTE-RUNTIME-LASTERROR-01` 的仓库路径排除与受控对照部分；不替代受影响实体手机来源 URL 与逐扩展 A/B | `pnpm exec vitest run scripts/rdg-remote-e2e.test.ts` 5 passed；`pnpm e2e:rdg-lan` exit 0 |
| v0.3.15 | 2026-08-02 | `<ITERATION-87-GIT-REAL-REPO>` | Git 交互写路径新增真实临时仓库守卫：commit、bare remote push 成功及 non-fast-forward push 失败均走共享 Rust handler | 收敛 `REQ-GIT-INTERACTIVE-PUBLISH-GRAPH-01` 本地真实仓库部分；认证 Remote push、公网拒绝/冲突链路仍待外部 | `cargo test -p ridge-core commit_and_push_real_temp_repo --lib --quiet` 1 passed |
| v0.3.16 | 2026-08-02 | `<ITERATION-87-EVIDENCE-REDACTION>` | LAN Remote 冒烟证据统一脱敏临时 TOTP/auth material；控制台仅显示 `<redacted>`，artifact 不再保存六位码 | 收敛 `REQ-RDG-REMOTE-CONNECT-01` 与 `REQ-MOBILE-REMOTE-RUNTIME-LASTERROR-01` 的受控证据安全性；不替代实体手机归因 | `pnpm exec vitest run scripts/rdg-remote-e2e.test.ts` 6 passed；`pnpm e2e:rdg-lan` exit 0；artifact scan 无 TOTP 数字 |
| v0.3.17 | 2026-08-02 | `<ITERATION-87-RUNTIME-SOURCE-GUARD>` | 新增 Remote 源码守卫，证明入口与 Service Worker 不携带 Chrome Extension Messaging API，消息仅走单向 Service Worker `Client.postMessage` | 收敛 `REQ-MOBILE-REMOTE-RUNTIME-LASTERROR-01` 的仓库静态证据；受影响实体手机来源 URL 与逐扩展 A/B 仍待外部 | `pnpm exec vitest run src/remote/runtimeMessagingScope.test.ts src/remote/pwaInstallScope.test.ts` 4 passed |
| v0.3.18 | 2026-08-02 | `<ITERATION-87-HOST-CONNECT-CONTRACT>` | Remote Host 接入流程新增确定性守卫：弹窗先关闭、Hosts 面板承接 Loading/错误进度，按钮与拖拽共用 attach 管线，接入后首个真实 DOM 尺寸同步 | 收敛 `REQ-RDG-REMOTE-CONNECT-01` 的本地接入 UX/Resize/拖拽部分；公网/实体设备延迟与真实工作区列表仍待外部 | `pnpm exec vitest run src/lib/hosts/hostConnectFlow.test.ts src/lib/actions/hostSessionDrag.test.ts`；全量 137 files / 1439 passed |
| v0.3.19 | 2026-08-02 | `<ITERATION-87-PANE-ATTENTION-ACTIVE-ACK>` | Desktop active-pane effect now clears transient Agent attention, covering keyboard/workspace restore/Agent-card takeover in addition to pointer focus; no outer ring for normal working/idle | 收敛 `REQ-AGENT-INTERACTION-STATE-01`、`REQ-AGENT-COMMUNE-UI-02` 的 active acknowledgement semantics | `pnpm exec vitest run src/lib/components/SplitContainer.test.ts`；`pnpm check` 0/0 |
| v0.3.20 | 2026-08-02 | `<ITERATION-87-KERNEL-READ-SEAM>` | Kernel client 新增 typed domain read adapter，严格校验 `ok=true` 与 `source=ridge-kernel`，并覆盖 workspace/Agent roster 错误响应；不替换当前 AppState 读路径 | 收敛 `REQ-RIDGE-KERNEL-DOMAIN-01` 的安全迁移 seam；workspace 名称/窗口 claims 与 Agent 复合身份未统一前不宣称纯壳 | `cargo test -p ridge-kernel --lib` 19 passed；`cargo check --manifest-path src-tauri/Cargo.toml` exit 0 |
| v0.3.21 | 2026-08-02 | `<ITERATION-87-GIT-CHILD-TEST-RACE>` | Git 真子进程生命周期测试统一串行锁，避免全局 active-child 计数被并行 SCM 测试污染；三次并行全量 ridge-core 通过 | 收敛 `REQ-20260730-01` / `REQ-REMOTE-RUNTIME-PERF-MEMORY-02` 进程回收测试确定性 | `cargo test -p ridge-core --lib --quiet` ×3：309 passed |
| v0.3.22 | 2026-08-02 | `<ITERATION-87-RELEASE-ROLLBACK>` | `v0.1.37` CI 闸门因 Linux bare Git 默认允许 non-fast-forward 而失败；按规则删除 tag、回退版本；测试显式配置 `receive.denyNonFastForwards=true` 后 targeted + 三次全量 ridge-core 通过 | 收敛 `REQ-GIT-INTERACTIVE-PUBLISH-GRAPH-01` 的跨平台真实仓库守卫；不声称失败 Release 或版本提升 | `gh run view 30741069265 --log-failed`；`cargo test ... commit_and_push_real_temp_repo` 1 passed；全量 309 ×3 |
| v0.3.23 | 2026-08-02 | `<ITERATION-87-KERNEL-CONVERGENCE-DIAGNOSTIC>` | Kernel client 新增只读 `DomainConvergenceReport`：精确比较 workspace/Agent identity set，显式记录 stable-key mismatch；空、重复、畸形身份 fail closed；不按列表顺序猜测、不切换桌面 AppState 权威源 | 收敛 `REQ-RIDGE-KERNEL-DOMAIN-01` 的可观测迁移前置条件；未统一复合身份与持久化前不宣称纯壳完成 | `cargo test -p ridge-kernel --lib` 21 passed；`build_domain_convergence_report`/`read_domain_convergence` |
| v0.3.24 | 2026-08-02 | `<ITERATION-87-RELEASE-RETRY-ROLLBACK>` | 第二次 `v0.1.37` CI `30741669936` 仍在 Linux 真实 Git push 守卫失败；按规则立即删除 tag/回退版号；测试 fixture 改用临时 `pre-receive` 拒绝并验证 remote head 不变 | 收敛 `REQ-GIT-INTERACTIVE-PUBLISH-GRAPH-01` 的跨平台确定性失败证据；正式 Release 仍保持 `v0.1.36` 直至新闸门全绿 | `gh run view 30741669936 --log-failed`；`cargo test ... commit_and_push_real_temp_repo` 1 passed；release filter 36 passed |
| v0.3.25 | 2026-08-02 | `<ITERATION-87-RELEASE-REF-FIX-ROLLBACK>` | 第三次 `v0.1.37` CI `30742032341` 捕获 Linux clone 未将竞争 push 指向 `refs/heads/main`；按规则删除 tag/回退版号；fixture 改用显式 `HEAD:refs/heads/main`，最终 stale push 仍经共享 handler | 完成真实 Git fixture 的跨平台 ref 绑定；正式 Release 仍保持 `v0.1.36` 直至全矩阵通过 | `gh run view 30742032341 --log-failed`；`cargo test ... commit_and_push_real_temp_repo` 1 passed；`8fa19b6` |
| v0.3.26 | 2026-08-02 | `<ITERATION-87-RELEASE-CLONE-FIX-ROLLBACK>` | 第四次 `v0.1.37` CI `30742240439` 捕获 clone 未从真实 `main` 分支起步；按规则删除 tag/回退版号；fixture 改为 `git clone --branch main` + 显式 refspec | 完成真实 Git 临时仓库基线初始化；正式 Release 仍保持 `v0.1.36` 直至全矩阵通过 | `gh run view 30742240439 --log-failed`；`cargo test ... commit_and_push_real_temp_repo` 1 passed；`cbada57` |
| v0.3.8 | 2026-08-02 | `<DIRECT-APPROVAL>` | Mobile Remote 键盘稳定偏移与 Remote 性能/健壮性/内存回收转 Active | 新增 `REQ-MOBILE-REMOTE-KEYBOARD-QOS-02`、`REQ-REMOTE-RUNTIME-PERF-MEMORY-02` | 用户明确「这条需求也预审批通过」 |
| v0.3.7 | 2026-08-02 | `PENDING-REQ-20260801-AGENT-COMMUNE-UI-01` | Agent's Commune 交互卡片、状态投影、历史按 Agent 分组与 CWD 恢复转 Active | 新增 `REQ-AGENT-COMMUNE-UI-02` | 用户明确「预审批刚刚需求」 |
| v0.3.6 | 2026-07-31 | `PENDING-REQ-RIDGE-KERNEL-HOST-01` | 内核进程与外壳生命周期（深根模式）转 Active | 新增 `REQ-RIDGE-KERNEL-HOST-01` | 用户明确「全数批准」 |
| v0.3.6 | 2026-07-31 | `PENDING-REQ-RIDGE-KERNEL-DOMAIN-01` | 领域能力 SSOT 在内核转 Active | 新增 `REQ-RIDGE-KERNEL-DOMAIN-01` | 同上 |
| v0.3.6 | 2026-07-31 | `PENDING-REQ-RIDGE-MCP-AS-KERNEL-API-01` | ridge-mcp 接内核面转 Active | 新增 `REQ-RIDGE-MCP-AS-KERNEL-API-01` | 同上 |
| v0.3.5 | 2026-07-31 | `PENDING-REQ-MCP-JOIN-GROUP-01` | ridge_join_group 参数/宿主校验转 Active | 新增 `REQ-MCP-JOIN-GROUP-01` | 用户明确「我批准所有项」 |
| v0.3.5 | 2026-07-31 | `PENDING-REQ-NLM-OPENPLAN-01` | NLM 开放规划优先项入 Active 追踪 | 新增 `REQ-NLM-OPENPLAN-01` | 同上 |
| v0.3.4 | 2026-07-30 | `<DIRECT-APPROVAL>` | Commune typed launch profile、跨工作区 Agent create/send、checkpoint 替换及 installer companion | 新增 `REQ-AGENT-COMMUNE-LAUNCH-PROFILE-01`、`REQ-RIDGE-MCP-INSTALLER-01` | 用户确认补充跨工作区范围并批准继续 |
| v0.3.3 | 2026-07-30 | `<DIRECT-APPROVAL>` | Mobile render Worker authority、切换连续性、IME/虚拟键盘与 pane-local 恢复 | 新增 `REQ-MOBILE-REMOTE-WORKER-AUTHORITY-01`、`REQ-MOBILE-REMOTE-INPUT-FEEDBACK-01` | 用户批准 |
| v0.3.1 | 2026-07-29 | `<DIRECT-FIX>` | Commune MCP send 默认真提交；提交 Enter 统一为 CR | 新增 `REQ-AGENT-COMMUNE-MCP-SUBMIT-03`；取代 iteration 65 的默认草稿语义 | 用户明确“批准”，要求载入本迭代并修复 |
| v0.3.0 | 2026-07-29 | 本轮 12 项 `PENDING-REQ-*` | 终端、Explorer、Remote、Agent 与对比度深研转 Active；无头项收紧为仅诊断，删除须另批 | 新增 12 项 `REQ-*`；既有 Active 差距继续有效 | 用户授权“审核无问题即全部通过”；Codex 审计通过 |
| v0.2.0 | 2026-07-27 | `<INITIAL>` | 建立 Remote/Agent 六项需求基线 | - | 用户本线程六项明确落地指令 |
| v0.2.0 | 2026-07-27 | `PENDING-REQ-REMOTE-HOST-TREE-01` | 公网/LAN 主机三层树转 Active | 新增 `REQ-REMOTE-HOST-TREE-01` | 用户明确“将之前 pending 的两个需求通过审批” |
| v0.2.0 | 2026-07-27 | `PENDING-REQ-WORKSPACE-SHARE-01` | 跨账号单工作区分享转 Active | 新增 `REQ-WORKSPACE-SHARE-01` | 同上 |
| v0.2.0 | 2026-07-27 | `<DIRECT-FIX>` | 已保存工作区重开、删除与滚动条纳入需求 | 新增 `REQ-WORKSPACE-SAVED-01` | 用户纠正“要进需求” |
| v0.2.0 | 2026-07-27 | `<DIRECT-FIX>` | LAN/public 浏览器 Remote 几何与命中一致 | 新增 `REQ-REMOTE-03` | 用户明确要求定位修复并并入下一迭代 |
| v0.2.1 | 2026-07-28 | `PENDING-REQ-MOBILE-REMOTE-STATE-01` | Mobile Remote Query/store、跨 workspace pane 保活、键盘 transform、scrollback 拼接/loading 与 pane 行纯 icon 转 Active | 新增 `REQ-MOBILE-REMOTE-STATE-01`；修订 `REQ-MOBILE-01` / `REQ-REMOTE-03` | 用户明确“批准 PENDING-REQ-MOBILE-REMOTE-STATE-01” |
| v0.2.2 | 2026-07-28 | `PENDING-REQ-MOBILE-ACTIVE-QOS-01` | 弱网 active pane 逻辑保留通道转 Active | 修订 `REQ-MOBILE-REMOTE-STATE-01` | 用户明确“批准” |
| v0.2.3 | 2026-07-28 | `<AUTO-APPROVED>` | Agent Center“最近回复”升级为按 Agent 分组、可折叠且可恢复的新“历史”页；扩展 CLI adapter | 新增 `REQ-AGENT-HISTORY-01` | 用户明确“添加任务并按 NLM 流程自动审批通过” |
| v0.2.4 | 2026-07-28 | `PENDING-REQ-AGENT-COMMUNE-CONTINUITY-01` | Commune 控制/文档区移底，保留成员/编组/历史，并统一 Agent Tab 与 pane header 状态 | 新增 `REQ-AGENT-COMMUNE-CONTINUITY-01`；关联 `REQ-AGENT-HISTORY-01` | 用户明确“批准所有” |
| v0.2.5 | 2026-07-28 | `PENDING-REQ-REMOTE-SMOOTH-STATE-02` | 光标锚定键盘、唤起回底、复合 pane 身份、后台保活与非阻断 scrollback 转 Active | 新增 `REQ-REMOTE-SMOOTH-STATE-02`；修订 `REQ-MOBILE-REMOTE-STATE-01` / `REQ-REMOTE-03` | 用户明确“批准所有” |
| v0.3.27 | 2026-08-02 | `<ITERATION-87-RELEASE-CLOSURE>` | `v0.1.37` release workflow `30742422090` 全闸门通过并以 12 项匹配资产正式发布；Remote/cloud workflow `30743623499` 构建、上传、索引与健康检查通过，激活 `0.1.37+ge4e0f91` | 完成本轮可发布闭环；保留物理设备、受影响手机 attribution、公网 WebRTC、WebView2 heap soak、鉴权 Git push 与 Kernel 深迁移外部闸门，不伪称全部关闭 | `gh release view v0.1.37`：draft=false/assets=12；`gh run view 30743623499` success；health HTTP 200 |
| v0.3.28 | 2026-08-02 | `<ITERATION-87-PERF-KERNEL-CONTINUATION>` | Added real in-page heap/resource and worker-pending sampling, fail-closed RSS/process guards, and a read-only desktop Kernel domain convergence report; unavailable counters stay null and no authority switch is hidden | Advances `REQ-REMOTE-RUNTIME-PERF-MEMORY-02` and `REQ-RIDGE-KERNEL-DOMAIN-01`; long-run WebView2/device/public evidence remains external | `pnpm check` 0/0; `cargo test -p ridge-kernel --lib` 21 passed; PowerShell AST + one-second real sampler smoke |
| v0.3.29 | 2026-08-02 | `<ITERATION-87-PERF-TIMEOUT-GUARD>` | WebDriver perf soak timeout now follows configured workload duration with a 24-hour cap, preserving long-run coverage while preventing detached-driver hangs | Extends `REQ-REMOTE-RUNTIME-PERF-MEMORY-02` testability; real sustained device run remains external | `pnpm check` 0/0; PowerShell AST parse |
| v0.3.30 | 2026-08-02 | `<ITERATION-87-REMOTE-REFRESH>` | Rebuilt and activated Remote artifacts from latest pushed main `08919b6`; retained formal desktop `v0.1.37` because no version bump is needed for the already verified package set | Closes latest-main Remote/cloud refresh; desktop release assets remain `v0.1.37` and external device/public gates remain open | `gh run view 30744331190` success; cloud health HTTP 200 |
| v0.3.31 | 2026-08-02 | `<ITERATION-88-RUNTIME-ATTRIBUTION-KERNEL-HOST>` | Added fail-closed Remote runtime.lastError attribution probe (clean profile + one-extension A/B, unsuppressed logs, installed extension layout discovery) and typed Kernel remote-host snapshot seam; real no-Tauri kernel host smoke passed | Advances `REQ-MOBILE-REMOTE-RUNTIME-LASTERROR-01` and `REQ-RIDGE-KERNEL-DOMAIN-01`; physical phone attribution and full domain migration remain external | `pnpm exec vitest run scripts/remote-runtime-last-error-attribution.test.ts`: 4 passed; `scripts/kernel-host-smoke.ps1`: ALL SMOKE PASSED |
| v0.3.32 | 2026-08-02 | `<ITERATION-88-RUNTIME-AB-SOURCE-GUARD>` | Runtime attribution run exercised five loaded local extensions and one fail-closed unverified candidate; single Google Translate A/B clean; Remote source guard now recursively scans shipped implementation files | Further closes repository-side `REQ-MOBILE-REMOTE-RUNTIME-LASTERROR-01`; affected physical phone attribution remains external | `scripts/remote-runtime-last-error-attribution.mjs` headed run; `pnpm exec vitest run src/remote/runtimeMessagingScope.test.ts scripts/remote-runtime-last-error-attribution.test.ts`: 6 passed |
| v0.3.33 | 2026-08-02 | `<ITERATION-88-PANE-BORDER-TRANSIENT-CLARIFICATION>` | Confirmed desktop/Remote Pane Border derives only from transient waiting/stopped intervention state; working/idle remain border-free; focus, Agent-card takeover, input and claim clear the ring | Clarifies `REQ-AGENT-INTERACTION-STATE-01`, `REQ-AGENT-COMMUNE-UI-02`, and Remote parity; no physical-device display claim is inferred | `pnpm exec vitest run src/lib/components/SplitContainer.test.ts src/remote/lib/TerminalCanvas.test.ts src/remote/lib/keyboardOffset.test.ts`: 16 passed |
| v0.3.34 | 2026-08-02 | `<ITERATION-88-REMOTE-CLOUD-REFRESH>` | Remote/cloud workflow rebuilt and activated current `main` SHA `8dfe261` as `0.1.37+g8dfe261`; desktop `v0.1.37` remained unchanged because only docs changed | Closes latest Remote artifact refresh without claiming a new desktop package; external physical/public gates remain open | `gh run view 30745144695`: success; publish log activation; cloud health HTTP 200 |
| v0.3.35 | 2026-08-02 | `<ITERATION-88-KERNEL-CLIENT-EXIT>` | Real detached `rdg` client exit followed by second-client attach now asserts same kernel PID and healthy control plane | Closes local parent-exit lifecycle regression; deep-root shell termination and public/physical gates remain external | `cargo test -p ridge-cli --test kernel_lifecycle_e2e -- --nocapture`: 2 passed |
| v0.3.36 | 2026-08-02 | `<ITERATION-88-KERNEL-SHELL-ADAPTER>` | `ef70b3c` removes duplicated rdg raw socket clients; Agent/FS/Git/MCP calls share authenticated `ridge_kernel::client::request_json`, with HTTP/JSON failures and reserved Windows query bytes handled fail-closed | Advances `REQ-RIDGE-KERNEL-DOMAIN-01` without claiming desktop AppState, PTY, window-claim, or filesystem-root authority migration | `cargo test -p ridge-cli --bin rdg kernel_ctl`: 2 passed; lifecycle E2E 2 passed; `scripts/kernel-host-smoke.ps1`: ALL SMOKE PASSED |
| v0.3.37 | 2026-08-02 | `<ITERATION-88-REMOTE-CLOUD-KERNEL-ADAPTER>` | Remote/cloud workflow activated `ef70b3c` as `0.1.37+gef70b3c` (233 files / 21.78 MiB); cloud health HTTP 200; desktop `v0.1.37` unchanged | Closes latest code artifact refresh; follow-up docs commit does not trigger version bump; physical/public/WebView2/authenticated-push/full-kernel-migration gates remain external | `gh run view 30746141772`: success; publish activation; health HTTP 200 |
| v0.3.38 | 2026-08-02 | `<ITERATION-88-PANE-HEADER-GIT-PILL-GUARD>` | `a1d816a` adds a deterministic one-layer guard: Repo switcher, Git branch pill, and Diff pill each render once as adjacent PaneHeader siblings; focused desktop/Remote/mobile slice is 17/17 | Advances PaneHeader visual contract; physical-device display remains external | `pnpm exec vitest run src/lib/components/SplitContainer.test.ts src/remote/lib/TerminalCanvas.test.ts src/remote/lib/keyboardOffset.test.ts`: 17 passed |
| v0.3.39 | 2026-08-02 | `<ITERATION-88-KERNEL-CLIENT-HTTP-GUARD>` | `40fa39f` adds a real loopback server guard for shared `request_json`: non-2xx and malformed JSON fail closed; ridge-kernel suite is 22/22 | Advances `REQ-RIDGE-KERNEL-DOMAIN-01` shell-adapter safety; no full desktop authority migration claim | `cargo test -p ridge-kernel --lib --quiet`: 22 passed |
| v0.3.40 | 2026-08-02 | `<ITERATION-88-NLM-REFRESH>` | Fixed-proxy NLM query refreshed the latest TUI-focused conversation; its Remote/PWA absence and TUI-only Pane Border assertions were rejected against local CodeGraph/tests, so no duplicate implementation was started | Records NLM as strategy-only and preserves local code/test authority; PTY physical-fidelity remains evidence-only | `nlm login --check`: valid / 20 notebooks; `notebook_query`: success; local Pane/Remote/kernel tests remain green |
| v0.3.41 | 2026-08-02 | `<ITERATION-88-REMOTE-LATEST-MAIN>` | Remote/cloud workflow `30746571058` activated literal latest `main` SHA `26241f2` as `0.1.37+g26241f2` (233 files / 21.78 MiB); cloud health HTTP 200; desktop `v0.1.37` unchanged | Closes latest-main artifact refresh without version bump; external phone/public/WebView2/authenticated-push/full-kernel-migration gates remain explicit | `gh run view 30746571058`: success; publish activation; health HTTP 200 |
| v0.3.42 | 2026-08-02 | `<ITERATION-88-FOREIGN-RESIZE-COALESCE>` | Desktop Foreign Host `OutboundClient::resize_pane` now serializes same-Pane resize admission, suppresses identical acknowledged dimensions, exposes suppression telemetry, clears snapshots across lifecycle boundaries, and fails closed after detach races | Advances high-priority RPC dedupe/queue control and Pane lifecycle safety; Remote scheduler remains the primary timeout/backoff path; external physical/public/heap/full-kernel gates stay open | Commit `de001bb`; `cargo test --manifest-path src-tauri/Cargo.toml hosts::outbound::tests -- --nocapture`: 10 passed |
| v0.3.43 | 2026-08-02 | `<ITERATION-88-REMOTE-PUBLISH-RESIZE-GUARD>` | Remote workflow `30747286348` rebuilt and atomically activated `0.1.37+g25d525b` from the Resize guard commit plus iteration archive; Cloud health HTTP 200; desktop `v0.1.37` formal release retained | Closes the Remote artifact propagation leg without falsely bumping the desktop version; physical/public/WebView2/authenticated-push/full-kernel gates remain external | `gh run view 30747286348`: success; activation log; `gh release view v0.1.37`: draft=false, prerelease=false, assets=12 |
| v0.3.44 | 2026-08-02 | `<ITERATION-88-VERSIONED-RELEASE-REMOTE-CLOSURE>` | `2346f5b` released `v0.1.38` only after the full test/build matrix passed; the formal release has 12 matching installer/CLI assets and is non-draft/non-prerelease. Remote workflow `30748970383` rebuilt the same commit and activated `0.1.38+g2346f5b` (233 files / 21.78 MiB), with desktop/mobile index checks and cloud health HTTP 200. | Closes the release/Remote/cloud publication leg; retains physical phone attribution, physical PWA geometry/keyboard/touch, public WebRTC, authenticated Git push, WebView2 long-run heap, and full deep-root Kernel authority as external gates. | `gh run view 30747757222`: success; `gh release view v0.1.38`: draft=false, prerelease=false, assets=12; `gh run view 30748970383`: success; activation log; health HTTP 200 |
| v0.3.45 | 2026-08-02 | `<ITERATION-88-CURRENT-MAIN-CODE-GUARDS>` | `81acf20` makes the Remote PWA build/verification deterministic; `3fc0073` recursively guards shipped Remote sources against Chrome Extension Messaging; `7199ead` adds no-Tauri `rdg kernel remote-hosts`; `08ffb50` carries explicit Git repository identity so clean repositories render correctly. | Advances PWA/runtime attribution, Kernel shell-adapter, and GitGraph/non-Git correctness; does not close physical phone, physical PWA, public WebRTC/authenticated push, WebView2 long-run, or full Kernel authority gates. | `pnpm build:remote:mobile`; `pnpm verify:pwa` 7/7; `pnpm check` 0/0; Remote Vitest 19 files/96 tests; `cargo test -p ridge-core commands::git --lib` 33 passed; `cargo test -p ridge-cli --bin rdg kernel_ctl --quiet` 3 passed; `kernel-host-smoke.ps1` green |
| v0.3.46 | 2026-08-02 | `<ITERATION-88-VERSIONED-RELEASE-039>` | Versioned `v0.1.39` built from `c772085` passed the test gate and all four platform jobs; 12 matching assets were verified before promotion to formal Release. Remote workflow rebuilt the same versioned main commit and activated `0.1.39+gc772085`; Cloud health returned HTTP 200. | Closes current code publication across desktop/Remote/cloud; physical phone/PWA, public WebRTC/authenticated Git push, WebView2 long-run heap, dual-window/Host device E2E, and full Kernel authority migration remain external. | `gh run view 30749879814`: success; `gh release view v0.1.39`: `draft=false`, `prerelease=false`, `assets=12`; `gh run view 30749958058`: success; activation log; health HTTP 200 |

## Wave40 build-ridge 构建入口可测化与覆盖率复核

- `scripts/build-ridge.mjs` 保持直接 CLI 行为不变，新增 ESM 入口保护并导出版本解析、Cargo/WiX 重写、Tauri 配置及子进程契约；测试不再因导入脚本而启动真实构建。
- `scripts/build-ridge.test.mjs` 覆盖 release 参数解析、语义版本校验、稳定 GUID、元数据定向重写、Tauri 参数契约、子进程成功/失败，以及前端双构建、临时文件清理、Cargo/WiX 成功/失败恢复；定向 `7/7` 通过。
- 全量 `pnpm test:coverage:sonar` exit `0`；本地 V8/LCOV statements `11988/18538 = 64.66%`、branches `6629/11546 = 57.41%`、functions `2310/3527 = 65.49%`、lines `10830/15832 = 68.40%`。较 Wave39 statements `+97`，距本地 80% 还需覆盖 `2843` 条 statements。
- `pnpm check` 为 `0 errors / 0 warnings`；`cargo fmt --all -- --check`、`node --check scripts/build-ridge.mjs`、`git diff --check` 通过。coverage 与 `.iteration` 运行态产物不纳入提交。
- V8 对部分既有 `.mjs` 仍报 `PARSE_ERROR/Expected ident` 并排除；本机 SonarQube `26.7.0.124771` 与 scanner 均已安装，监控 API 为 `UP`，但当前项目 coverage `56.7%`、Quality Gate `ERROR`，故不得将本地 LCOV 宣称为 Sonar 80% 或 Gate 通过，`REQ-SONAR-COVERAGE-80-01` 继续 `ACTIVE`。

## Wave39 覆盖率与通信边界回归

- `packages/remote/src/shared/terminal/manager.attach.test.ts` 新增隔离 wasm/DOM 生命周期测试：未 ready 拒绝、Canvas/scrollback/theme 注入、重复 attach fencing、focus in/out、TUI mouse press/release、double/triple click、ResizeObserver 与 detach 清理；聚焦 `2/2` 通过。既有 `manager.test.ts` 与 `controllerIdentity.test.ts` 合计 `25/25` 通过。
- `src/remote/lib/cloudRemote.test.ts` 新增 3 组边界测试：空 pane/零尺寸输入 fail-closed、Agent/shell/HITL/health/workspace parity 命令、空 workspace 创建与 close 失败可重试；全文件 `50/50` 通过。
- 全量 `pnpm test:coverage:sonar`：`188` 个测试文件，`1779 passed / 1 skipped`；本地 V8/LCOV statements `11891/18537 = 64.14%`、branches `6599/11542 = 57.17%`、functions `2298/3527 = 65.15%`、lines `10739/15831 = 67.83%`。相较 Wave38 covered statements 增加 `288`，距本地 80% 尚缺 `2939` 条；该 LCOV 仍不替代 Sonar project metric。
- `node --check scripts/*.mjs` 全部通过，但 V8 remap 仍对部分 `.mjs` 报 `PARSE_ERROR/Expected ident` 并排除；本机仍无 `SONAR_TOKEN`/`SONAR_HOST_URL`、scanner 与 Quality Gate 证据，故 `REQ-SONAR-COVERAGE-80-01` 保持 ACTIVE，不宣称 80%/Gate 完成。
- 质量证据：全量 coverage 命令退出 `0`；`cargo fmt --all -- --check`、`git diff --check` 应随提交前复核；`pnpm check` 本波未重跑（既有 Node/dev 进程竞争曾导致超时）。coverage 与 `.iteration` 运行态产物不纳入提交。

## Wave41 Remote 通信契约覆盖与 Sonar 差距复核

- `packages/remote/src/shared/terminal/manager.attach.test.ts` 新增开发诊断面真实调用：PTY feed/write、可见网格、主题/光标探针、selection/delta、PTY write spy、worker 状态及 detach；聚焦 `3/3` 通过。
- 新增 `packages/remote/src/shared/transport/wsRemote.behavior.test.ts`，覆盖 LAN 消息/二进制 PTY 路由、capability/theme/meta/resize、Agent typed envelope、HITL、history/resume、workspace/shell API、saved workspace、断开取消；与既有 scheduler/attach 回归合计 `27/27` 通过。
- 全量 `pnpm test:coverage:sonar` exit `0`；当前本地 V8/LCOV statements `12307/18538 = 66.38%`、branches `6772/11546 = 58.65%`、functions `2399/3527 = 68.01%`、lines `11118/15832 = 70.22%`。按 statements 目标需覆盖 `14831` 条，尚缺 `2524` 条；`.mjs` `PARSE_ERROR/Expected ident` 仍为既有诊断。
- 最终非 coverage 回归：`pnpm test` `195` files，`1810 passed / 1 skipped`；`node --check scripts/*.mjs` 全部通过。
- `pnpm check` 为 `0 errors / 0 warnings`；当前 Sonar monitor 仍为 coverage `56.7%`、Quality Gate `ERROR`，本波未取得新 scanner/CE 成功证据，故 `REQ-SONAR-COVERAGE-80-01` 继续 `ACTIVE`。
- 通信底座回归：`cargo test --target-dir target/codex-wave41-mcp -p ridge-mcp --features axum-transport --lib` 测试汇总 `93 passed / 0 failed`；`cargo test --target-dir target/codex-wave41-kernel -p ridge-kernel --lib` `49 passed / 0 failed`；`cargo fmt --all -- --check` 通过。
- 本波只证明 Remote 适配器消费统一通信契约的更多边界；Kernel PTY 五条件原子运行时快照、第三方 Runtime/A2A 真实兼容性、Sonar `>=80%`/Gate 仍未闭环。coverage 与 `.iteration` 运行态变更不纳入提交。

## Wave44 Agent/Cloud 通信边界与编组持久化覆盖

- `packages/remote/src/shared/cloud/ridgeCloudProvider.test.ts` 新增 ICE 失败、重复上线/信令断线、封禁 controller、握手/信令公钥拒绝、workspace scope 拒绝、bridge 公钥拒绝、offer 异常七类边界回归。
- `src/lib/teammate/teammateGroups.test.ts` 新增持久化字段防御解析、循环数据 fail-closed，以及真实 `TeammateGroupStore` 工作区切换、localStorage 回读、编组变更、成员事件桥与任务记录路径。
- 焦点回归 `45/45`；全量 `pnpm test:coverage:sonar` 为 `199` files、`1835 passed / 1 skipped`；statements `12539/18603 = 67.40%`，branches `6903/11608 = 59.46%`，functions `2448/3536 = 69.23%`，lines `11322/15891 = 71.24%`。
- 本地 statements 80% 仍缺 `2344` 条；部分 `.mjs` 仍有 `PARSE_ERROR/Expected ident`，Sonar project `>=80%`/Quality Gate 与 PTY 真实五条件原子注入、第三方 Runtime/A2A 兼容性仍 `ACTIVE`，不作闭环宣称。

## Wave45 Cloud Host store scope 与事件投影覆盖

- `src/lib/remote/cloud/cloudHostStore.test.ts` 新增浏览器凭据/启动失败、host 回调、普通与共享 workspace bridge、scope deny、pane 事件过滤与 unsubscribe 回归。
- 全量 `pnpm test:coverage:sonar`：`199` files、`1837 passed / 1 skipped`；statements `12591/18603 = 67.68%`，branches `6937/11608 = 59.76%`，functions `2462/3536 = 69.62%`，lines `11364/15891 = 71.51%`；距本地 statements 80% 缺 `2292` 条。
- `.mjs` `PARSE_ERROR/Expected ident`、Sonar project `>=80%`/Quality Gate、PTY 真实五条件原子注入与第三方 Runtime/A2A 兼容性仍 `ACTIVE`。

## Wave46 通信 topology、终端背压与 pane 投影覆盖

- `src/lib/remote/cloud/cloudHostTopologyLink.test.ts` 覆盖多 workspace/shell 查询、workspace mutation 失败闭环、断连资源释放、空 pane、Agent 注册/注销与 scoped shell activation；聚焦 `15/15`。
- `packages/remote/src/shared/terminal/manager.test.ts` 覆盖 inline-TUI 分片合并、非 inline 顺序、bounded deferred feed、输入 ownership、Mac Cmd、wheel 上限、空响应与未知 pane；聚焦 `20/20`。
- `src/lib/stores/paneTree.coverage.test.ts` 覆盖 Agent status、pane/CWD 投影、ratio anchor、junction registry、reattach gate 与 CWD listeners；聚焦 `10/10`。
- 全量 `pnpm test`：`199` files，`1844 passed / 1 skipped`。
- 全量 `pnpm test:coverage:sonar` 本地 V8/LCOV：statements `12651/18603 = 68.00%`、branches `6972/11608 = 60.06%`、functions `2475/3536 = 69.99%`、lines `11412/15891 = 71.81%`；距 statements 80% 尚缺 `2232` 条。
- `.mjs` coverage 仍报 `PARSE_ERROR/Expected ident`；Sonar project/Quality Gate 未以本地 LCOV 冒充闭环，`REQ-SONAR-COVERAGE-80-01` 与 PTY/第三方 Runtime-A2A 现场证据继续 `ACTIVE`。

## Wave47 Cloud controller provider 错误边界覆盖

- `packages/remote/src/shared/cloud/controllerCloudProvider.test.ts` 新增 ICE 获取失败、WebSocket 构造失败、畸形信令、ICE candidate 失败、可恢复 error、peer leave 与 answer 应答失败回归；聚焦 `26/26`。
- 全量 `pnpm test:coverage:sonar` exit `0`；本地 V8/LCOV statements `12664/18603 = 68.07%`、branches `6982/11608 = 60.14%`、functions `2475/3536 = 69.99%`、lines `11423/15891 = 71.88%`；距 statements 80% 尚缺 `2219` 条。
- `pnpm check` 复核为 `0 errors / 0 warnings`；本波仅修正测试 fixture 类型，不改生产运行语义。
- `.mjs` coverage 仍报 `PARSE_ERROR/Expected ident`；Sonar project/Quality Gate 未以本地 LCOV 冒充闭环，`REQ-SONAR-COVERAGE-80-01` 与 PTY/第三方 Runtime-A2A 现场证据继续 `ACTIVE`。

## Wave48 Cloud controller 启动接线与生命周期覆盖

- 新增 `src/lib/remote/cloud/cloudControllerBoot.integration.test.ts`，覆盖 bridge attach、全局 transport 安装、provider 回调组合、重复 boot 单例、token 定时刷新、前台唤醒、fixed-token isolated boot 与 disconnect 回收；聚焦 `9/9`。
- 全量 `pnpm test:coverage:sonar` exit `0`；本地 V8/LCOV statements `12716/18603 = 68.35%`、branches `7022/11608 = 60.49%`、functions `2484/3536 = 70.24%`、lines `11466/15891 = 72.15%`；距 statements 80% 尚缺 `2167` 条。
- `pnpm check` 复核为 `0 errors / 0 warnings`；本波仅增隔离测试，不改生产运行语义。
- `.mjs` coverage 仍报 `PARSE_ERROR/Expected ident`；Sonar project/Quality Gate 未以本地 LCOV 冒充闭环，`REQ-SONAR-COVERAGE-80-01` 与 PTY/第三方 Runtime-A2A 现场证据继续 `ACTIVE`。

## Wave49 Cloud Host bridge 重连清理围栏

- `CloudHostBridge.reset()` 显式退订并清空 DataChannel 背压监听，避免旧连接 `bufferedamountlow` 回调跨重连残留。
- `packages/remote/src/shared/cloud/cloudHostBridge.test.ts` 覆盖 host event 门控/退订、preauthorized connected 回执、背压控制替换与 reset 清理；聚焦 `60/60`。
- 全量 `pnpm test:coverage:sonar` exit `0`；本地 V8/LCOV statements `12723/18606 = 68.38%`、branches `7030/11608 = 60.56%`、functions `2486/3536 = 70.30%`、lines `11473/15894 = 72.18%`；距 statements 80% 尚缺 `2162` 条。
- `pnpm check` 复核为 `0 errors / 0 warnings`；本波包含一处重连生命周期清理修复。
- `.mjs` coverage 仍报 `PARSE_ERROR/Expected ident`；Sonar project/Quality Gate 未以本地 LCOV 冒充闭环，`REQ-SONAR-COVERAGE-80-01` 与 PTY/第三方 Runtime-A2A 现场证据继续 `ACTIVE`。

## Wave50 LAN RemoteConnection 前台与缓存边界覆盖

- `packages/remote/src/shared/transport/wsRemote.behavior.test.ts` 覆盖坏 JSON fail-closed、缺 workspace 输出丢弃、5000 行输出缓存上限、visibility 前台 liveness probe 与 disconnect listener 清理；聚焦及 scheduler 回归 `25/25`。
- 全量 `pnpm test:coverage:sonar` exit `0`；本地 V8/LCOV statements `12735/18606 = 68.44%`、branches `7041/11608 = 60.65%`、functions `2488/3536 = 70.36%`、lines `11480/15894 = 72.22%`；距 statements 80% 尚缺 `2150` 条。
- `pnpm check` 复核为 `0 errors / 0 warnings`；本波仅增确定性测试，不改生产运行语义。
- `.mjs` coverage 仍报 `PARSE_ERROR/Expected ident`；Sonar project/Quality Gate 未以本地 LCOV 冒充闭环，`REQ-SONAR-COVERAGE-80-01` 与 PTY/第三方 Runtime-A2A 现场证据继续 `ACTIVE`。

## Wave51 Cloud Host bridge pane 订阅与背压恢复边界

- `packages/remote/src/shared/cloud/cloudHostBridge.test.ts` 覆盖无效 pane 订阅、pane source 缺失退订句柄、超长 pane id 编码失败、背压后切换 active 的私有 resync 恢复；聚焦 `62/62`。
- 全量 `pnpm test:coverage:sonar` exit `0`；本地 V8/LCOV statements `12743/18606 = 68.48%`、branches `7044/11608 = 60.68%`、functions `2490/3536 = 70.41%`、lines `11486/15894 = 72.26%`；距 statements 80% 尚缺 `2142` 条。
- `pnpm check` 复核为 `0 errors / 0 warnings`；本波仅增确定性测试，不改生产运行语义。
- `.mjs` coverage 仍报 `PARSE_ERROR/Expected ident`；Sonar project/Quality Gate 未以本地 LCOV 冒充闭环，`REQ-SONAR-COVERAGE-80-01` 与 PTY/第三方 Runtime-A2A 现场证据继续 `ACTIVE`。

## Wave52 Hosts 快照与跨账号共享聚合边界

- 新增 `src/lib/stores/hosts.refresh.test.ts`，覆盖 native 枚举失败仍保留远端快照、跨账号 incoming share 聚合、active/pending 状态投影、outgoing share 过滤及 refresh generation 围栏。
- 修复 `src/lib/stores/hosts.ts`：同一设备聚合内只要存在 active share，host 状态即升级为 `connected`，避免首条 pending share 错误遮蔽可用连接。
- 聚焦 hosts 回归 `15/15`；全量 `pnpm test:coverage:sonar` exit `0`，本地 V8/LCOV statements `12773/18608 = 68.64%`、branches `7064/11610 = 60.84%`、functions `2492/3536 = 70.47%`、lines `11514/15895 = 72.43%`，距 statements 80% 尚缺 `2114` 条；`pnpm check` 为 `0 errors / 0 warnings`。
- Sonar project `>=80%`/Quality Gate、`.mjs` coverage `PARSE_ERROR/Expected ident`、PTY 五条件原子运行时证据及第三方 Runtime/A2A 兼容性仍 `ACTIVE`；本地 LCOV 不冒充 Sonar 项目指标。

## Wave53 Host 权威布局同步与 CWD 围栏覆盖

- `src/lib/stores/paneTree.coverage.test.ts` 新增 Host 权威 workspace id、active pane 重选、stale CWD prune、new pane CWD seed、跨 workspace 保留及 Host id 查询失败回退回归；聚焦 `11/11`。
- 全量 `pnpm test:coverage:sonar` exit `0`，`scripts/normalize-lcov.mjs` 返回 `ok=true`；本地 V8/LCOV statements `12774/18608 = 68.64%`、branches `7066/11610 = 60.86%`、functions `2492/3536 = 70.47%`、lines `11514/15895 = 72.43%`，距 statements 80% 尚缺 `2113` 条；`pnpm check` 为 `0 errors / 0 warnings`。
- Sonar project `>=80%`/Quality Gate、`.mjs` coverage `PARSE_ERROR/Expected ident`、PTY 五条件原子运行时证据及第三方 Runtime/A2A 兼容性仍 `ACTIVE`；本地 LCOV 不冒充 Sonar 项目指标。

## Wave54 Cloud E2E harness 分页、能力与资源回收覆盖

- 新增 `packages/remote/src/shared/cloud/__cloudE2eHarness.test.ts`，覆盖连接成功/失败、分页局部失败、HELLO 能力协商、exploit、pane 原始字节流、tamper 标记清理及 host/controller 资源回收；聚焦回归 `2/2`，Cloud 通信组合回归 `106/106`。
- 全量 `pnpm test:coverage:sonar` exit `0`，`scripts/normalize-lcov.mjs` 返回 `ok=true`；本地 V8/LCOV statements `12858/18608 = 69.09%`、branches `7090/11610 = 61.06%`、functions `2501/3536 = 70.72%`、lines `11594/15895 = 72.94%`，距 statements 80% 尚缺 `2029` 条；`pnpm check` 为 `0 errors / 0 warnings`。
- Sonar project `>=80%`/Quality Gate、`.mjs` coverage `PARSE_ERROR/Expected ident`、PTY 五条件原子运行时证据及第三方 Runtime/A2A 兼容性仍 `ACTIVE`；本地 LCOV 不冒充 Sonar 项目指标。

## Wave55 Cloud signaling 失败边界与诊断覆盖

- `packages/remote/src/shared/cloud/ridgeCloudProvider.test.ts` 新增设备签名失败回落裸握手、malformed/unsupported/workspace-scope 信令丢弃、失败 peer 安全拆除、ICE 目标缺失及 blacklist/kick 回归；聚焦 `16/16`。
- 全量 `pnpm test:coverage:sonar` exit `0`，`scripts/normalize-lcov.mjs` 返回 `ok=true`；本地 V8/LCOV statements `12877/18608 = 69.20%`、branches `7106/11610 = 61.20%`、functions `2502/3536 = 70.75%`、lines `11610/15895 = 73.04%`，距 statements 80% 尚缺 `2010` 条；`pnpm check` 为 `0 errors / 0 warnings`。
- Sonar project `>=80%`/Quality Gate、`.mjs` coverage `PARSE_ERROR/Expected ident`、PTY 五条件原子运行时证据、第三方 Runtime/A2A 兼容性及其余 Cloud/Postgres/物理 DPR/跨卷权限/移动端现场证据仍 `ACTIVE`；本地 LCOV 不冒充 Sonar 项目指标。

## Wave56 Cloud API/认证 cookie 边界与覆盖补强

- `packages/remote/src/shared/cloud/apiClient.test.ts` 覆盖公开 API 路由的 JSON envelope、Bearer/SSO cookie、路径编码、401 单次刷新、未知错误码、网络错误与坏 JSON；`packages/remote/src/shared/cloud/auth.test.ts` 覆盖浏览器授权 wake/approved/expired、设备绑定 username 补齐及畸形持久化数据；聚焦合计 `16/16`。
- 全量 `pnpm test:coverage:sonar` exit `0`，`scripts/normalize-lcov.mjs` 返回 `ok=true`；本地 V8/LCOV statements `12947/18608 = 69.57%`、branches `7119/11610 = 61.31%`、functions `2533/3536 = 71.63%`、lines `11675/15895 = 73.45%`，距 statements 80% 尚缺 `1940` 条；`pnpm check` 为 `0 errors / 0 warnings`。
- Sonar project `>=80%`/Quality Gate、`.mjs` coverage `PARSE_ERROR/Expected ident`、PTY 五条件原子运行时证据、第三方 Runtime/A2A 兼容性及其余 Cloud/Postgres/物理 DPR/跨卷权限/移动端现场证据仍 `ACTIVE`；本地 LCOV 不冒充 Sonar 项目指标。

## Wave57 Cloud Tauri 代理与设备码终止边界

- 扩展 `packages/remote/src/shared/cloud/apiClient.test.ts` 覆盖 Tauri `cloud_http` 代理成功、代理网络失败、代理坏 JSON；扩展 `packages/remote/src/shared/cloud/auth.test.ts` 覆盖 device-code `expired` 与已 abort 取消；聚焦 `18/18`。
- 全量 `pnpm test:coverage:sonar` exit `0`，`scripts/normalize-lcov.mjs` 返回 `ok=true`；本地 V8/LCOV statements `12961/18608 = 69.65%`、branches `7127/11610 = 61.38%`、functions `2533/3536 = 71.63%`、lines `11687/15895 = 73.52%`，距 statements 80% 尚缺 `1926` 条；`pnpm check` 为 `0 errors / 0 warnings`。
- Sonar project `>=80%`/Quality Gate、`.mjs` coverage `PARSE_ERROR/Expected ident`、PTY 五条件原子运行时证据、第三方 Runtime/A2A 兼容性及其余 Cloud/Postgres/物理 DPR/跨卷权限/移动端现场证据仍 `ACTIVE`；本地 LCOV 不冒充 Sonar 项目指标。

## Wave58 Controller Cloud 重连与绑定终止边界

- `packages/remote/src/shared/cloud/controllerCloudProvider.test.ts` 新增信令 error/close、offer 创建失败、pane lane 关闭回落 control、ArrayBufferView 入站及绑定宽限期 relay-trust 回落；聚焦 `28/28`。
- 全量 `pnpm test:coverage:sonar` exit `0`，`scripts/normalize-lcov.mjs` 返回 `ok=true`；本地 V8/LCOV statements `12981/18608 = 69.76%`、branches `7140/11610 = 61.49%`、functions `2537/3536 = 71.74%`、lines `11705/15895 = 73.63%`，距 statements 80% 尚缺 `1907` 条；`pnpm check` 为 `0 errors / 0 warnings`。
- Sonar project `>=80%`/Quality Gate、`.mjs` coverage `PARSE_ERROR/Expected ident`、PTY 五条件原子运行时证据、第三方 Runtime/A2A 兼容性及其余 Cloud/Postgres/物理 DPR/跨卷权限/移动端现场证据仍 `ACTIVE`；本地 LCOV 不冒充 Sonar 项目指标。

## Wave59 Cloud HostBridge admission 与信任门控边界

- `packages/remote/src/shared/cloud/cloudHostBridge.test.ts` 新增非对象/未知通道 fail-closed、带 id `$/hello`/`$/cancel`、协议/白名单拒绝、notification 失败、TOTP trust malformed/lockout 与信任记录持久化失败回归；聚焦 `64/64`。
- 全量 `pnpm test:coverage:sonar` exit `0`，`scripts/normalize-lcov.mjs` 返回 `ok=true`；本地 V8/LCOV statements `13011/18608 = 69.92%`、branches `7154/11610 = 61.61%`、functions `2539/3536 = 71.80%`、lines `11735/15895 = 73.82%`，距 statements 80% 尚缺 `1876` 条；`pnpm check` 为 `0 errors / 0 warnings`。
- Sonar project `>=80%`/Quality Gate、`.mjs` coverage `PARSE_ERROR/Expected ident`、PTY 五条件原子运行时证据、第三方 Runtime/A2A 兼容性及其余 Cloud/Postgres/物理 DPR/跨卷权限/移动端现场证据仍 `ACTIVE`；本地 LCOV 不冒充 Sonar 项目指标。

## Wave60 Markdown 渲染、异步高亮与 Mermaid 边界

- `src/lib/utils/markdown.test.ts` 新增 Windows 链接、URL/代码反斜杠、task-list、Mermaid 占位符、Monaco highlight 成功/失败、Mermaid render 成功/失败及空容器边界；聚焦 `34/34`。
- 全量 `pnpm test:coverage:sonar` exit `0`，`scripts/normalize-lcov.mjs` 返回 `ok=true`；本地 V8/LCOV statements `13072/18608 = 70.24%`、branches `7178/11610 = 61.82%`、functions `2548/3536 = 72.05%`、lines `11792/15895 = 74.18%`，距 statements 80% 尚缺 `1815` 条；`pnpm check` 为 `0 errors / 0 warnings`。

## Wave61 paneTree 分屏状态机与保存回退覆盖

- `src/lib/stores/paneTree.coverage.test.ts` 新增分屏拖拽 `pending -> drag -> idle`、重复 splitter 引用去重、ratio 更新、release 返回值与空闲释放回归；新增 Tauri 不可用、成功及可选 startup/persistence 命令失败回退回归。
- 聚焦 `paneTree.coverage.test.ts` 为 `13/13`；未改生产语义，未向 Codex 之外 CLI、Agent 或 teammate 发消息。
- 全量 `pnpm test:coverage:sonar` exit `0`，`scripts/normalize-lcov.mjs` 返回 `ok=true`；本地 V8/LCOV statements `13090/18608 = 70.34%`、branches `7192/11610 = 61.94%`、functions `2550/3536 = 72.11%`、lines `11799/15895 = 74.23%`，距 statements 80% 尚缺 `1797` 条；`pnpm check` 为 `0 errors / 0 warnings`。
- Sonar project 实际 coverage `>=80%`、Quality Gate `OK`、PTY 五条件原子运行时、第三方 Runtime/A2A、Cloud/Postgres 真 E2E、物理 DPR、跨卷权限及移动 profile 仍需外部/现场证据；本地 LCOV 不冒充 Sonar 指标。

## Wave62 TerminalManager 渲染顺序与尺寸调度覆盖

- `packages/remote/src/shared/terminal/manager.test.ts` 新增多 pane RAF 渲染顺序回归：focused pane 优先、非 focused pane 轮转、公平性及 parked pane 过滤；新增 viewport resize trailing-fit 的重复 resize 合并、settle 后单次 fit、parked/missing pane fail-safe 回归。
- 聚焦 `manager.test.ts` 与 `manager.attach.test.ts` 为 `22/22`；未改生产语义，未向 Codex 之外 CLI、Agent 或 teammate 发消息。
- 全量 `pnpm test:coverage:sonar` exit `0`，`scripts/normalize-lcov.mjs` 返回 `ok=true`；本地 V8/LCOV statements `13185/18608 = 70.85%`、branches `7250/11610 = 62.44%`、functions `2558/3536 = 72.34%`、lines `11887/15895 = 74.78%`，距 statements 80% 尚缺 `1702` 条；`pnpm check` 为 `0 errors / 0 warnings`。
- Sonar project 实际 coverage `>=80%`、Quality Gate `OK`、PTY 五条件原子运行时、第三方 Runtime/A2A、Cloud/Postgres 真 E2E、物理 DPR、跨卷权限及移动 profile 仍需外部/现场证据；本地 LCOV 不冒充 Sonar 指标。

## Wave63 CloudRemote 降级与滚动历史边界

- `src/remote/lib/cloudRemote.test.ts` 新增可选 Cloud parity 命令失败回归：workspace 查询、Agent message/session、保存/打开/关闭 workspace、theme catalog、layout 查询的 fail-safe 或错误保留语义；新增 resync-frame → bounded tail 回退、实时 PTY 顺序、scrollback cursor commit/at-oldest 及无 cursor 短路回归。
- 聚焦 `cloudRemote.test.ts` 为 `52/52`；未改生产语义，未向 Codex 之外 CLI、Agent 或 teammate 发消息。
- 全量 `pnpm test:coverage:sonar` exit `0`，`scripts/normalize-lcov.mjs` 返回 `ok=true`；本地 V8/LCOV statements `13203/18608 = 70.95%`、branches `7258/11610 = 62.51%`、functions `2568/3536 = 72.62%`、lines `11902/15895 = 74.87%`，距 statements 80% 尚缺 `1684` 条；`pnpm check` 为 `0 errors / 0 warnings`。
- Sonar project 实际 coverage `>=80%`、Quality Gate `OK`、PTY 五条件原子运行时、第三方 Runtime/A2A、Cloud/Postgres 真 E2E、物理 DPR、跨卷权限及移动 profile 仍需外部/现场证据；本地 LCOV 不冒充 Sonar 指标。
- Sonar project `>=80%`/Quality Gate、`.mjs` coverage `PARSE_ERROR/Expected ident`、PTY 五条件原子运行时证据、第三方 Runtime/A2A 兼容性及其余 Cloud/Postgres/物理 DPR/跨卷权限/移动端现场证据仍 `ACTIVE`；本地 LCOV 不冒充 Sonar 项目指标。

## Wave64 wsRemote Agent Hub 与滚动提交失败边界

- `packages/remote/src/shared/transport/wsRemote.behavior.test.ts` 新增 Agent Hub receipt 结构损坏、typed error（含 stale lease）传播、topology 错误保留，以及 scrollback 空页/`atOldest`/序列错配 fail-closed、成功页后的 stale discard 回归。
- 聚焦 `wsRemote.behavior.test.ts` 为 `6/6`；未改生产语义，未向 Codex 之外 CLI、Agent 或 teammate 发消息。
- 全量 `pnpm test:coverage:sonar` exit `0`，`scripts/normalize-lcov.mjs` 返回 `ok=true`；本地 V8/LCOV statements `13210/18608 = 70.99%`、branches `7268/11610 = 62.60%`、functions `2568/3536 = 72.62%`、lines `11907/15895 = 74.91%`，距 statements 80% 尚缺 `1677` 条；`pnpm check` 为 `0 errors / 0 warnings`。
- Sonar project 实际 coverage `>=80%`、Quality Gate `OK`、PTY 五条件原子运行时、第三方 Runtime/A2A、Cloud/Postgres 真 E2E、物理 DPR、跨卷权限及移动 profile 仍需外部/现场证据；本地 LCOV 不冒充 Sonar 指标。
- `REQ-SONAR-COVERAGE-80-01`、PTY 五条件原子运行时证据、第三方 Runtime/A2A 兼容性及其余现场证据继续 `ACTIVE`；`.mjs` coverage `PARSE_ERROR/Expected ident` 继续记录，不以排除文件冒充达标。

## Wave42 Host transport onboarding coverage

- 新增 `src/lib/stores/hosts.connect.test.ts`，以隔离 fake transport 覆盖 LAN 成功接入、LAN 错误与进度保留、Cloud E2EE 接入、统一 topology 投影及清理；聚焦 `3/3` 通过。
- `pnpm check` 为 `0 errors / 0 warnings`；全量 `pnpm test:coverage:sonar` exit `0`，本地 V8/LCOV statements `12337/18538 = 66.54%`、branches `6790/11546 = 58.80%`、functions `2402/3527 = 68.10%`、lines `11147/15832 = 70.40%`。距本地 statements `80%` 尚缺 `2494` 条；Sonar 项目指标与 Quality Gate 未因本地运行改变。
- 既有 `.mjs` remap `PARSE_ERROR/Expected ident` 保留为待修质量债；不改 coverage include/exclude，不宣称 Sonar `>=80%` 或 Gate 完成。PTY 五条件原子运行时快照仍 fail-closed。

## Wave43 原子 PTY runtime snapshot 与脚本契约覆盖

- `packages/ridge-mcp/src/delivery.rs` 将 PTY fallback 注册模型收敛为 `HubPtyRuntimeSnapshot`：五项安全条件须随同一快照提交，并要求非零 `state_revision/input_epoch`；generation/lease/3 秒新鲜度/旧代注销围栏继续 fail-closed。`McpSessionState` 与 Kernel destroy teardown 已迁移至新 API。
- `scripts/build-ridge-mcp-sidecar.mjs`、`scripts/build-rdg-sidecar.mjs`、`scripts/check-release-version.mjs` 增加 ESM main guard 与可测导出；新增 3 个测试文件，聚焦回归 `12/12` 通过。未改变直接 CLI 构建/版本校验行为。
- Rust 定向回归：`ridge-mcp 93/93`、`ridge-kernel 49/49`；全量 `pnpm test:coverage:sonar`：`199` files，`1825 passed / 1 skipped`；statements `12440/18603 = 66.87%`、branches `6869/11608 = 59.17%`、functions `2420/3536 = 68.43%`、lines `11239/15891 = 70.72%`。较 Wave42 新增 covered statements `103`，距本地 80% 尚缺 `2443`。
- `.mjs` remap `PARSE_ERROR`、Sonar project `>=80%`/Quality Gate、真实宿主五条件快照注入与第三方 Runtime/A2A 兼容性仍 ACTIVE；本波不宣称闭环。coverage/`.iteration` 运行产物不纳入提交。
