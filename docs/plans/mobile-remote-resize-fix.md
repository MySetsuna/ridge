# 移动端远程终端自适应全屏修复（mobile-remote-resize-fix）

## 问题（GM 现场复现）

移动端远程终端（mobile web-remote，SvelteKit / Svelte 5）在视口尺寸变化后**不会自动重排填满屏幕**：
运行中的 `claude` TUI 出现文字溢出 / 错位、状态行词中断行、画布大片空白，
**直到用户手动点击工具栏的「锁定渲染尺寸到本端并刷新」按钮**，画布才完美重排（满宽、盒线对齐）。
即手动刷新路径做了自动 resize 路径没做的事——这个差量就是 bug。

> 注意：本路径是 **移动端远程渲染器**（`src/remote/`），与桌面终端
> （`src/lib/terminal/manager.ts` / `src/lib/components/RidgePane.svelte`，另一 agent 负责）是**两套独立路径**。

## 根因：两条路径发往 host 的 WS 消息不同，host 对二者语义完全不同

### 手动按钮路径（已知正确）

1. `BottomTabBar` 的 `onRefresh` → `MainApp.handleRefresh()`（`src/remote/MainApp.svelte:44`）
2. `canvasRef.getDims()` → `ctrl.getDims()`（`src/remote/lib/terminalController.ts:336`），取当前 grid + 容器像素尺寸
3. `ws.refreshPane(...)` → 发送 `{type:'refresh-pane', ...}`（`src/remote/lib/wsRemote.ts:300`）

### 自动 resize 路径（出 bug）

1. 容器 `ResizeObserver`（`src/remote/lib/TerminalCanvas.svelte:79`）→ `ctrl.requestResize()`
2. `requestResize()`（`terminalController.ts:324`）→ `setTimeout(fitPane, 500ms)`（旧值，太慢）
3. `fitPane()`（`terminalController.ts:282`）重新计算 cols/rows（已正确地重读 `window.devicePixelRatio` + 读取 post-layout 容器 rect），
   调用 `kernel.resize()` 本地重排，并**仅当 cols/rows/dpr 真的变化时**触发
   `onResize?.(...)`（`terminalController.ts:312`）→ `MainApp.onResize`（`MainApp.svelte:40`）→ `ws.resizePane(...)`
   → 发送 `{type:'resize', ...}`（`wsRemote.ts:290`）

### host 端的关键差量（`src-tauri/src/remote/server.rs`，真相来源）

| 路径 | 发送的消息 | host 行为 |
|---|---|---|
| 手动按钮 | `refresh-pane` | `apply_pane_resize()`：**resize 真实 PTY + canonical parser**，重新解析当前 grid，广播 `pty-resized` 全量重绘给所有 viewer |
| 自动 resize | `resize` | **仅记录 fallback 尺寸**（`mobile_rows`/`mobile_cols`），**完全不碰 PTY、不发 `pty-resized`** |

证据（server.rs:1530-1543 `resize` 分支注释原文）：
> "a viewport-only resize doesn't touch the shared PTY or the client kernel. We just record the clamped size as the fallback used by the next claim/refresh."

证据（server.rs:1550-1561 `refresh-pane` / `claim-pane` 分支）：调用 `apply_pane_resize(...)`，
该函数（server.rs:902-955）`master.lock().resize(PtySize{...})` + `parser.lock().resize(rows, cols)` + 广播 `PtyResized`。

**结论**：自动路径发的 `resize` 在 host 端是 no-op。本地 wasm kernel 被 `fitPane` 的 `kernel.resize()` 改了网格，
但 host 仍按**旧 grid** 持续推送内容 → TUI 按旧列宽排版 → 错位/溢出/空白。
只有手动按钮的 `refresh-pane` 真正 reflow 了 host PTY 并触发 `pty-resized`
（client 经 `onPtyResize` → `resizeKernel` 收到，`MainApp.svelte:155-159`）。

### 附带发现

- host 早已实现 `claim-pane`，注释明确定位为「隐式『我刚交互/视口变了』的自动 claim」
  （server.rs:1545-1549，与 `refresh-pane` 共用 `apply_pane_resize` 完整重排路径），但**前端从未发送过 `claim-pane`** —— 之前是 host 侧死代码。
- 前端 remote 渲染器**没有** `window.resize` / `orientationchange` / `visualViewport`→refit 的接线，
  仅有容器 `ResizeObserver`；而真实设备 / CDP 模拟的视口变化（横竖屏、浏览器 chrome 收起、地址栏伸缩）
  改变的是**可见视口**，未必同步改变 flex 容器盒，`ResizeObserver` 可能不触发或滞后。

## 修复（仅前端，immutable 风格，Svelte 5 runes，错误不吞）

1. **`src/remote/lib/wsRemote.ts`** — 新增 `claimPane()`，镜像 `refreshPane` 但发 `{type:'claim-pane', ...}`，
   共用单调 `_refreshSeq` 让 host 可丢弃过期 claim。给旧 `resizePane()` 加 `@deprecated` 注释说明它只是 host 侧记账、不会 reflow（保留以维持协议完整性，已无前端调用方）。

2. **`src/remote/MainApp.svelte`** — `onResize` 从 `ws.resizePane(...)`（host no-op）改为 `ws.claimPane(...)`。
   该回调由 controller 仅在 grid 真正变化（cols/rows/dpr 差量）时触发，即「视口确实变了、需要 host 重排」的精确信号，不会刷屏。
   现在自动路径走与手动按钮**完全相同**的 host 重排路径 → 自动 自适应全屏。

3. **`src/remote/lib/terminalController.ts`** — `RESIZE_DEBOUNCE_MS` 由 `500` 降到 `100`（≤120ms，符合 brief 的「snappy 不回退到慢半拍」）。
   仍保留小 debounce 以把横竖屏切换 / 键盘弹出的突发信号合并成一次 `fitPane`/claim。

4. **`src/remote/lib/TerminalCanvas.svelte`** — 给现有 `visualViewport` 'resize' 监听补加 `ctrl?.requestResize()`
   （原来只调 `keyboardOffset`），并新增 `orientationchange` 监听同样触发 refit。
   `fitPane` 幂等（grid 未变则 `onResize`/claim 不触发），键盘弹出不改变 grid 时是廉价 no-op；
   `transform: translateY(...)` 不影响 `clientHeight`，故键盘弹出不会错误地缩小网格——无回归。

## 验证

- `pnpm build:remote`：**成功**（3790 modules transformed，21.74s，bundle 正常产出 `static/remote/`）。
- `pnpm check`：报 **1 处错误**，位于 `src/lib/components/SaveWorkspaceDialog.svelte:209`
  （`{@const}` 直接放在 `<span>` 下，非允许的父节点）。**此文件由另一 agent 的 i18n 重构改动，不在本次修复范围（本次只动 4 个文件）**，
  且与 resize 修复无关。`svelte-check` 汇总为 `1 ERRORS 1 FILES_WITH_PROBLEMS`，即本次改动的 4 个文件贡献 **0 个问题**。

## 改动文件清单

- `src/remote/lib/wsRemote.ts`（新增 `claimPane`，弃用 `resizePane`）
- `src/remote/MainApp.svelte`（`onResize` 改用 `claimPane`）
- `src/remote/lib/terminalController.ts`（debounce 500→100ms）
- `src/remote/lib/TerminalCanvas.svelte`（visualViewport + orientationchange 触发 refit）

## 待验证（需用户在真机 / CDP 重启 host 后端到端确认）

- 横竖屏旋转 / 浏览器 chrome 收起后，TUI 是否自动满宽对齐（不再需要手动点按钮）。
- 因 host 端 `claim-pane` 分支本身无需改动（已存在且正确），无需 rebuild ridge 后端；仅需用新 `static/remote/` bundle 重新加载移动端页面即可生效。
