# Contract · Iteration 66 · Remote 连续性与终端输入闭环

状态：APPROVED  
日期：2026-07-29  
需求：`REQ-REMOTE-SMOOTH-STATE-02`、`REQ-REMOTE-03`、
`REQ-TERMINAL-PASTE-ORDER-02`

## 当前事实

- 复合 `PaneRef`、后台订阅、scrollback Worker、高/低 writer lane、共享几何函数与
  bracketed-paste 已存在。
- iteration 63 自动闸曾绿而用户真机失败；旧测试数不作完成证据。
- 当前缺口先视为运行链或验收链未闭，不预设再造协议或状态源。

## 目标 1 · 输入原子性

- 桌面与 Remote paste 皆编码为单一 payload。
- 同一 `(workspaceId,paneId)` 的 paste、键入、MCP/程序注入共用 FIFO 出口。
- 不以逐行延时、固定 sleep、跨 pane 全局锁修序。

验收：

- LF/CRLF、Unicode、末尾无换行 fixture 字节序等于输入。
- paste 与紧随键入并发时，后者不得越过 paste。
- Windows ConPTY echo 与 Remote delta mirror 保序。

## 目标 2 · Remote 复合身份与后台保活

- subscribe/stdin/claim/refresh/scrollback、raw/meta callback 皆携带并校验
  `(workspaceId,paneId)`。
- 普通切换仅改变 active、焦点与尺寸所有权；visited pane 的 kernel/订阅继续存活。
- 迟到响应仅提交至请求发起 PaneRef；关闭按复合身份精确释放。

验收：

- `wsA/pane1` 未决时切 `wsB`，迟到 list/raw/meta/scrollback 不写入 `wsB`。
- `ws1/A → ws1/B → ws2/C → ws1/A` 后三 kernel/订阅存续。
- 缺失或错误 workspace 的业务帧 fail-closed。

## 目标 3 · 输入优先与分页有界

- WebSocket sink 仅由独占 writer 持有。
- control/active raw 优先；background raw/scrollback 有界、可取消、满载不阻塞 reader。
- scrollback 解码、seq 校验在 Worker；仅 prepend 成功后推进 cursor。

验收：

- 挂起 low sink 时 stdin/control/active raw 仍推进。
- 取消后低优先队列、请求与任务计数归零。
- 快速滚顶、切 workspace、关闭 pane 时旧页不提交。

## 目标 4 · 几何与软键盘

- pane 内容 rect、padding、cell、DPR 为 viewport/grid/pointer/claim 共用真相。
- 显式唤起键盘依次执行：
  `scrollToBottom → resolve cursor/fallback center → focus({preventScroll:true})`。
- pointer/touch 不参与键盘锚点；visualViewport 只作有界位移，不改 grid/PTY。

验收：

- DPR `1/1.25/1.5/2` 与分数像素 fixture：rows/cols、viewport、pointer cell 同源。
- 不同触点产生同一 IME 锚点；无效 cursor 回退可见区中心。
- 键盘开合不改变 container/canvas 尺寸或增加 PTY claim。

## 允许路径

- `src/lib/components/RidgePane.svelte`
- `src/remote/MainApp.svelte`
- `src/remote/lib/{TerminalCanvas,keyboardOffset,scrollbackWorker}*`
- `packages/remote/src/shared/terminal/{manager,paneGeometry,ptyBridge}*`
- `packages/remote/src/shared/hosts/liveBackpressure.test.ts`
- `src-tauri/src/remote_host_impl.rs`
- `scripts/remote-state-e2e.mjs`
- 对应本轮文档与归档

## 禁止路径

- 第二 WebSocket/DataChannel、无界缓存或 Query cache 持有 PTY bytes
- `activeWorkspaceId` 代填业务帧 workspace
- 固定 sleep/FPS、逐行 paste、全局输入串行
- 发布、版本号、第三方 session 写入

## 质量闸

- 聚焦 Vitest：keyboard、scrollback、query identity、pane geometry、PTY FIFO、live backpressure。
- Rust：explicit workspace、multiline order、active lane 与任务回收。
- `pnpm check`、desktop/mobile production build、`cargo check -p ridge`。
- 启动受控 LAN host 后运行 `node scripts/remote-state-e2e.mjs`；进程有墙钟超时并回收进程树。
- 公网、iOS、Android 缺真实凭据/设备时，只记用户轨缺口，不冒充完成。

## 停机条件

- 发现需新增产品语义、第二状态源或新协议面。
- 确定性测试证明当前实现已满足而真实失败无法复现：转采集诊断，不盲改。
- 外部真实设备或公网凭据不可得：自动轨继续，用户轨留证，不发布。

## 追踪

| 需求 | 主落点 | 完成信号 |
| --- | --- | --- |
| `REQ-TERMINAL-PASTE-ORDER-02` | RidgePane / manager / PTY | 单 payload + 同 pane FIFO + 真 ConPTY |
| `REQ-REMOTE-SMOOTH-STATE-02` | MainApp / TerminalCanvas / host writer | 身份竞态、保活、背压、键盘全绿 |
| `REQ-REMOTE-03` | paneGeometry / manager / E2E | DOM/grid/PTY/pointer 同源 |
