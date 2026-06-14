# 公网远控四问题修复设计（重定向环 / 共享 PTY 尺寸 / host 刷新按钮 / cwd null 崩溃）

日期：2026-06-14
分支：develop（wind）+ ridge-cloud
关联记忆：[[perf_web_remote_split_compress]]、[[web_remote_desktop_in_browser]]、[[project_vscode_ide_capabilities]]、[[feedback_shared_tree_git_amend]]

## 背景

用户在「桌面浏览器公网远控」场景报四个问题（含一个中途补充的控制台报错）。本设计逐一给出根因与修复，
并明确 wind 侧与 ridge-cloud 侧的分工、需重新部署的范围。用户已拍板：① 两侧都改 + 我帮其完整部署云端；
②③ 采用「手动锁定 + 居中信箱」策略。

---

## ① 专属泛域名（租户子域）反复重定向

### 根因
混合重定向环：
- **服务端** `ridge-cloud/src/router.rs` `tenant_login_gate`：租户子域未带 `ridge_sso` cookie 的 HTML 导航 → 303 跳主域 `/?redirect=<子域URL>`（判据 = cookie **存在性**）。
- **客户端** `ridge-cloud/web/src/routes/+page.svelte:97-110`：带 `redirect` 时**只看 localStorage 里的旧 JWT**（`auth.getToken()`）判「已登录」，直接 `window.location.replace(redirect)` 回跳子域，**从不验证 `ridge_sso` cookie 是否真有效**。
- 于是「localStorage 有旧 token 但浏览器无有效 `ridge_sso` cookie」的存量用户：apex 判已登录回跳子域 → 子域 gate 判未登录跳回 apex → ……死循环。
- `ridge_sso` 由 `issue_session_and_cookie`（仅登录/验证邮箱时）种，30 天 refresh 会话；`/auth/session`（cookie 认证）用它换短 access。**无 Bearer→cookie 端点**。

三处登录态判据互不一致放大了环：gate 看 cookie 存在性 / apex 看 localStorage / 子域 boot 看 `/auth/session` 有效性。

### 修复（两侧都改）

**ridge-cloud 根治（需重新部署）**
1. `web/src/lib/api/client.ts`：`RequestOptions` 增加 `credentials?: RequestCredentials`，`request()` 透传；新增 `apiClient.session()` → `GET /auth/session`（`credentials:'include'`，复用 `AuthResult` 类型）。
2. `web/src/routes/+page.svelte` onMount：带 `redirect && isTenantRedirect(redirect)` 时，**先 `await apiClient.session()` 确认 `ridge_sso` cookie 真有效再 `replace(redirect)`**；失败（401/网络）→ `goto('/login?redirect=...')` 去重新种 cookie。打破「只信 localStorage 空跳」的环。

**wind 止血（纵深防御，不依赖云端）**
3. `src/routes/+layout.svelte` `startCloudControllerBootMode`：
   - `bootstrapFromCookie()` 的返回值（cookie 是否有效）要用上：`bootCloudControllerFromUrl` 在租户子域返回 null **只因缺 userToken/username**（即 cookie 无效）；host 离线是返回句柄后经 `onState('error')`，不会走回跳。
   - 回跳主域前加 `sessionStorage` 计数（`ridge_tenant_login_bounce`）：**第二次** boot 失败就**停在子域显式报错**（新 i18n `main.remoteGateErrTenantLoginStuck`：「登录态未生效，请回主域重新登录」），不再无限 `replace`。
   - `onState('connected')` 时清零该计数（连上即认为鉴权 + WebRTC 成功）。

---

## ②③ 共享单 PTY 尺寸：手动锁定 + 居中信箱

一个 PTY 只能有一个 grid 尺寸。host webview 与浏览器控制端各按自己 pane 尺寸发 `resize_pane`（桌面浏览器走
tauriShim 隧道 `invoke('resize_pane')`，**无移动端那套 seq 仲裁**），互相覆盖 → 浏览器本地 kernel 经 host 回发的
Resize delta 被改小，但 scissor 仍按容器（大）→ 内容只填左上小块 = 死区，刷新被 host 立即覆盖。

### 关键事实（已核对代码）
- `manager.fitPane` **不**直接 resize kernel；它调 `entry.resizeHandler`（=`RidgePane.onPtyResize`=`invoke('resize_pane')`），host resize PTY+PaneParser 后回发 Resize delta，本地 kernel 经 delta 更新 grid。
- host 模式渲染：scissor (`_recomputeViewport`) = `cols*cellW × rows*cellH` 锚定 content-box 左上，cols/rows 来自**容器**。

### 策略（用户选「手动锁定 + 居中信箱」）
仅对**浏览器控制端**（`WEB_REMOTE`）启用「共享远控模式」`sharedRemoteMode`，host 路径基本不变（仅加按钮门控）：

- **被动 fitPane 不再 claim PTY**：`sharedRemoteMode && !claim` 时 fitPane 只重算（居中）scissor 后返回，不发 `resize_pane`、不动 kernel。→ 浏览器不再与 host 抢尺寸。
- **scissor 跟随 kernel 实际 grid、居中、clamp 到容器**：`_recomputeViewport` 在 `sharedRemoteMode` 下 cols/rows 取 `kernel.cols()/rows()`（而非容器），并把 `cssX/cssY` 居中偏移；grid 超出容器则 clamp（裁切）。→ 内容居中、四周为终端背景，**死区变成有意的居中信箱**。
- **kernel grid 变化时重算 scissor**：rAF 预扫描里（仅 `sharedRemoteMode` 的 host pane）比对 `kernel.rows()/cols()` 与上次记录，变则 `_recomputeViewport`。→ host/浏览器任一方 claim 导致 PTY 尺寸变化、Resize delta 落地后，居中信箱即时跟随。
- **手动刷新 = claim**：新增 `manager.claimPaneSize(paneId)` → `fitPane(entry, /*claim*/true)`（走完整路径，按本视图容器尺寸发 `resize_pane`）。`RidgePane.refreshForRemote` 改调 `claimPaneSize`。在 host（非 sharedRemoteMode）`claim` 参数被忽略=正常 fit=重新锁定 host 尺寸。

### ② host 刷新按钮门控
- 新增全局 store `cloudHostOnline`（`src/lib/stores/remoteStatus.ts`），`RemotePanel` 的 `goOnline/goOffline/onHostState` 写入（online→true，offline/error→false）。
- `RidgePane` 按钮 gate：`{#if $remoteRunning || $cloudHostOnline || WEB_REMOTE}`。→ LAN 或公网远控任一开启，host 每个 pane 都显示「重新锁定尺寸」按钮。

### sharedRemoteMode 开关接线
- `setSharedRemoteMode(on)`：web-remote boot（`+layout.svelte` 或 manager 初始化，`WEB_REMOTE===true`）时置 true；翻转时对所有 pane 重算 viewport（翻 false 时正常 fitPane 重新填满）。
- host 端**不**进 sharedRemoteMode（保持窗口缩放即自适应的既有行为；被远端 claim 扰动后用按钮重新锁定）。

---

## ④ 控制台 `pane-cwd-changed` 在 null 上 `.replace` 刷屏

### 根因
`ridge-cloud`…不，是 wind `src-tauri/src/remote/server.rs:2262` 的 `RemotePtyEvent::Metadata` 分支**无条件**转发
`pane-cwd-changed`，但标题变更（`PaneTitleChanged`→`Metadata{title:Some, cwd:None}`，每次 prompt 重绘都发）
携带 `cwd:null` → 前端 `setPaneCwd(null)` → `normalizeCwd(null).replace(...)` 抛 `TypeError`（每帧刷屏）。

### 修复（两道防线）
1. `server.rs` Metadata 分支：仅 `cwd.is_some()` 才转发 `pane-cwd-changed`（标题仍经 `pty-meta` 走）。
2. 前端 `src/lib/stores/paneTree.ts`：`setPaneCwd` 顶部 `if (cwd == null) return;`（`collapseCwd`/`normalizeCwd` 已有/补 null 守卫），`setupPaneCwdListeners` 监听器同样防 `e.payload.cwd` 为 null。

---

## 验证与部署

**wind**：`pnpm svelte-check`（0/0）+ `cargo check`（server.rs/lib.rs）+ `pnpm test`（vitest，含 paneTree 既有用例）+ `pnpm build:desktop-web`。
**ridge-cloud**：`cargo check` + `web` build。
**部署（hard-to-reverse，用户已授权我帮其完整部署）**：ridge-cloud 重新构建 + Dokku 部署（apex 页面 + apiClient）；wind 侧 host 改动（server.rs）需本机 `tauri build` / 重建 ridge 才生效，web-remote 改动需 `build:desktop-web` 并同步到 ridge-cloud `desktop-app/` 后随云端部署。真机验证：租户子域不再循环、浏览器终端居中无死区、刷新可锁定、控制台无 cwd 报错。

**风险与隔离**：sharedRemoteMode 全程仅在 `WEB_REMOTE` 浏览器构建生效，正常桌面渲染路径零改动；server.rs/paneTree 的 cwd 守卫是纯防御。共享 tree 多会话并发，提交按 hunk 只暂存本设计相关改动（见 [[feedback_shared_tree_git_amend]]）。
