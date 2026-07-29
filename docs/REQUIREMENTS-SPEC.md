# Ridge · 需求规范（REQUIREMENTS-SPEC）

> 本地只保留此一份需求文档。Pending 未获用户明确批准前，不改代码、不生成执行合同、不上传；
> NotebookLM 继续使用上一版已批准的 `REQUIREMENTS-SPEC` 来源。

## 待审批变更 (Pending Changes)

## 正式需求 (Active Requirements)

### REQ-TERMINAL-PASTE-ORDER-02

- 状态：`ACTIVE`
- 日期：`2026-07-29`
- 版本：`v0.3.0`
- 批准证据：用户授权“审核无问题即全部通过”；审计通过。
- 类型：`FIX`
- 关联：`CONTRACT-iteration-65.md` 目标 7。
- 原始意图：ridge 粘贴多行时，行序不得颠倒、交错或错乱。
- 当前证据：`RidgePane.pasteIntoPane` 将文本编码为单一 bracketed-paste payload，并以
  `(workspaceId,paneId)` FIFO 队列投递；该代码已随 `v0.1.9` 发布，但现场仍失败。
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

- 状态：`ACTIVE`
- 日期：`2026-07-29`
- 版本：`v0.3.0`
- 批准证据：用户授权“审核无问题即全部通过”；审计通过。
- 类型：`MODIFY` + `FIX`
- 关联：`REQ-AGENT-01`、`REQ-AGENT-COMMUNE-CONTINUITY-01`。
- 原始意图：
  - 首次进入 Agent's Commune 不得窄折叠；无需切 tab 自愈。
  - 点击活跃 Agent，直接切到其工作区、聚焦并高亮对应 pane。
  - Agent 停止为红，等待审批为黄；Agent 卡片边框与 pane header Agent 图标同步显色，
    直至目标 pane 真正聚焦才清除暂态高亮。
  - 交互卡片实时接收动态标题。
- 当前证据：AgentMemberRow 已有等待审批黄点、运行绿点、OSC title 优先；失联用 muted，
  行本身无跨工作区导航，亦无“直到聚焦才消失”的统一暂态。
- 目标行为：状态与标题由 topology/pane tree 同一事实投影；导航使用显式 workspace/pane；
  红/黄暂态在 Agent 卡片与 pane header 一致，聚焦目标 pane 后原子确认并清除。
- 范围：桌面 Agent Center、AgentMemberRow、pane header、跨工作区聚焦与 topology 标题投影。
- 非目标：改变 Agent 进程生命周期；以颜色替代状态文本或安全审批。
- 边界：颜色不得成为唯一语义；须有文本/aria-label；轮询不变不得制造布局或标题事件风暴。
- 假设/待确认：“停止运行”映射 `Disappeared`/进程退出，不等同用户主动暂停。
- 验收：首次挂载真实宽度 fixture；wsA→点击 wsB Agent 后 active workspace/pane 唯一匹配；
  Working→Waiting→Focused 与 Working→Disappeared 状态机；两表面颜色/文本同源；动态 title
  更新一次、无变化零事件。
- 预期落点：AgentCenter/AgentMemberRow、pane tree/header、`+page.svelte`、teammate model/tests。

### REQ-AGENT-HISTORY-SOURCE-02

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

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 行为：rdg 仪表盘 LAN 地址只显示根地址（如 `https://172.21.130.235:9527`），不附 `/login`；LAN Remote 默认停止，须用户显式启动；TUI 内须有名称明确的公网 Remote 启动/停止入口。
- 边界：不改变 `rdg remote` 子命令兼容性；退出 TUI 时仍须回收由本 TUI 启动的服务。
- 验收：dashboard 单测证明根 URL、初始 stopped、无启动 action；菜单文本与动作测试证明 LAN/公网入口明确。
- 追踪：`packages/ridge-cli/src/tui/dashboard.rs` → dashboard tests。

### REQ-REMOTE-02 · rdg LAN 桌面浏览器真正接入

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 行为：桌面浏览器访问 rdg LAN Remote 时直接走 LAN TOTP/session 启动链，完成 WebSocket、workspace、pane、PTY 接入；不得只显示空白桌面壳。
- 边界：公网租户域仍走 Cloud WebRTC/E2EE；LAN 与 Cloud 启动判定不得依赖远端 cloud API 成败。
- 验收：启动判定纯逻辑测试覆盖 LAN IP/localhost 与 cloud 租户/query；LAN host 协议 probe 或等价集成测试证明握手、订阅、stdin 回显。
- 追踪：`src/routes/+layout.svelte`、新增启动判定 helper/tests、`packages/ridge-cli/src/tui/lan_host*`。

### REQ-CLOUD-01 · 公网 Remote 设备配额不得误停 rdg

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 行为：host 与 controller 均按数据库实时用户组计算设备配额；配额自动停用与用户手动停用须分因记录；额度恢复时仅自动启用“因配额停用”的设备。
- 边界：不绕过会员 Remote 权限、设备归属、WS 并发或 controller 数量门禁；手动停用不可被后台 daemon 自动撤销。
- 验收：ridge-cloud 单测覆盖会员 host 不按免费额度降级、quota/manual 两种停用区分、额度恢复只恢复 quota-parked；WS 门控回归全绿。
- 追踪：`ridge-cloud/src/ws/handler.rs`、`src/db/device_quota.rs`、`src/db/device_repo.rs`、顺序 migration。

### REQ-MOBILE-01 · Mobile Remote 弹层、图标与按钮

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 行为：工作区/终端类型切换层通过 portal 挂到 `body`；顶部 Agent 协作入口使用小机器人；Agent 图标后不跟“标记/运行中/启动中”等标记文案；手机 Remote 右侧功能按钮不显示 border 外壳。
- 边界：保留按钮的 `title`/`aria-label`、触控尺寸、焦点与点击行为。
- 验收：Svelte/Vitest 断言 portal action、Bot 图标及无标记文案；移动构建与 svelte-check 全绿。
- 追踪：`src/remote/lib/WorkspaceTree.svelte`、`src/remote/MainApp.svelte`、`src/remote/lib/RemoteSidebar.svelte`、`src/lib/components/SplitContainer.svelte`。

### REQ-AGENT-01 · 全局 Agent Center、pane 状态与最近回复

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 行为：Agent tab 聚合所有已打开工作区的 agents，不以焦点工作区过滤；原顶部操作移入可滚动内容；自动识别的 agent 进程须同步 pane header 与 roster 状态；最近回复从 Claude/Codex JSONL 会话历史提取并显示。
- 边界：工作区目标、编组编辑等写操作仍须显式落到所属工作区；JSONL 扫描须有文件数、单文件读取量与返回条数上限，不上传会话内容。
- 验收：聚合模型与进程识别单测；Rust JSONL fixture 测试覆盖 Claude/Codex assistant 文本、项目过滤与有界排序；pane header UI 不显示尾随标记文案。
- 追踪：`src/lib/teammate/AgentCenterPanel.svelte`、`src/lib/components/RidgePane.svelte`、`src-tauri/src/commands/project.rs`。

### REQ-AGENT-02 · Agent 启动的无头 Shell 发现与唤起

- 状态：`ACTIVE`
- 版本：`v0.2.0`
- 行为：经 Ridge tmux shim 由 pane/agent 启动的 native 无头 session 记录创建工作区与 pane；Agent Center 自动列入对应 agent，支持一键召唤到当前工作区；未能归因的普通无头 session 仍可单列。
- 边界：仅承诺 Ridge 持有 PTY master 的 native session 可召唤；任意已脱离 PTY、仅剩 OS PID 的后台进程不可伪装成可接管会话。
- 验收：ridge-tmux 测试证明 creator metadata 从 HTTP header 入 session/list DTO；tmux shim 测试证明工作区/pane header 传播；前端测试证明按 agent 归组与 attach 调用。
- 追踪：`src-tauri/src/bin/tmux.rs`、`src-tauri/src/commands/terminal.rs`、`packages/ridge-tmux/src/{http.rs,lib.rs}`、`src/lib/stores/hosts.ts`、`AgentCenterPanel.svelte`。

## 修订账本 (Revision Ledger)

| 版本 | 日期 | Pending ID | 变更 | 关联/取代 | 批准证据 |
| --- | --- | --- | --- | --- | --- |
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
