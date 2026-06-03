# S8 · shim 全量审计报告（R12）

> 横切·安全切片 S8 任务 B。对照上游计划 §5.5 / §5.6 / §7 R12（"复用整个桌面 SPA ⇒ shim
> 必须覆盖 SPA 触达的全部 `@tauri-apps/api` 调用点，否则远控模式运行时报错"）。
> 范围：只读 `src/`（桌面 SPA），对照 `src/lib/transport/tauriShim/*` 与 `vite.config.js`
> 的 WEB_REMOTE alias。**本报告不改代码。**
> 产出时间：2026-06-04。语言约定：散文简体中文，标识符英文。

---

## 0. 结论摘要（先看这里）

- SPA 触达的 `@tauri-apps/*` **模块说明符共 6 个**：`api/core`、`api/event`、`api/window`、
  `plugin-dialog`、`plugin-clipboard-manager`、`plugin-opener`。
- `vite.config.js` 的 WEB_REMOTE alias **覆盖前 5 个**（各有对应 shim 文件），逐一隧道/桩。
- **唯一未覆盖项 = `@tauri-apps/plugin-opener`**（3 处调用，均为动态 `import()`）。这是 R12
  风险面，但**不会让页面崩溃**：三处都在 `try/catch` 或 `__TAURI_INTERNALS__` 守卫内，最坏后果
  是"点外链/点终端链接无反应 + 控制台 warn"，**有一处还有可达的降级问题（见 §3.1 高危）**。
- `invoke`/`event`/`window` 三大主面**已全覆盖**；命令面经 `bridge` → LAN WS 隧道，事件经
  `listen` 注册 + host 推送，窗口面多为惰性桩（远控下窗口控件本就被 `webRemote` 标志隐藏）。
- 安全侧：shim 的 `isTauri()` **硬编码返回 `true`**（core.ts:32-34），这是大多数桌面 `if
  (!isTauri()) return;` 守卫在远控下走"正常 Tauri 路径"的关键——也正是 plugin-opener 那处高危
  降级失效的根因（§3.1）。

**给 GM 的处置建议**：plugin-opener 体量极小（仅 `openUrl`），**最干净的修法是新增第 6 个
shim `opener.ts` + alias**，把 `openUrl` 在远控下落为 `window.open(url, '_blank', 'noopener')`。
详见 §4。

---

## 1. 覆盖矩阵（按 API 面）

| `@tauri-apps` 模块 | shim 文件 | alias（vite.config.js）| 覆盖状态 | 调用点（运行时 SPA，排除 `*.test.ts`）|
|---|---|---|---|---|
| `api/core` | `tauriShim/core.ts` | ✓ L24 | **隧道**（invoke→bridge WS）+ 桩（Channel/transformCallback）+ 改写（convertFileSrc→`/file`）| 见 §2.1 |
| `api/event` | `tauriShim/event.ts` | ✓ L25 | **隧道**（listen→bridge 注册，host 推送）；emit/emitTo 桩（SPA 无 emit 站点）| 见 §2.2 |
| `api/window` | `tauriShim/window.ts` | ✓ L26 | **桩 / 局部实现**（onResized/isMaximized 用浏览器 API；min/close 惰性）；远控下窗口控件被 `webRemote` 隐藏 | 见 §2.3 |
| `plugin-dialog` | `tauriShim/dialog.ts` | ✓ L27 | **桩 + 局部隧道**（open 用 `window.prompt` 录主机绝对路径，seed 经 `get_current_project`）| 见 §2.4 |
| `plugin-clipboard-manager` | `tauriShim/clipboard.ts` | ✓ L28 | **隧道到浏览器**（Web Clipboard API，HTTPS 安全上下文）| 见 §2.5 |
| `plugin-opener` | — | **✗ 无 alias** | **未覆盖（R12）** —— 远控下 import 该模块会解析失败，落 catch / 守卫 | 见 §3 |

> 备注：grep 命中的 `src/routes/+layout.svelte:11`、`MarkdownPreview.svelte:11`、
> `DevIssueDialog.svelte:33`、`CloudProModal.svelte:101` 等是**注释/文案**里的 `@tauri-apps`
> 字样，非 import；已剔除。`*.test.ts`（paneTree/ptyBridge/fileExplorer/paneGitStatus/
> terminalHistory 等）只在 vitest 下跑、不进 web-remote 产物，不计入运行时覆盖面。

---

## 2. 已覆盖项明细

### 2.1 `@tauri-apps/api/core`（隧道，主命令面）

shim 导出 `invoke / isTauri / Channel / convertFileSrc / transformCallback`，与桌面导入面一致。

- `invoke` → `bridge.invoke()` 走 LAN WS 的 invoke-RPC（白名单见 server.rs `dispatch_invoke_request`
  / ridge-core `REMOTE_ALLOWLIST`）。两个特例就地改写：`register_pane_delta_channel` →
  `bridge.subscribePane`，`set_pane_delta_mode` → no-op（浏览器吃 raw-byte 不吃 postcard delta）。
- `isTauri()` → **硬编码 `true`**（让桌面的 `if (!isTauri()) return;` 守卫走正常 Tauri 路径，
  再由 shim 隧道）。真正无浏览器等价的面改用 `import.meta.env.RIDGE_WEB_REMOTE` 构建标志而非
  `isTauri()` 区分——**安全/正确性关键，见 §3.1**。
- `convertFileSrc` → 指向 host 鉴权端点 `/file?path=…&token=…`（server.rs:266 `file_handler` 实存，已核实）。
- `Channel` / `transformCallback` → 纯桩（仅为 `new Channel()` 不炸）。

运行时调用点（静态 import，全部经 alias 命中 shim）：
`+page.svelte:174`、`fileEditor.ts:10-11`、`RemotePanel.svelte:3`（远控下该面板被 `!webRemote`
门控、不渲染）、`fileExplorer.ts:3`、`fsEvents.ts:14`、`manager.ts:36`、`fileWatcherSync.ts:21`、
`paneGitStatus.ts:21`、`NativeSessionsPanel.svelte:3`、`ptyBridge.ts:29`、`paneTree.ts:2`、
`CloudPanel.svelte:10`（远控下 Cloud 面板同样在 remote tab 内、被门控）、`linkResolver.ts:11`、
`remoteStatus.ts:16`、`project.ts:8`、`MarkdownPreview.svelte:24`、`settings.ts:2`、
`FileTree.svelte:14`、`SourceControl.svelte:3`、`themes.ts:2`、`terminalHistory.ts:2`、
`SplitContainer.svelte:12`、`SaveWorkspaceDialog.svelte:3`、`PaneShellSwitcher.svelte:2`、
`FileEditor.svelte:31`、`transport/tauri.ts:1`（注：远控走 `WsDataProvider`，非 `TauriDataProvider`，
此 import 在远控下不应被执行路径触达，但模块仍会被 alias 解析为 shim invoke——安全）、
`SettingsPanel.svelte:7,335`、`RidgePane.svelte:17`、`Explorer.svelte:19`、`PaneGitPill.svelte:12`、
`SearchSidebar.svelte:18`。

### 2.2 `@tauri-apps/api/event`（隧道）

`listen()` → `bridge.listen()` 注册兴趣，host 经 WS 推送匹配事件（server.rs 事件 tap）。
`once()` 包 listen + 自注销。`emit/emitTo` 为 no-op 桩（SPA 无 emit 站点，已 grep 确认）。
调用点：`+page.svelte:175`、`fsEvents.ts:13`、`ptyBridge.ts:30`、`paneTree.ts:3`、
`SourceControl.svelte:4`。

### 2.3 `@tauri-apps/api/window`（桩 / 局部实现）

`ShimWindow`：`onResized` 用 `window.resize` 事件忠实实现、`isMaximized/isFullscreen` 读
`document.fullscreenElement`、`maximize/unmaximize` 走 Fullscreen API；`minimize/close/setTitle`
惰性。远控下原生标题栏/窗口控件由 `+page.svelte` 的 `webRemote` 标志隐藏（L1557 `class:hidden`）。
调用点：`+page.svelte:176`（`getCurrentWindow`）。**覆盖充分。**

### 2.4 `@tauri-apps/plugin-dialog`（桩 + 局部隧道）

`open()`：浏览器无法弹主机原生目录选择器，且选中的路径必须是**主机**文件系统路径——v1 用
`window.prompt` 录入主机绝对路径，seed 经 `bridge.invoke('get_current_project')`。`save/message/
confirm/ask` 映射到 `window.prompt/alert/confirm`。
调用点：`+page.svelte:48`、`SaveWorkspaceDialog.svelte:4`、`SettingsPanel.svelte:8`。
**可用但体验降级**（prompt 录路径）；shim 注释已标"server-driven 目录浏览器（复用 host
`browse_directory` 命令）是后续项"——非 R12 崩溃风险，记为体验改进 backlog。

### 2.5 `@tauri-apps/plugin-clipboard-manager`（隧道到浏览器）

`writeText/readText` → Web Clipboard API（远控经 HTTPS 服务，是 secure context，`navigator.clipboard`
可用）。`readText` 可能因缺用户手势/权限被拒，调用方已处理 clipboard 失败。
调用点：`FileTree.svelte:15`、`RidgePane.svelte:18`、`Explorer.svelte:34`。**覆盖充分。**

---

## 3. 未覆盖项（R12 风险）：`@tauri-apps/plugin-opener`

**无 alias、无 shim。** 该模块仅被用来调 `openUrl(url)`（在 OS 默认浏览器里打开外链 / 终端识别的
URL）。远控构建下，对它的 `import('@tauri-apps/plugin-opener')` **没有重定向**：

- 若运行时 `@tauri-apps/plugin-opener` 包仍在 `node_modules` 且被打进产物，`openUrl` 会调用
  Tauri IPC，而浏览器里没有 `__TAURI_INTERNALS__` → 抛错；
- 若按 web-remote 产物裁剪掉了该包，动态 import 直接 reject。

两种情况都落到调用点的 `catch` / 守卫，**不会让 SPA 崩溃**，但功能静默失效。3 处调用点：

### 3.1 【高危·可达降级失效】`linkResolver.ts:208-219` `openShell()`

```ts
async function openShell(href: string): Promise<void> {
  if (!isTauri()) {                                  // ← 远控下 isTauri()===true，不进此分支
    window.open(href, '_blank', 'noopener,noreferrer');
    return;
  }
  try {
    const { openUrl } = await import('@tauri-apps/plugin-opener'); // ← 远控下解析/调用失败
    await openUrl(href);
  } catch (err) {
    console.warn('[linkResolver] openUrl failed', href, err);      // ← 仅 warn，无 window.open 兜底
  }
}
```

**问题**：shim 的 `isTauri()` 恒为 `true`（§2.1），所以远控下**跳过了 `window.open` 兜底**，直接走
import-opener 路径并失败 → 用户在远控浏览器里点链接（Markdown 外链、`linkResolver` 路由的外部 URL）
**无任何反应**，只在控制台 warn。这是本审计**唯一会被用户实际感知的功能回归**。

### 3.2【低危·自带兜底】`CloudProModal.svelte:100-105` `openExternal()`

```ts
import('@tauri-apps/plugin-opener')
  .then((m) => m.openUrl(url))
  .catch(() => { window.open(url, '_blank', 'noopener'); });   // ← catch 里有 window.open 兜底
```

catch 内已 `window.open` 兜底，远控下能正常打开外链。**且 CloudProModal 属 Cloud 升级流，远控下
入口（remote/cloud 面板）本就被 `!webRemote` 门控**，触达概率低。

### 3.3【低危·守卫拦截】`manager.ts:1374-1378`（终端链接点击）

```ts
if (typeof window !== 'undefined' && window.__TAURI_INTERNALS__) {  // ← 浏览器里为 false
  void import('@tauri-apps/plugin-opener').then(({ openUrl }) => openUrl(uri)) ...
}
```

守卫显式检查 `__TAURI_INTERNALS__`（浏览器里不存在），远控下**根本不进 import 分支**，终端里点 URL
不会打开 OS 浏览器（静默无操作，无报错）。属可接受降级，但与 3.1 体验不一致。

---

## 4. 建议处置（交 GM 拍板）

按成本/收益排序：

1. **【推荐】新增 `tauriShim/opener.ts` + alias**（最干净、根除 R12 未覆盖项）
   - 新文件导出 `export async function openUrl(url: string): Promise<void> { window.open(url, '_blank', 'noopener'); }`
     （并按需补 `openPath` 等 SPA 实际用到的导出——目前只用到 `openUrl`）。
   - `vite.config.js` 增 `webRemoteAliases['@tauri-apps/plugin-opener'] = shim('opener.ts');`。
   - 效果：3.1/3.2/3.3 三处统一在远控下用浏览器新标签打开外链，体验一致，且消除"动态 import
     解析失败"这一 R12 隐患。**S8 自身可落，不依赖 S4/S5。**

2. **【次选/可并行】修 3.1 的降级逻辑**（即使不加 opener shim 也应修）
   - `openShell` 的 `if (!isTauri())` 判据在远控语境下失真（shim isTauri 恒 true）。应改用
     `import.meta.env.RIDGE_WEB_REMOTE` 区分，或把 `catch` 里补 `window.open` 兜底，使远控点外链
     不再静默失败。**注意**：此项触碰 `src/lib/utils/linkResolver.ts`（业务文件），是否纳入 S8
     还是留给后续 owner，请 GM 定（本审计仅报告，未改）。

3. **【backlog·非 R12】dialog 体验升级**（§2.4）：把 `window.prompt` 录路径换成 host
   `browse_directory` 驱动的服务端目录浏览器。非崩溃风险，排期级。

---

## 5. 审计方法与可复现命令

- 模块面枚举：`rg -o "@tauri-apps/[\w/-]+" src`（剔除 `*.test.ts` 与注释命中）。
- 反向验证无遗漏 api/plugin 子模块：`rg "@tauri-apps/(api/(?!core|event|window)\w+|plugin-(?!dialog|clipboard-manager|opener)[\w-]+)" src` → 0 命中。
- 全局对象旁路：`rg "__TAURI__|__TAURI_INTERNALS__" src` → 仅 manager.ts 守卫 + DevIssueDialog 注释。
- alias 覆盖面：`vite.config.js` L23-29（5 条）。
- host 端点核实：`/file`（server.rs:266 `file_handler`）支撑 `convertFileSrc`。

**审计判定**：除 `plugin-opener` 外，SPA 全部 `@tauri-apps` 调用面均被 shim 覆盖；`plugin-opener`
未覆盖但不致崩溃，最严重表现为 §3.1 远控外链点击静默失效。R12 收口建议见 §4.1。
