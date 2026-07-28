# Iteration 63 Contract · Mobile Remote 持态与交互

状态：`APPROVED`

批准需求：`REQ-MOBILE-REMOTE-STATE-01`

范围：手机 Remote（LAN 与公网 Cloud）状态、后台 pane、软键盘、scrollback 与 pane 行控件。桌面端仅改共享实现所必需部分。

## 不变量

1. Query cache 仅存 Remote 可序列化快照；PTY bytes、terminal kernel、DOM/UI 临时态不得入 cache。
2. 同 query key 刷新失败时保留最近成功数据；push 以 canonical ID 合并，成功 snapshot 方可替换。
3. session 内每个曾激活且仍存活的 `(workspaceId, paneId)` 保持订阅与 feed；普通 pane/workspace 切换不得退订、清 kernel 或 replay。
4. 后台 pane 不 claim/fit/resize；仅可见 pane 拥有尺寸。切回时有界 claim，但不得清 terminal。
5. 软键盘只改变 `.term-stage` 视觉 transform；不得改变 `.container`、canvas、PTY grid 或触发 resize。
6. scrollback 页仅在 `endSeq === currentOldestSeq` 且 prepend 成功后 commit cursor；失败、重叠、缺口均不得推进。
7. A pane 请求 scrollback 时切到 B，返回页仍须写入 A 的 parked kernel。
8. 不以 pane 数、时间或内存启发式降级后台保活；边界沿用 live pane、既有 kernel 行数与传输背压。
9. 弱网 active QoS 使用同一认证链路内逻辑保留通道；不得新建第二 WebSocket、PeerConnection 或 DataChannel。

## T1 · Query 与 UI store

目标：

- 引入 `@tanstack/svelte-query`，由单一 `QueryClientProvider` 承载会话。
- workspace、按 workspace 分键的 pane snapshot、capability snapshot 进入 Query。
- active workspace/pane、sidebar、viewer、selection、sentence buffer、keyboard 等本地态进入 Svelte store。
- 同 key refetch 静默保留旧 UI；跨 workspace 命中各自 cache。

允许路径：

- `package.json`
- `pnpm-lock.yaml`
- `src/remote/App.svelte`
- `src/remote/MainApp.svelte`
- `src/remote/lib/*query*`
- `src/remote/lib/*state*`
- 对应测试

验收：

- 静默刷新期间已有列表不闪空。
- push 与 refetch 后无重复 canonical ID。
- Query cache 中无 raw PTY bytes、kernel 或 DOM 引用。

## T2 · 跨 workspace 后台 pane

目标：

- session 注册表以 `(workspaceId, paneId)` 为键，记录曾激活且仍存活 pane。
- LAN 与 Cloud 的 `subscribePane` 接受可选 `workspaceId`，重连恢复全部，当前 pane 最后恢复。
- `subscribePane` 另接受可选 active 标记；active 输出/控制进高队列，background 进低队列。每个低帧后重查高队列。
- 所有已注册 pane 的 raw bytes/metadata/resize 可达对应 parked kernel。
- workspace/pane 关闭只清对应键；socket teardown 清全部 host 注册，客户端 kernel 留待重连。
- LAN `current_pane` 继续作为 Files/Git/Search 当前 cwd；不得与后台订阅集合混用。
- LAN 高低队列沿用 `RAW_CHAN_CAP` 并 biased drain；Cloud background 仅在既有低水位以下准入，active 使用低/高水位间保留容量。
- background 队列满时订阅不减，只按 pane 标 `dirty`；重新 active 后以 ordered barrier 恰一次有界 canonical resync，再续 live。

允许路径：

- `src/remote/MainApp.svelte`
- `packages/remote/src/shared/transport/wsRemote.ts`
- `packages/remote/src/web/cloudRemote.ts`
- `packages/remote/src/shared/terminal/*`
- `src-tauri/src/remote_host_impl.rs`
- 对应测试

验收：

- `ws1/A → ws1/B → ws2/C → ws1/A`，A/B/C 皆持续收流；回 A 显示离开期间最新输出。
- 普通切换无 unsubscribe、无 kernel clear、无 full replay。
- 后台 pane 无 resize/claim。
- 断线重连恢复全部 live visited pane；删除 pane/workspace 后无孤儿 listener/注册。
- 弱网 background 洪泛时，active 最多等待一个已开始的低帧；background 不令 active dirty。
- dirty pane 切回时 barrier 前旧 live 不拼、snapshot 原子落入、barrier 后 live 续接；无重复、空洞或 RIS 循环。

## T3 · 软键盘视觉位移

目标：

- 以 visual viewport、terminal 可见框、输入锚与 `cellH` 算有限负向位移。
- 位移施于 `.term-stage`，shared canvas、pane canvas 与 hidden input 同源移动。
- shared-grid pointer/touch 以同一 offset 反算；非 shared 分支不得双重补偿。

允许路径：

- `src/remote/MainApp.svelte`
- `src/remote/TerminalCanvas.svelte`
- `src/remote/lib/keyboardOffset.ts`
- `packages/remote/src/shared/terminal/*`
- 对应测试

验收：

- 键盘开合时 DOM 容器高、canvas 像素尺寸、terminal rows/cols 与 PTY resize 计数不变。
- 光标已可见则零位移；遮挡时位移有界，且保留顶部上下文。
- transform 后 mouse/touch 命中 cell 与视觉位置一致。

## T4 · Scrollback 原子分页

目标：

- 统一页结构含 `[startSeq, endSeq)`、bytes、`atOldest`。
- transport/pager 单飞；先验邻接，parked kernel prepend 成功后方 commit cursor。
- host 统一选择 UTF-8 完整且尽可能位于换行后的页起点；LAN/Cloud 不各写内容去重。
- shell 顶部显示 absolute loading 光条及 `aria-busy`；不参与布局。

允许路径：

- `src/remote/MainApp.svelte`
- `src/remote/TerminalCanvas.svelte`
- `packages/remote/src/shared/transport/*`
- `packages/remote/src/web/cloudRemote.ts`
- `packages/remote/src/shared/terminal/*`
- `src-tauri/src/remote_host_impl.rs`
- `src-tauri/src/state.rs`
- `packages/ridge-term/src/term/terminal.rs`
- 对应测试

验收：

- 相邻页 seq 连续；重复、重叠、缺口、异常、关闭皆不推进 cursor，并释放 loading/单飞。
- A 请求中切 B，页落入 A；切回无缺页。
- CRLF、宽字符、SGR、超长行与连续页无大片重复/空白，阅读锚不跳。
- loading 光条出现且消失；terminal/canvas 布局尺寸不变。

## T5 · Pane 行纯图标控件

目标：

- WorkspaceTree 的 Agent、Shell 触发器去边框、pill 与背景。
- Agent 去文案，只以 icon 颜色表达状态。
- 保留原透明命中盒、动态 `title`、`aria-label`、键盘/点击行为。

允许路径：

- `src/remote/WorkspaceTree.svelte`
- `src/remote/PaneShellPicker.svelte`
- 对应组件测试

验收：

- idle/active/open 仅颜色或透明度变化，无边框/背景。
- 命中区域不缩小，辅助名称仍随状态更新。

## T6 · 集成、E2E 与质量关

必测：

- 单元/组件：Query 保旧值与 push 去重；visited registry；keyboard transform/命中；scrollback seq/commit；纯图标 a11y。
- Rust：多 workspace 订阅生命周期；scrollback 安全边界与连续 prepend。
- LAN 真链路 mobile E2E：跨 pane、跨 workspace、后台持续输出、键盘开合、命中、scrollback loading/拼接。
- Cloud 自动测用可控 fixture/mock transport；不得以生产凭据作 CI 必需条件。
- 相关 typecheck、lint、测试、构建均绿；随后执行 Sonar scanner 并记录 project key、连接与 quality gate。

停止条件：

- 任一正常切换仍退订、清 kernel、重放全量或停止后台 feed。
- 任一后台 pane 可 claim/resize。
- 后台 backlog 可排在新 active 帧前，或 active 保留容量可被 background 占用。
- 键盘路径改变布局高度、canvas/grid 或触发 PTY resize。
- 非邻接 scrollback 页仍可 commit。
- Query 持有 raw bytes/kernel。
- E2E 仅以 mock 证明 LAN 真链路。
- 以第二物理连接、pane 数、固定时长、FPS 或内存启发式换取 active 流畅。

非目标：

- 不改 ridge-cloud 服务架构。
- 不重写 Files/Git/Agent provider。
- 不新增 pane 数/超时/内存降级策略。
- 不清理或迁移与本需求无关的用户文件。
