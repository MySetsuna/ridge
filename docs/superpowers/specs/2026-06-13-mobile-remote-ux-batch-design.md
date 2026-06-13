# 手机端 Remote UX 批量修复设计

日期：2026-06-13
范围：`src/remote/*`（手机远控 PWA）+ `src-tauri/src/remote/server.rs`（host WS 协议）+ 共享 sidebar。

## 背景

用户在手机端 Remote 反馈一批问题，覆盖虚拟键盘排版、底栏功能键、工作区树形选择器、左上角 文件/Git/搜索 三个功能区，以及一个追加需求（不切换工作区即可展开查看其它工作区终端列表 + 终端标题实时刷新）。

## 根因诊断（已核实）

| # | 问题 | 根因 | 修复面 |
|---|------|------|--------|
| 1 | 键盘退格/回车超出屏幕；方向键排布不对 | `VirtualKeyboard` 用固定 `min-width` + `space-between`，宽度和 > 视口 → 溢出；方向键是倒 T（↑上、←↓→下） | 客户端 CSS |
| 2 | 粘贴键不可用 | `MainApp.handlePaste` 发 `{type:'paste'}`，**server.rs 无该处理** → 空操作 | 客户端（改读手机剪贴板） |
| 3 | 复制未进控制端剪贴板 | `copyAndClear` 已 `navigator.clipboard.writeText`，但尾随 `onStdin('\x03')` 会打断 shell；写入失败静默 | 客户端 |
| 4 | 选择键在刷新后 TUI 某些场景不可用 | TUI（mouse-reporting）下选择模式把拖动转成鼠标事件，无法选文本/复制 | 客户端 |
| 5 | 主题键不可用 | `handleThemeToggle` 发 `{type:'cycle-theme'}`，**server.rs 无该处理** → 空操作 | host + 客户端 |
| 6 | 工作区树选择器溢出/按钮被压缩 | 底栏 `group-left`(图标×5) 与 `tree-anchor` 均 `flex-shrink:0`，trigger label 撑到 160px → 总宽 > 视口 | 客户端 CSS |
| 7 | 文件/Git/搜索三区一直不可用 | **`data-request` 回包无 `type` 字段** → `wsRemote._handleMessage` 中 `type.endsWith('-result')` 抛 `TypeError`，被外层 `catch{}` 静默吞 → 回包永不达 `WsDataProvider` → 请求超时 → 面板空白 | 客户端（根因）+ host（补 `type`）|
| 7b | 文件查看器/差异查看器缺失 | `RemoteSidebar` 未传 `onOpenFile`；无 viewer 组件；git 面板无 diff-on-click | 客户端 + host(`git_diff_file`) |
| 8 | 追加：不切换即看其它工作区终端 + 标题实时刷新 | `list-panes` 只返回**活动**工作区 panes；标题取自上次 list-panes 易陈旧 | host(`list-workspace-panes`) + 客户端 |

## 设计决策

### 1. VirtualKeyboard（`src/remote/lib/VirtualKeyboard.svelte`）
- 方向键改正 T：第一排 `← ↑ →`，第二排仅 `↓`（grid col2 row2），↑/↓ 同列纵向对齐。
- 四组（左修饰 / 方向 / 导航 / Enter·⌫）用 flex 按列权重分配（3 / 3 / 2 / 1.4），键 `flex:1; min-width:0`，组间/键间 gap 收到 2px → 永不溢出、退格/回车恒在屏内。

### 2. 粘贴键（`MainApp.handlePaste` + `TerminalCanvas`）
- 改为客户端：`navigator.clipboard.readText()` → `canvasRef.pasteText(text)`（导出，走 `encodePaste` bracketed paste）。需用户手势（onclick 满足）+ 安全上下文（LAN TLS / 云端 HTTPS）。彻底脱离 host 支持。

### 3. 复制（`TerminalCanvas.copyAndClear` / `copySelection`）—— 两端都写（用户确认）
- 写**控制端（手机）**剪贴板：`navigator.clipboard.writeText` + `execCommand` 兜底；**移除尾随 `\x03`**（复制不应打断 shell）。
- 同时写**被控端（桌面 host）**系统剪贴板：新增 `onHostClipboard(text)` 回调 → `ws.setHostClipboard` → WS `set-host-clipboard` → host `app.clipboard().write_text`（clipboard-manager 插件，AppHandle 取自 `state.app_handle`）。这样 host 自己的原生 **Ctrl+V** 粘贴能拿到复制内容。
- 粘贴分工（用户确认）：**粘贴按钮** = 手机剪贴板 → 终端（bracketed paste，客户端 readText）；**Ctrl+V** = host 原生粘贴（由 host 自身剪贴板完成，复制已写入故可用）。

### 4. 选择模式 TUI 适配（`TerminalCanvas` touch 三函数）
- `selectionMode` 开启时**始终走文本选择**（忽略 `isMouseReporting`），使 TUI（含刷新后）也能选中→复制 pill。`selectionMode` 关闭时仍驱动鼠标/滚动（不破坏 TUI 交互）。

### 5. 主题键（host `cycle-theme` 无状态 + 客户端）
- host 新增 `Some("cycle-theme")`：读 `get_theme_data()`，按入参 `current`(id) 找下一主题，回包 `{type:"theme", id, themeType, colors}`。**不写盘、不 clobber**（遵循既有主题隔离原则）。
- connect 时的 theme 推送补 `id` 字段。
- `wsRemote`：`_lastTheme` 增 `id`；新增 `cycleTheme(currentId)`。
- `MainApp.handleThemeToggle` → `ws.cycleTheme(ws.lastTheme()?.id ?? '')`；回包经 onTheme 自动应用。

### 6. 底栏不溢出（`BottomTabBar.svelte` + `WorkspaceTree.svelte`）
- 图标按钮 `flex-shrink:0`（保持尺寸）；`tree-anchor`/`tree-trigger` 改 `flex-shrink:1; min-width:0`，label 截断；trigger `max-width` 响应式收窄。保证图标不压缩、trigger 截断、总宽恒 ≤ 视口。

### 7. 文件/Git/搜索（根因 + viewer）
- **根因修复（客户端，免 host 重建）**：`wsRemote._handleMessage` 守卫 `type` 为 `undefined` 时不调 `.endsWith`（`const type = (...).type ?? ''`），使无 type 的 data-request 回包正常落到 `messageListeners` → `WsDataProvider` 按 `_reqId` resolve。
- host 侧补 `obj.insert("type","data-result")`（与 `invoke-result` 一致，双保险）。
- **文件查看器**：`RemoteSidebar` 传 `onOpenFile` → 新增轻量只读 `FileViewer.svelte`（`provider.readFile` + 行号 + 等宽 + 搜索命中跳行）。非 Monaco（移动端轻量）。
- **差异查看器**：git 面板文件行点击 → 经新增 `git_diff_file` data-request 拉 unified diff → 轻量解析渲染（+/- 行着色）。
  - host：`dispatch_data_request` 增 `"git_diff_file"`（只读，复用 `commands::git::git_diff_file`）。
  - provider：`WsDataProvider.gitDiffFile`、`SidebarProvider.gitDiff`。
  - `SidebarGitPanel` 增 `onOpenDiff`。

### 8. 跨工作区终端浏览 + 标题刷新（host + 客户端）
- host 新增 `Some("list-workspace-panes")`（含 `workspaceId`）：`build_remote_pane_list(ws)` 取该工作区 panes，回包 `{type:"workspace-panes", workspaceId, panes}`。只读、不改 `active_ws_id`。
- `wsRemote`：`listWorkspacePanes(workspaceId)` 请求/响应。
- `WorkspaceTree`：展开**非活动**工作区 chevron → 拉该工作区 panes 缓存渲染；选择其中 pane → 切换工作区 + 聚焦；打开期间 `setInterval` 周期刷新（活动 + 已展开非活动）→ 标题实时。
- `MainApp.onMetadata`：pty-meta 到达时同步更新 `panes[]` 对应 pane 的 title（活动 pane 标题实时）。

## 影响 / 验证
- 客户端：`pnpm check`(svelte-check) + `pnpm build:remote` + `pnpm vitest run`。
- host：`cargo check -p ridge`（`cargo fmt --check` 单文件）。host 改动需用户 rebuild 才在手机生效；客户端根因修复（#7 type 守卫）免 host 重建即可让 文件/Git/搜索 工作。
- 运行时：`pnpm tauri:dev:cdp` + scripts/cdp-* 自助验证（勿杀正式会话）。

## 风险
- 主题/diff/跨工作区 panes 三处 host 改动需 rebuild（杀会话）；客户端改动独立可先生效。
- 选择模式改为"始终文本选择"偏离既有"TUI 转模拟鼠标"决策，但正是用户诉求（选择模式关闭时仍可模拟鼠标）。
