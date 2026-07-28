# NotebookLM guidance 63 · Mobile Remote Pending 对抗评审

状态：需求已批准；合同见 `CONTRACT-iteration-63.md`，业务码尚未改。

## Maker 建议

- Query 只管 Remote 可序列化拓扑；UI 与 PTY/kernel 不入 cache。
- 当前 workspace 内 visited pane 有界保活。
- transform 须与 pointer-to-cell 共用偏移。
- 后台 kernel 须受既有 scrollback 上限与清理合同约束。
- 复用 Cloud 每-pane listener Map、TerminalManager parked kernel。

## 对抗裁决

- 采纳：LAN 多订阅合同；transform 命中补偿；5000 行既有硬上限；关闭/workspace 切换/断连归零。
- 校正：LAN host 落点为 `src-tauri/src/remote_host_impl.rs`，非 maker 所称 `ridge-cli lan_host`。
- 驳回：无证据的 `1MB` / `5 pane` 新阈值；live pane 集合与既有 scrollback cap 已给确定边界。
- 驳回：以桌面 `REQ-REMOTE-03` 反对移动端 transform。用户已明确禁止键盘改变 canvas/container 高度及 PTY grid；本轮以“grid 恒定 + 视觉变换 + 命中同源”作 checker。
- 约束：Query cache 是远端快照投影，不是协议/业务 SSOT；push 只合并 canonical ID，重取成功覆盖，失败保留最近成功数据。

## 代码事实

- `MainApp.svelte`：`workspaces/panes` 仍为手写 `$state`；刷新与 push 直接替换。
- `MainApp.svelte`：`onRawBytes` 仅 feed active pane。
- `remote_host_impl.rs`：mobile `subscribe-pane` 替换单 `current_pane`；desktop global 才用 Set。
- `cloudRemote.ts`：已有 `ptyUnlisten: Map<paneId, UnlistenFn>`。
- `TerminalManager`：parked kernel 存续且 `feed(paneId, bytes)` 可直投。
- `TerminalCanvas.svelte`：当前键盘方案写内联 `height`，并两次 `viewportChanged`，会改 grid/PTY，违反 Pending。

## 可行性补证

- `TerminalManager.inputAnchorResolved(paneId)` 已同源给出输入锚的 `row/col/x/y/cellH`；无需另造光标观测器。
- 视觉位移应施于 `.term-stage`，使共享 host canvas、per-pane canvas、hidden textarea 与容器同移；其布局高保持不变。
- 非 shared `cellFromEvent` 读取变换后的 `getBoundingClientRect()`，天然同移；shared 分支读缓存 `PaneGeometry`，须以同一 shift 补偿 clientY，禁止借 `viewportChanged()` 重算并误触 PTY resize。
- LAN `current_pane` 还决定 Files/Git/Search cwd，不能删除；应保留为“当前上下文”，另以当前 workspace 的 `subscribed_panes` Set 管后台流。workspace 切换/关闭与 socket teardown 同点 drain。
- `RemoteLink.listPanes()` 现为 fire-and-forget；Query 接线宜加本地 Promise adapter 等待下一份 canonical `panes` 快照，复用既有 `list-panes` 帧，不扩协议。

## 用户修订后二次评审

- 用户明确：后台保活跨 workspace；普通 workspace 切换不得退订或停 feed。
- 采纳 maker：session 级 `(workspace,pane)` 注册；LAN/cloud 对称 checker；visited Set、listener、parked kernel 三者对账；重连恢复全部。
- 校正：LAN 输出在 `src-tauri/src/remote_host_impl.rs`，非 maker 再次臆称的 `ridge-cli/src/tui/lan_host.rs`。`raw_rx` 本就只收到已注册 subscription 的事件，移除 `active_ws_id` 过滤不会枚举未订阅 workspace。
- 驳回后台“静默 resize”：parked pane 无可见容器，强算尺寸会制造第二所有者并违用户键盘/几何约束。仅前台 pane fit/claim；后台保留最后 grid，切回有界 claim，kernel 与流不清。
- 驳回按物理内存暂停最旧流：无稳定跨浏览器信号，且违用户“所有已激活 pane 保持连接”。边界仍为 host live-pane 并集 + 每 kernel 5000 行 + 既有传输背压。
- UI：不把 20px 写成新验收；保留不缩小的透明命中盒与动态 `title/aria-label`，视觉删除 Agent/Shell border、pill、背景及 Agent 文案，仅以 icon 色表状态。
- 协议最小增量：既有 `subscribe-pane` 仅追加可选 `workspaceId`，使重连时可恢复非当前 workspace；不建新消息类型。当前 pane 最后恢复，保住 Files/Git/Search cwd 上下文。

## Scrollback 补充需求代码核验

- 当前 `loadOlderScrollback()` 在请求中切 pane 后丢弃返回页；transport 却已先推进 `scrollbackCursor`，故该页永久缺失。跨 pane 保活后应按原 pane id 直投 parked kernel。
- LAN/cloud 现只返回 bytes/startSeq/atOldest；缺显式 endSeq 与客户端邻接校验。Pending 要求页区间满足 `endSeq == old oldestSeq` 后方 commit。
- host `get_pty_scrollback_before` 保证取 `seq < beforeSeq`，但只对 chunk 起点作 UTF-8 边界对齐；独立 sandbox parse 遇跨页半行/ANSI seam 仍可裂行。应在 host 统一选择终端行安全边界，LAN/cloud 同源，不在两 transport 各写内容去重。
- `Terminal::prepend_scrollback` 已隔离 live grid/cursor/attrs，并 trim flush 产生的尾空行；保留此实现，补相邻页/CRLF/宽字符/ANSI seam 回归，勿重写 renderer。
- loading 置于 shell 顶部 absolute overlay，不参与 flex 高度；每 pane 单飞状态，所有终态收束。

## Scrollback 对抗裁决

- 采纳：请求页先验 `[startSeq,endSeq)` 邻接；对应 parked kernel prepend 成功后，transport/pager 方可 commit cursor。
- 采纳：A 请求期间切 B，结果仍按 pane id 入 A；删除现有 `activePaneId === pid` 丢弃门。
- 采纳：absolute 光条 + `aria-busy` + indeterminate `progressbar`；空页/失败/关闭皆 finally 收束。
- 采纳：viewport 阅读锚不因 push-front 跳动；sandbox 仍为函数内短命对象，现实现已自动释放。
- 收窄 maker 的“ANSI 扫描”：host 统一把页起点选在 UTF-8 完整、换行后的安全点；LAN/cloud 复用同一 `ScrollbackChunk`。不另写 transport 字符串去重，不承诺解析任意恶意多行 OSC 为新协议。

## 批准后合同校正

- 采用 `@tanstack/svelte-query`，非 maker 误称的 `svelte-query`；provider 落在现有 `src/remote/App.svelte`。
- loading 落在 `TerminalCanvas.svelte`；不经 `RidgePane.svelte` 另造层。
- 驳回 maker 的 `20px` 新命中阈值；合同只要求不缩小既有透明命中盒。
- 驳回 `500ms`、`3 pane`、内存启发式等降级；已激活且存活 pane 全部保活。
- 驳回后台 resize fallback；尺寸所有权只属可见 pane。
- 驳回无法找 seam 时按 PTY 宽度硬截；分页须 seq 邻接、UTF-8 完整，并尽可能从换行后起页。
- 真 E2E 路径固定为 `ws1/A → ws1/B → ws2/C → ws1/A`；Cloud 自动门禁不得依赖生产凭据。

## 弱网 active QoS 对抗评审

- 代码事实：LAN 为每连接容量 `512` 的共用 raw FIFO，`ws_tx` 串行；Cloud 为单 ordered DataChannel，现有低/高水位 `1 MiB / 8 MiB`，全 pane 共用背压。
- 采纳 NLM：不建第二物理连接；同链路作逻辑 active 高优先、background 低优先；后台溢出按 pane 标缺，恢复不可常态全量 replay。
- 驳回 NLM：新造 `2 MiB`、`256`、`<200ms`、`10 pane` 数字均无校准证据。
- 驳回 NLM：FPS < 30 或 pane > 5 即停止后台 feed，违反用户明确的跨 workspace 保活。
- 校正：无 live seq 的当前协议不能仅靠 controller 猜空洞；恢复须经 ordered barrier，使 barrier 前 live 丢弃、canonical snapshot 原子落入、barrier 后 live 续接。
- 校正：物理链路被 active 自身打满时无“高速”保证；合同只保证 background 不占 active 保留容量、不造成应用层队头阻塞。
