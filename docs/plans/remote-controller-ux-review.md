# 远程控制器可用性缺陷修复报告（前端）

> 范围：仅前端。未改动 Rust host 与 WS 协议（所需操作均已存在）。
> 另一名队友（term-resize）正在改 `src/lib/terminal/manager.ts` 与
> `src/lib/components/RidgePane.svelte`，本次修复刻意未触碰这两个文件。

## 背景架构（已在 LIVE 复测中确认）

- **桌面浏览器控制器** = 完整桌面 SPA（`src/routes/+page.svelte` +
  `src/routes/+layout.svelte`），经 UA 分叉用 LAN 远程服务器（HTTPS）下发。
  `@tauri-apps/api/*` 被别名到 `src/lib/transport/tauriShim/*`，所有 `invoke()`
  经 `bridge` 隧穿到 host。`isTauri()` 在 shim 里返回 `true`
  （`src/lib/transport/tauriShim/core.ts:32`），所以桌面 UI 里所有
  `if (!isTauri()) return;` 守卫都会走正常 Tauri 路径。
- **移动浏览器控制器** = 独立 App `src/remote/MainApp.svelte`
  （+ `TopBar.svelte`、`BottomTabBar.svelte`），WS 客户端
  `src/remote/lib/wsRemote.ts`。
- `wsRemote.ts` 已暴露 `createPane()`、`createWorkspace()`、`switchWorkspace()`、
  `listWorkspaces()`、`listPanes()`、`subscribePane()` 等；host 已实现对应处理
  （`src-tauri/src/remote/server.rs` 的 invoke 派发、`commands/workspace.rs`），
  即 host 可应远程请求创建 pane / workspace，缺的只是 UI 从未调用。

---

## 缺陷一：桌面 SPA 控制器停留在「请先选择一个工作区」

### 根因

- 「请先选择一个工作区」是 Explorer 侧栏在 `$activeWorkspaceId` 为空时的空状态
  文案（`src/routes/+page.svelte:1313`）。
- host 启动时**必然**持有一个全局活动工作区：`AppState::new` 创建首个工作区并把
  `active_workspace` 指向它（`src-tauri/src/state.rs:557-580`）；
  `get_active_workspace_id` 直接读全局 `active_workspace`
  （`src-tauri/src/commands/workspace.rs:39-41`），`list_workspaces` 也至少含一项。
  本机 LIVE 时原生桌面已开着 `C:/code/wind`，故 host 侧工作区状态完好。
- 桌面 SPA 启动会走 `refreshWorkspaces()`（`+page.svelte:1026` →
  `src/lib/stores/paneTree.ts:1296`），其中 `activeWorkspaceId.set(active)` 在
  paneTree.ts:1306。正常路径下该值应被填上。
- 因此缺陷本质是**前端启动期的竞态/异常导致 `activeWorkspaceId` 仍为空**：
  `refreshWorkspaces()` 链路中后续步骤（`get_pane_layout` /
  `setupPaneCwdListeners` / `refreshWorkspaceSaveInfo`）若在隧穿往返中抛错，
  或 `bridge.attach()` 里 fire-and-forget 的 `use-global-workspace` 通知与首批
  workspace 查询发生时序竞争，都会让 SPA 落在无活动工作区的状态而无法操作。

### 修复

采用**前端兜底**而非追查不稳定的竞态点（且不得改 host）：在启动工作区解析块之后
新增 `ensureActiveWorkspace()` 守卫——仅当当前无活动工作区（空串或全零 UUID）时介入，
优先切到 host 工作区列表里的第一个（即采用 host 当前工作区），列表为空才新建，
保证远程控制器连上后永远落在一个工作区上。正常路径零开销。

- `src/routes/+page.svelte`
  - 启动序列在 `loadSavedWorkspaces()` 之前插入 `await ensureActiveWorkspace();`
    （原 catch 块之后）。
  - 新增 `NIL_WORKSPACE_ID` 常量与 `ensureActiveWorkspace()` 函数（置于
    `renameActiveWorkspace` 之前）：读 `activeWorkspaceId`，非空且非全零则直接返回；
    否则 `switchWorkspace(list[0].id)` 或 `createWorkspace()`，随后
    `refreshWorkspaces()` 同步顶部下拉与活动 id；失败仅告警不抛。

### 桌面 SPA 新建终端能力核查

桌面 SPA 用桌面原有「新建根工作区 / 新建窗格」流程经隧穿 invoke 创建终端：
`splitPane`（paneTree.ts:1372）、`createWorkspace`（paneTree.ts:1330）守卫均为
`if (!isTauri()) return;`，而 shim 的 `isTauri()` 返回 `true`，故这些路径在 web-remote
下照常执行并隧穿到 host。检查 `+page.svelte` 中全部 `webRemote` 门控（1264 远程控制侧栏入口、
1322 RemotePanel、1587 原生窗口控制按钮）——**均不涉及工作区/窗格创建**。
故桌面 SPA 的建终端能力未被门控关闭，**无需 un-gate**。

---

## 缺陷二：移动远程端文案「在桌面端打开一个终端以开始」是死路

### 根因

- `src/remote/MainApp.svelte:171`（修改前）空状态写死「无活跃终端 /
  在桌面端打开一个终端以开始」，把建终端的唯一入口推回桌面端，远程端无法自助。
- 而 `ws.createPane()`（`src/remote/lib/wsRemote.ts:322`，host 回 `create-pane-result`）
  本就可用，UI 从未在空状态调用它。

### 修复

- `src/remote/MainApp.svelte`
  - 新增 `creatingPane` / `createError` 状态与 `handleCreatePane()`：调用
    `ws.createPane()` → 成功后置 `activePaneId` 为新 id 并 `ws.listPanes()`；
    返回空或抛错时把错误文案显示给用户（绝不静默吞掉）。
  - 空状态标记改为「无活跃终端」+ 可用的 **「新建终端」** 按钮（创建中显示「创建中…」，
    `disabled`），并在下方渲染 `createError`。新增 `.create-btn` / `.create-error` 样式。
- `src/remote/TopBar.svelte`
  - TopBar 的 `+` 按钮原本已接 `handleAddPane()` → `ws.createPane()`，但失败被静默吞掉。
    改为 `try/catch` 并新增 `addPaneError` 状态：失败时 `console.error` 并通过按钮 `title`
    与红色边框（`.add-pane-btn.err`）暴露错误，满足「不静默吞掉创建失败」要求。

---

## 推迟项

无。两处缺陷均为纯前端，所需 host/WS 能力已存在，未触发任何需要 rebuild host 的改动。

---

## 校验结果

- `pnpm check`：`0 ERRORS 0 WARNINGS`（4480 文件）。
- `pnpm build:remote`（移动远程 App）：`✓ built in 27.82s`，PWA precache 19 项生成，无错误。
- `pnpm build:desktop-web`（桌面控制器）：`✓ built in 2m 49s`，
  已写出站点至 `web-remote-dist`，无错误。

> GM 将通过重建 web-remote 包并重连浏览器控制器做 LIVE 复验。
