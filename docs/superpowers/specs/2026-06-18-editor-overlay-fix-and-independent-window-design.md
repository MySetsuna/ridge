# 文件编辑器：修复蒙层遮挡聚焦 + 新增「独立窗口」模式

日期：2026-06-18
状态：已批准，实施中

## 背景与问题

文件编辑器（`src/lib/components/FileEditor.svelte`，基于 Monaco 的 drawer/floating/embedded 面板）存在两个问题：

1. **编辑器无法聚焦——被蒙层遮挡。** Monaco 宿主区有两个层叠的 `absolute inset-0` 容器：常规 `mountPoint` 与画在其上的 keep-alive `diffMountPoint`。二者仅靠 `visibility:hidden` 切换显隐。由于 diff 编辑器是一个常驻 DOM、全尺寸的活动 Monaco 实例，其子树会重新参与命中测试（`visibility:hidden` 的子元素可被重新置为 `visible`），**吞掉本应落到常规编辑器的点击** → 用户所说的「diff 蒙层」。搜索命中高亮（`rg-search-flash-inline`）是行内文本装饰，本身不挡焦点，但搜索后会作为视觉「蒙层」长期残留 → 用户所说的「搜索标识蒙层」。

2. **无法把编辑器弹出为独立窗口。** 编辑器只能作为主窗口内的 drawer/floating/embedded 存在。

## 目标

- A：让编辑器可靠地获得焦点；搜索标识在离开搜索后不再残留。
- B：新增第 4 种形态——真正的独立操作系统窗口（Tauri `WebviewWindow`），承载整个编辑器（含所有标签页），其余行为保留；web-remote 下回退到 floating。

用户确认的决策：**真正的 OS 窗口** · **弹出整个编辑器含所有标签页** · **web-remote 回退 floating**。

## 设计

### Phase 1 — Bug 修复

- `FileEditor.svelte` Monaco 宿主：隐藏态的 `mountPoint` / `diffMountPoint` 样式追加 `pointer-events: none;`。`pointer-events:none` 对整棵隐藏子树禁用命中测试（子元素无法再 opt-in），且不影响 `automaticLayout`（它观察盒子尺寸，不变）。可见态保持空样式（默认 `auto`）。
- `SearchSidebar.svelte`：把 results→`setSearchHits` 的推送按 `active` 门控（`setSearchHits(active ? mapped : [])`）。离开搜索 tab 即清除高亮；回到搜索自动恢复（results 仍在内存）。门控既有推送以避免与单独 clear effect 竞争。

### Phase 2 — 独立窗口

**所有权转移模型**（避免双编辑器分叉）：弹出时把标签页「搬」到新窗口；关闭新窗口时反向交接回主窗口。任意时刻只有一个编辑器表面。

- **capabilities**（`src-tauri/capabilities/default.json`）：`windows: ["main"]` → `["main","editor"]`（让 editor 窗口继承编辑器所需的全部 core/插件权限）；新增 `core:webview:allow-create-webview-window`。需重建 Rust 生效。
- **协调器** `src/lib/stores/editorWindow.ts`（主窗口侧）：
  - `editorPoppedOut` writable。
  - `popOutEditor()`：非 Tauri / web-remote → `setDisplayMode('floating')` 返回；否则把打开文件（paths+active）写入共享 `localStorage['ridge-editor-window-handoff']`（同源多窗口共享 localStorage，无需 Rust 交接命令），经全局 Tauri（`window.__TAURI__.webviewWindow.WebviewWindow`，因 `withGlobalTauri:true`）创建 label=`editor`、`url:'index.html?win=editor'`、原生装饰的窗口；已存在则 focus + forward。随后清空主编辑器、置 `editorPoppedOut=true`。
  - `initEditorWindowHost()`（主窗口 onMount 调一次）：`listen('editor-window-closed')` → 反向交接重开文件、`editorPoppedOut=false`；注册 open 拦截器，弹出期间把新 open `emit('editor-window-open-file')` 转发到 editor 窗口。
- **store 钩子**（`fileEditor.ts`）：`setOpenInterceptor(fn)`；`openFile`/`openDiffTab` 先问拦截器，返回 true（已转发）则提前返回。`reopenFiles(paths, active)` 供反向交接。保持 store 与窗口无关。
- **FileEditor popout 渲染**：`popout = win=editor`；`containerStyle` 在 `coarsePointer` 分支之前加 `if (popout) return 'position: fixed; inset: 0; z-index: 0;'`；用 `!popout` 门控所有会写共享 prefs 的 chrome（折叠按钮、drawer/floating resizer、floating 关闭、根面板圆角描边、设置下拉的「显示模式」段）。新增设置项「独立窗口」（`!popout` 且桌面端显示）→ `popOutEditor()`。
- **新组件** `EditorWindow.svelte`：onMount 读+清 handoff、逐个 openFile、设 active；无条件渲染 `<FileEditor/>`（先于懒挂载门）；`listen('editor-window-open-file')`；`onCloseRequested` → `emit('editor-window-closed', {files, active})` 后放行。
- **布局分支**（`+layout.svelte`）：`win=editor` 时渲染 `<EditorWindow/>` 取代 `{@render children()}`（把 +page 的重型 app 初始化挡在 editor 窗口之外）；`startTotpIdentitySync` 用 `!editorWindow` 门控；`setTransport` 仍跑以保证 invoke 可用。
- **+page.svelte**：onMount 调 `initEditorWindowHost()`（仅主窗口）。

## 复用
- 窗口创建先例 `src-tauri/src/lib.rs:191`（`WebviewWindowBuilder` 建主窗口）；本设计从 JS 建窗，Rust 仅改 capabilities。
- URL flag boot 先例：`+layout.svelte`、`remote/App.svelte`、`cloudControllerBoot.ts`。
- 搜索命中链路已存在：`SearchSidebar` → `fileEditorStore.setSearchHits` → FileEditor 装饰 effect。

## 验证
1. `pnpm check` 0/0；`pnpm build` 绿。
2. 重建 Rust（capabilities 编译期烘焙）：`pnpm tauri:dev:cdp`（与安装版并存）。注意本机 WebView2 148 上 CDP 近期挂掉——若 `curl :9222/json/version` 失败则在 dev 窗口人工验证。
3. Bug：开过 diff tab 后仍能点入常规编辑器聚焦/编辑；全局搜索点结果后切走搜索 tab → 高亮清除；全程可聚焦。
4. 功能：开文件 → 设置菜单「独立窗口」→ 新 OS 窗口含同样标签；编辑 + Ctrl+S 落盘；主窗口资源管理器再开文件转发进该窗口；关窗口标签回到主面板；web-remote 浏览器会话下「独立窗口」表现为 floating（不崩）。

## 范围外 / 风险
- 不支持多 editor 窗口（单 `editor` label；再次弹出聚焦既有）。
- 不做跨窗口实时镜像（弹出转移所有权、关闭交还）以避免双编辑分叉。
- `core:webview:allow-create-webview-window` 是唯一安全相关改动，仅用于创建带标签的 editor 窗口。
