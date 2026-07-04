# 2026-07-04 — 手机远控 / 桌面 / 主机接入 / 多智能体 一批修复设计

/ goal 批量修复。跨四个子系统的 17 项 bug / 体验 / 业务合理性修复。每项都先读码定位**根因**，
再最小化修复。多数改动跨 Rust（需 rebuild）或 src/remote（需 host 重服 static/remote），本机无法
端到端真机验证——已尽力 `svelte-check` / `cargo check` / `vitest` 静态 + 单测把关，真机验证留待
rebuild + 设备联调（承既有「未真机验」惯例）。

## 分组与文件局部性（并行安全）

- **A. 手机 PWA（`src/remote/*`）**：A1–A11。同一批紧耦合文件，主会话亲自改，串行保证一致。
- **B. 桌面终端渲染（`packages/ridge-term`）**：B1。独立 crate，后台 agent。
- **C. 桌面主机接入（`src-tauri` + `src/lib/components/hosts`）**：C1（pane.rs 双关闭）、C2（对话框）。
- **D. 多智能体（`ridge-core/teammate` + `src-tauri/teammate` + `src/lib/teammate`）**：D1–D3。

---

## A. 手机 PWA 远控

### A1 触摸选区定位错（手指 A 区、选中 B 区）
根因：`terminalController.ts` 的 `startSelection`/`extendSelection` 用 `row + scrollOffset` 换算绝对行——
**反向且缺 scrollbackLen 基底**的错公式（只有 scrollbackLen=0 时凑巧对）。桌面 `manager.ts` 的
`vpToAbsRow` 早已是正确的 `scrollbackLen + row − scrollOffset`（注释明确记录旧 `vp+off` 是 bug）。
wasm `set_selection_abs` 的坐标系：live grid 行绝对行 = `scrollback_len + vp_row`（见 lib.rs 测试）。
修复：两处均改 `absRow = this.rowsAboveViewport() + row`（= `scrollbackLen − scrollOffset + row`，
`rowsAboveViewport()` 已 `max(0,…)` 兜底防下溢）。同步重写 `terminalController.test.ts` 5 个断言
（原测试编码了 bug 值）。✅ vitest 55 绿。

### A2 复制到手机系统剪贴板不可靠
根因：`navigator.clipboard` 仅安全上下文可用；旧 fallback 用 readonly textarea + `select()`，iOS Safari
下选不中→execCommand 复制空。修复：抽 `src/remote/lib/clipboard.ts` 共享 `writeClipboard()`——先试
async API，失败退回 **iOS 硬化**的 execCommand（contentEditable + 非 readonly + Range + setSelectionRange）。
TerminalCanvas 与 FileViewer 共用（DRY）。注：A1 修好后选中内容正确，是「复制拿到空/错内容」的另一半根因。

### A3 文件「复制」按钮复制的是文件名而非内容
根因：`FileViewer.copyPath` 写的是 `path` prop。修复：改 `copyContent` 写已载入的 `content`（diff 视图=diff 文本），
带 loading/error/空 守卫 + 复制成功态；i18n `viewerCopyPath`→`viewerCopyContent`（zh/en）。

### A4 关闭文件回到文件目录而非终端
根因：`MainApp` 的 `onClose` 只清 `viewer`，遗留的 `sidebarTab` 会重新露出侧栏目录。终端仅在两个 overlay
都为 null 时渲染。修复：新增 `closeViewer()` 同时清 `viewer` 与 `sidebarTab`。

### A5 LAN 懒加载 scrollback 未实现（cloud 已实现）
根因：cloud 路径（cloudRemote）已「先拉 tail + 滚顶 before 分页」；LAN 路径（wsRemote ⇄ remote_host_impl）
host 在 subscribe 时推固定 64KiB tail（无 seq 元数据），client 无 `fetchOlderScrollback`。修复（后台 agent，
镜像 cloud 契约）：
- host `subscribe-pane`：改用 `get_pty_scrollback_tail` 推 tail，随后补一条 `scrollback-meta`
  `{startSeq,atOldest}` 供 client 播种游标；新增 `scrollback-before` 帧 → `get_pty_scrollback_before`
  回 `scrollback-before-result`（均带 per-client `active_ws_id`，非全局）。
- client wsRemote：`scrollbackCursor`/`fetchingOlder` + `scrollback-meta` 播种 + 实现 `fetchOlderScrollback`
  （`_sendAndWait('scrollback-before-result')`）+ disconnect 清理。MainApp/TerminalCanvas 既有 onNearTop 接线不改。
- **rdg（lan_host_impl）无 seq 游标 scrollback 存储 → 不伪造，LAN 懒加载暂桌面 host 专有**。
✅ cargo check -p ridge + svelte-check 绿。⚠️需 rebuild host + 真机验。

### A6 手机重连后接上 scrollback 但不接实时渲染（cloud）
根因：`cloudRemote.subscribePane` 在 `ptyUnlisten.has(paneId)` 时提前返回；重连后旧订阅条目仍在→
MainApp.onReconnect 的 subscribePane 被吞→不重挂 `pty-output` 监听、不重 `register_pane_delta_channel`。
scrollback 从缓存重绘（故「接上了」），live 不恢复。修复：`_handleReconnect` 先 `_teardownSubscriptions()`
（unlisten + 清 ptyUnlisten/subscribing/scrollbackCursor/fetchingOlder），再 fire reconnectListeners；
disconnect() 复用同一 helper。LAN 路径无此 guard，不受影响。

### A7 虚拟键盘顶吸头部、不被输入法顶飞遮内容
现状：VK 在 `position:sticky;top:0` 的头部内，画布用 keyboardOffset 上移。当移动浏览器为露出焦点输入而
上滚 fixed 布局时，头部（含 VK）可能被滚出可见区。修复：MainApp 加 `visualViewport` 的 offsetTop 跟踪，
`transform: translateY(headerShift)` 把头部重新钉到**可见视口顶**（offsetTop=0 时为 no-op，各视口模型安全，
有界）。⚠️设备相关、CDP 无法真实模拟软键盘 → 需真机验。

### A8 键盘布局：左右箭头下移第二行、原位放 `/` `@`
修复：VirtualKeyboard 方向组改 3×2：第一排 `/ ↑ @`、第二排 `← ↓ →`（箭头成倒-T）。新增 `sendChar()`
（复用 handleVirtualKey 的 encodeKey 单字符路径），网格 CSS 重排 + `.sym` 样式。

### A9 Ctrl 未手动切换则一直保持选中
根因：modState 文档与代码矛盾；实际一次性清除只发生在命名键/软字符路径，其它情况残留。修复：重写 modState
为**一次性 vs 锁定**三态：单击=armed（下一键消费即释放）、再击=locked（caps-lock 跨键保持）、三击=off。
`consumeMods()` 清一次性、保留锁定；VK 各发送路径统一走 consumeMods；locked 用实心强调色区分。

### A10 工作区树无 store / 无持久化，每次重载
根因：`WorkspaceTree` 的 `expandedWs`/`peekedPanes` 每次挂载重置。修复：抽 `treeState.svelte.ts` 持久化
store——**只持久化 UI 偏好**（展开集 `expanded` + 首次自动展开去重集 `seen`）到 localStorage，**pane/工作区
数据仍实时**从 host 拉（不持久化，避免陈旧）。`seedActiveWorkspace` 只首见时自动展开一次（尊重后续手动折叠
跨刷新）；`pruneExpanded` 随工作区列表收敛防膨胀。

### A11 头部标题下加 cwd
修复：header 面包屑改纵向堆叠：标题行 + `compactCwd(activeCwd)`（取末两段 `…/repo/dir`，full 在 title 提示）。

---

## B. 桌面终端渲染

### B1 未滚到底时 TUI 输入光标不显示
根因：`ridge-term/render/renderer.rs` 的光标门 `offset == 0`（`tick` 与 `is_dirty` 两处）在任何上滚时强制
`new_cursor=None`；`compute_cursor_draw` 又用裸 `cur.row` 无 offset 补偿。修复：去掉 `offset==0` 门（保留
focused && blink），`compute_cursor_draw(offset)` 发 `row: cur.row + offset` 并在 `cur.row+offset >= rows`
（滚出底部）返回 None。✅ cargo check -p ridge-term 绿。⚠️需 wasm 重建。

---

## C. 桌面主机接入

### C1 无头会话关闭报 "Pane not found" 但实际关闭挂回后台
根因：foreign/headless pane 的 `native_ref` 分支里 `kill_pty_if_present`（terminal.rs）detach 时**已**
`pane_tree.close`，`close_pane`（pane.rs）随后又 close 一次→`PaneNotFound` 字符串外泄到前端 alertDialog。
本地 pane 无 native_ref 不触发。修复：`close_pane` 的第二次 close 容忍 `CoreError::PaneNotFound`（native 已摘除
属良性），其它错误仍上抛。✅ cargo check -p ridge 绿。⚠️需 rebuild。

### C2 连接远端主机对话框：整窗居中 + 只分 LAN/公网 + 接入方式同网页
修复：重写 `HostConnectDialog.svelte`——
- `use:portal` 整窗居中（逃出侧栏 backdrop-filter 包含块，复用 CloudProModal/SaveWorkspaceDialog 先例）。
- 通道只分**局域网 (LAN) / 公网**（去掉 ridge/rdg 实现细节暴露）。
- LAN：无需登录，只填「地址 + TOTP 验证码 + 别名」。
- 公网：先登录云端账户（**复用 `cloud/auth` 的 loginViaBrowser + email/password + cloudAuth store**，与公网远控
  tab 同一账户）；已登录则同样只填地址 + TOTP。
- 后端 `connect_host` live 传输仍是 scaffold（凭据不落库），LAN/公网当前都登记为 'remote'（后端暂不区分通道）；
  已在 UI 与文案标注 live 传输为下一里程。后端 HostKind 'rdg' 保留（pane origin / HostsPanel 徽标等仍用）。

---

## D. 多智能体（Domain Zero teammate）

### D1 自动识别智能体能力（+ 竞选 Leader）
背景：能力画像 + Leader 竞选曾实现后在「底座化瘦身」删除。用户要求最小化重新引入。方案（后台 agent）：
ridge-core 加轻量 `AgentTier{Base,Skilled,Expert}` + `recognize_capability(name,program)`（关键词：claude/opus→Expert、
codex/gpt/sonnet/gemini→Skilled、else Base）；`elect_leader`（最高 tier，Uuid 定序 tiebreak）；profiles/topology/
commands 让 `topology.leaderId` + `role` 反映真实竞选结果。⚠️需 rebuild + live agents 验。

### D2 给组派任务只发给 Leader（不广播全体）
根因：`TeammateGroupsSection.dispatchTask` 对全体在线成员逐个 `write_to_pty`。修复：选中在线成员里
`profile.role==='Leader'`（无 Leader 回退首名在线），单发给 Leader，由其自行分派。依赖 D1 提供真实 leaderId，
无 D1 时回退首名——绝不静默无投递。

### D3 发消息回车不生效 / 变成换行
根因/取舍：现输入是单行 `<input>`（Enter 已 send，但无 IME 守卫；用户报「回车变换行」指向 textarea 语义）。
修复：改 `<textarea rows=1>` 并加标准聊天输入语义——Enter 发送、Shift+Enter 换行、**输入法拼字中的 Enter 确认
候选词不发送**（`!e.isComposing` 守卫，修 CJK 提前发送/阻断确认）。

---

## 验证与上线

- 静态：`pnpm check`（svelte-check 0/0）、`cargo check -p ridge / -p ridge-term / -p ridge-core`、
  `vitest`（terminalController/cloudRemote/paneScrollbackCache + teammate）。
- ⚠️**未 rebuild、未真机验**：Rust 改动（B1 renderer→wasm 重建、C1/A5 host、D1）需重建 ridge；src/remote 改动需
  host 重服 `static/remote` + 手机 PWA 刷新；A7/A2 软键盘/剪贴板需真机。
- 共享 tree：本机 develop 多会话共用工作树，提交按文件/hunk 隔离（见 feedback_shared_tree_git_amend）。
