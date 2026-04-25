# Wind — Next Loop 计划

最后更新：2026-04-25（第 68 轮） · 由 /loop 自动生成

> 本文档由 /loop 循环结束时写入，下一轮 `/loop` 会优先读取本文档。
> 对标：VS Code、JetBrains Fleet、Warp、Zed。

---

## ✅ 历史轮次已完成

1–21 轮：三大主诉求 + Explorer 完整体验 + SCM + 搜索 sidebar + pane git
pill 含 picker / 内联创建分支 + 插件三 scope + Claude teammate 闭环 +
测试矩阵 + 终端 scrollback Phase 1+2+3（block 模型 + tail replay + 历史浏览
modal 含搜索栏 / 自动上滚 / 另存 .log / `stripAnsi` 11 个单测）+ cargo
0 warnings + CLAUDE.md 同步。

### 第 47 轮（2026-04-25 13:55）— P0-I bare alert/confirm 全迁移 + 搜索 gitignore 修复（后台）

#### P0-I — 全量 bare alert/confirm → WindDialog

**9 个文件，共 ~20 处调用全部清零**

| 文件 | 变更 |
|---|---|
| `ClaudeAgentLauncher.svelte` | +import alertDialog；1 alert → alertDialog |
| `Explorer.svelte` | 1 alert → alertDialog（import 已在） |
| `FileTree.svelte` | 5 alerts → alertDialog（import 已在） |
| `PaneGitPill.svelte` | +import alertDialog；2 alerts → alertDialog |
| `ScrollbackHistoryModal.svelte` | +import alertDialog；1 alert → alertDialog |
| `SearchModal.svelte` | +import alertDialog；1 alert → alertDialog（非 danger） |
| `SearchSidebar.svelte` | +import alertDialog/confirmDialog；1 confirm + 1 alert → 各自 themed |
| `SourceControl.svelte` | 4 alerts + 1 confirm + commit msg empty alert；`discard()` confirm → async confirmDialog；import 已在 |
| `SplitContainer.svelte` | +import alertDialog；2 alerts → alertDialog |

所有函数原已 async，无需新增 async。验证：`grep -c "alert(\|confirm(" 6 个目标文件 → 全部 0`。
`pnpm check` **0 / 0 / 0**（3893 文件）。

#### 搜索 gitignore 修复（已交付）

用户报告搜索会卡死——根因：`search.rs` 用 `walkdir::WalkDir` 扫文件，不尊重
`.gitignore`，大仓库的 `node_modules/` / `target/` / `dist/` 等导致扫描百万文件。

修复：`ignore` crate（已在 `Cargo.toml`，版本 0.4）的 `WalkBuilder`  
= ripgrep 同款引擎，默认尊重 `.gitignore`、`.git/info/exclude`、全局 gitignore。

改动要点（`src-tauri/src/fs/search.rs`）：
- `text_search` 和 `search_files` 两处 walk loop 替换为 `WalkBuilder::new(root)`；
- `hidden(!options.include_hidden)` — `options.include_hidden` 已有前端开关对应；
- `git_ignore(true)` + `git_global(true)` + `git_exclude(true)` + `ignore(true)` + `require_git(false)`;
- 保留 `FileTree::should_ignore` 兜底静态 SKIP_DIRS；
- `search_files` 保留 `.max_depth(Some(10))` 限制。

背景 agent 完成后 `cargo check` 需为 0 warnings。

---

### 第 68 轮（2026-04-25）— 终端内搜索（Ctrl+F）+ P3 计划整理

#### 终端内搜索（`xterm-addon-search@0.13.0`）

**`src/lib/components/Pane.svelte`**

新增 Ctrl+F 触发的搜索栏，悬浮在终端容器右上角：

- **新增 import**: `SearchAddon` from `xterm-addon-search`
- **状态**: `termSearchOpen`, `termSearchQuery`, `termSearchCase`, `searchInputEl` (bind ref)
- **Addon 加载**: `searchAddon = new SearchAddon(); term.loadAddon(searchAddon)` (在 terminal init 后)
- **键盘处理**:
  - `Ctrl+F` → `termSearchOpen = true`（在 `attachCustomKeyEventHandler` 里）
  - `Esc` 关闭搜索 + 清空 query + 回焦终端
  - `Enter` → `findNext`，`Shift+Enter` → `findPrevious`
- **实时搜索**: `$effect(() => searchAddon.findNext(termSearchQuery, { incremental: true }))` — 打字即搜
- **UI**: 搜索框 + Aa（大小写敏感切换）+ ↑↓（前后导航）+ × 关闭；z-150 在滚动底部按钮之上

#### P3 计划整理

- P3-9（ScrollbackHistoryModal 复制 toast）/ P3-10（PaneGitPill 操作 toast）
  均已在第 51 轮交付，补充 ✓ 标注

**回归**：`pnpm check` 0/0/0 · `vitest` 142/142

---

### 第 67 轮（2026-04-25）— 终端 Web Links + 字体大小控制

#### 可点击 Web Links（`xterm-addon-web-links`）

**`src/lib/components/Pane.svelte`**

新增 `WebLinksAddon`（`xterm-addon-web-links@0.9.0`，已 `pnpm add` 安装）：
- 终端输出中的 URL 自动识别并高亮（下划线 + Ctrl+点击可打开）
- 点击回调调用 Tauri `opener` 插件的 `openUrl(uri)`（已有 `tauri-plugin-opener = "2"` 依赖）
- 非 Tauri 环境（dev browser）回退到 `window.open`

#### 终端字体大小控制（Ctrl+= / Ctrl+- / Ctrl+0）

**`src/lib/stores/termSettings.ts`**（新文件）

模块级 `termFontSize` store（writable）：
- 初始值从 `localStorage['wind-term-font-size']` 读取（默认 15）
- 有效范围：8–32 px
- `increase()` / `decrease()` / `reset()` 每次调用后同步持久化到 localStorage

**`src/lib/components/Pane.svelte`**

- 终端创建时从 store 读取当前 fontSize（`currentFontSize`）
- 加载 `WebLinksAddon` + 订阅 `termFontSize` store：store 变化时 `term.options.fontSize = size; fitAddon.fit()`
- 自定义键盘处理器新增三条快捷键：
  - `Ctrl+= / Ctrl++` → `termFontSize.increase()`
  - `Ctrl+-` → `termFontSize.decrease()`
  - `Ctrl+0` → `termFontSize.reset()`
- `onDestroy` 取消 store 订阅

所有 pane 实例共享同一 store，Ctrl+= 在任意 pane 操作后所有 pane 同步更新 + 持久化。

**回归**：`pnpm check` 0/0/0 · `vitest` 142/142 · `cargo check` 0 warnings

---

### 第 66 轮（2026-04-25）— SearchSidebar content-visibility + 计划整理

#### SearchSidebar 结果行 `content-visibility: auto`

**`src/app.css`** + **`src/lib/components/SearchSidebar.svelte`**

新增 CSS class `wf-search-row`（应用在每条结果 `<button>` 上）：
```css
.wf-search-row { content-visibility: auto; contain-intrinsic-size: 0 22px; }
```

作用：浏览器跳过不在视口内的行的 layout + paint，大幅降低 "显示全部 500+ 条结果"
时的首次渲染开销。注意事项已记录在 CSS 注释和 NEXT_LOOP_PLAN 里：
**不能**对 `.search-file` 容器用此属性，否则 layout containment 会破坏组内
`position:sticky` 文件标题。

#### 计划整理

- P1 部分修复了重复 "3." 编号，标注 φ（CRLF front-matter）/ item 4（5分钟刷新）
  / item 5（base ref combobox）均已交付
- P3-12（MarkdownPreview choiceDialog）/ P3-13（linkTrust per-basePath）
  均已在早期轮次交付，补充 ✓ 标注

**回归**：`pnpm check` 0/0/0 · `vitest` 142/142

---

### 第 65 轮（2026-04-25）— PaneGitPill 基分支 combobox + CLAUDE.md 同步

#### PaneGitPill 基分支 `<select>` → `<datalist>` combobox

**`src/lib/components/PaneGitPill.svelte`**

旧行为：创建新分支的"基于："行使用 `<select>` 元素，在 monorepo 的数百条分支下
下拉列表极长且不能过滤。

新行为：改为 `<input type="text" list="wf-git-base-list">` + `<datalist>` 组合：
- 用户可以直接**输入**任意 ref（分支名、tag、commit hash）
- 已加载的 `branches` 列表作为候选项出现在浏览器原生建议下拉
- placeholder `HEAD（当前）` 表明空值即用当前 HEAD
- `autocomplete="off"` 避免浏览器历史干扰
- 由于同一时间只有一个 picker 打开，共用 `id="wf-git-base-list"` 无冲突

#### CLAUDE.md 同步（三节更新）

1. **Sidebar / Explorer conventions** — 新增"Horizontal tab scrolling"条目，说明
   `applyContentLayout()` 在 `.os-content` 上注入 flex-row 的必要性及调试方法。

2. **SCM git watcher** — 追加"SCM refresh policy"段，记录 round 64 移除 periodic
   timer + workspace-switch subscriber 后的刷新策略（两条路径：cwd 变化 + watcher）。

3. **Claude Code Agent Teams** — 补充 `kill-pane`、`rename-window`、
   `display-message` 完整变量列表、`list-panes?json=1` 新 `cwd` 字段。

**回归**：`pnpm check` 0/0/0 · `vitest` 142/142

---

### 第 64 轮（2026-04-25）— 横向 Tab 滚动修复 + SCM 刷新降频

#### 横向 Tab 滚动修复（`overlayScroll.ts`）

**根因**：`OverlayScrollbars(node)` 将 host 的子元素包裹进
`.os-viewport > .os-content`，任何加在 HOST 上的 flex 布局对
`.os-content` 内的 Tab 子元素完全无效，导致 Tab 竖向堆叠而非横向排列。

**修法**（`src/lib/actions/overlayScroll.ts`）：

新增 `applyContentLayout(node, params)` 函数：
- 仅在 `horizontal-tabs` preset 时运行
- 在 `OverlayScrollbars(node, ...)` 初始化**之后**（`.os-content` 已存在）
  调用 `node.querySelector('.os-content')` 取内容容器
- 注入 `display:flex; flex-direction:row; align-items:center; gap:4px;
  white-space:nowrap; min-width:max-content`
- `layout: false` 仅阻止 HOST 的额外 flex 注入，不影响内容容器
- `update()` 钩子同步更新内容容器布局

**效果**：
- `WorkspaceTabs.svelte`：工作区 tab 现在横向排成一行，支持横向滚动
- `FileEditor.svelte`：文件编辑器 tab 同样横向排列（即便传了 `layout: false`）

#### SCM 刷新降频（`SourceControl.svelte`）

**根因**：`onMount` 里有两条 subscribe 触发 discover：
1. `paneCwdStore.subscribe` → 280ms debounce（用户期望的行为）
2. `activeWorkspaceId.subscribe` → 0ms delay，每次切换工作区立即强制 discover

加上 fresh-cache 时的 1s 后台刷新，每次切换 tab 或工作区都会追加多次
`discoverRepos` + `loadGraph` IPC 调用。

**修法**：
- 移除 `activeWorkspaceId` subscriber（第 2 条触发路径）
  → 工作区切换后等 pane 的 OSC-7 上报 cwd 再触发 discoverRepos，
    不再主动轮询
- 移除 fresh-cache 时的 1s background refresh
  → SCM 的两个主动刷新路径现在是：（a）cwd 变化 + （b）文件系统 watcher
- 新增注释说明保留路径

**回归**：`pnpm check` 0/0/0 · `vitest` 142/142

---

### 第 63 轮（2026-04-25）— tmux 模板变量扩展 + list-panes cwd 字段

#### tmux_replacements 扩充（静态变量）

**`src-tauri/src/bin/tmux.rs`** `tmux_replacements()`

新增 16 个常用 `#{...}` 占位符，补全 Claude Code TmuxBackend 常用的查询字段：

| 变量 | 值 | 说明 |
|---|---|---|
| `#{pane_pid}` | `1` | PTY 进程 ID（静态占位） |
| `#{pane_title}` | `wind` | Pane 标题 |
| `#{pane_current_command}` | `shell` | 当前运行命令 |
| `#{pane_width}` | `120` | 宽度（与 list-panes 默认一致） |
| `#{pane_height}` | `80` | 高度 |
| `#{pane_left/top/right/bottom}` | `0/0/119/79` | 边界坐标 |
| `#{window_layout}` | `tiled` | 布局名 |
| `#{window_width/height}` | `120/80` | 窗口尺寸 |
| `#{session_windows}` | `1` | 会话窗口数 |
| `#{client_session}` | `wind` | 当前 session 名 |
| `#{client_width/height}` | `120/80` | 客户端尺寸 |
| `#{client_tty}` | `/dev/pts/0` | 客户端 TTY |

#### tmux 动态变量查询（`render_tmux_format_dynamic`）

新增 `render_tmux_format_dynamic(fmt, pane_index, url, token)`：
- 先做静态替换（`render_tmux_format`）
- 若结果仍含 `#{window_panes}` 或 `#{pane_current_path}`，发一次 HTTP 请求
  `GET /api/v1/list-panes?json=1` 拿 JSON（`pane_count` + 每 pane 的 `cwd`）
- 用真实值替换；后端不可达时静默降级（已替换的静态部分仍正确输出）

`cmd_display_message` 和 `cmd_list_panes -F` 均切换到 `render_tmux_format_dynamic`，
由此支持 `tmux display-message -p '#{pane_current_path}'` 和 `tmux list-panes -F '#{window_panes}'`。

#### list-panes JSON 增加 cwd 字段

**`src-tauri/src/teammate/server.rs`** `PaneRowJson`

- 新增 `cwd: Option<String>` 字段（`#[serde(skip_serializing_if = "Option::is_none")]`）
- `route_list_panes` 中从 `ws.pane_tree.panes[uuid].cwd` 填充，正斜杠归一化
- 这是 `render_tmux_format_dynamic` 拉取 `#{pane_current_path}` 的数据源

**回归**：`cargo check` 0 warnings · `cargo test` 68/68 · `vitest` 142/142 · `pnpm check` 0/0/0

---

### 第 62 轮（2026-04-25）— δ PARTIAL缺口：kill-pane 真实路由 + rename-window 新路由

#### `cmd_kill_pane` → POST `/api/v1/kill-pane`

**`src-tauri/src/bin/tmux.rs`** `cmd_kill_pane`

旧行为：`kill-pane` 解析 `-t` 参数后直接返回 `Ok(())`（no-op）。Claude Code 退出 teammate
后，对应 pane 继续以 zombie 形式留在 Wind 布局中，用户需手动关闭。

新行为：解析 `-t` pane_index 后 POST 到 `/api/v1/kill-pane { pane_index }`，
Wind 后端移除 pane、tear down PTY、emit `teammate-layout-changed`。
`-a`（kill all）特判为 no-op（保留至少一个 pane 的安全策略）。

#### `/api/v1/rename-pane` 新路由

**`src-tauri/src/teammate/server.rs`**

新增 `route_rename_pane` handler：
- 接收 `{ pane_index?: usize, name: string }`
- 若无 `pane_index`，默认用 `teammate_tmux_pane_cursor`
- 写入 `teammate_pane_titles[pane_uuid] = name`（`name` 为空时删除）
- emit `"teammate-layout-changed"` 事件触发前端标题刷新
- 路由：`POST /api/v1/rename-pane`

#### `cmd_rename_window` → POST `/api/v1/rename-pane`

**`src-tauri/src/bin/tmux.rs`** `cmd_rename_window`

旧行为：`rename-window` 解析参数后 no-op。Claude Code 调用 `tmux rename-window -t 0 <name>`
给 teammate pane 起名，但 Wind pane 标题从不更新。

新行为：解析 `-t pane_index` 和 `name`，POST 到 `/api/v1/rename-pane { pane_index, name }`。
Wind 后端写入 `teammate_pane_titles` 并 emit 布局变更事件，pane 标题栏立即显示新名字。

**回归**：`cargo check` 0 warnings · `cargo test` 68/68 · `vitest` 142/142 · `pnpm check` 0/0/0

---

### 第 61 轮（2026-04-25）— SearchSidebar 并行搜索 + CLAUDE.md 同步

#### SearchSidebar 真正并行（第 29 轮 MEDIUM 全收尾）

**`src/lib/components/SearchSidebar.svelte`**

旧行为：诊断（第 58 轮修）是即时的，但 per-root 的 `text_search` 仍是串行 `for` 循环——
N 个工作区 root 里每个搜索都要等上一个完成，总延迟线性增长。

新行为：`Promise.allSettled(roots.map(root => invoke('text_search', { root, ... })))` —
所有 root 的 IPC 并发发出，Tauri 命令层和 Rust 后端并发处理（每个 `text_search` 无共享状态）。
单根场景零开销；多根场景（多工作区、多 cwd 终端）延迟从 Σt_i 缩减为 max(t_i)。

#### CLAUDE.md 同步（三节新增）

- **CWD 路径归一化**：明确记录 `normalize_cwd_str` (Rust) / `normalizeCwd` (TS) 约定，
  以及"不经过归一化直接透传 `PathBuf::to_string_lossy()` 会导致 Explorer 列重复"的陷阱。
- **WindDialog API 表**：四个函数 + 返回类型 + 适用场景；禁止 `window.alert/confirm/prompt`。
- **SCM git watcher**：记录 worktree `.git`-is-file 处理、debounce 层级、
  `start_watching_repos` 命令的调用入口。

**回归**：`pnpm check` 0/0/0 · `vitest` 142/142

---

### 第 60 轮（2026-04-25）— P3-11 linkTrust "仅本次" + WindDialog choiceDialog + TODO 清理

#### P3-11 — 外部链接信任三档选择（始终允许 / 仅本次 / 取消）

**`src/lib/components/WindDialog.svelte`**

新增 `choiceDialog(opts)` API：
- 新 DialogKind `'choice'`，新接口字段 `secondaryLabel?: string`，新导出类型
  `ChoiceResult = 'primary' | 'secondary' | 'cancel'`。
- `onCancel` → `'cancel'`；`onOk` → `'primary'`；新 `onSecondary` → `'secondary'`。
- 模板：`{#if dialog.kind === 'choice' && dialog.opts.secondaryLabel}` 中间按钮（中性
  样式：`border-[wf-border]`，区别于主色 OK），渲染在 Cancel 和 OK 之间。
- 不破坏现有 `alertDialog`、`confirmDialog`、`promptDialog` 的调用方和类型。

**`src/lib/components/MarkdownPreview.svelte`**

`openExternal` 改用 `choiceDialog`：
- "始终允许（本次会话）" → `'primary'` → 调用 `trustHostFromUrl` + 打开链接
- "仅本次" → `'secondary'` → 只打开链接，不写入信任集合
- "取消" → `'cancel'` → 什么都不做

用户体验对齐 VS Code：点击 markdown 里不信任的链接时有三个明确意图档，而不是
"被迫 always-trust 才能打开任何外链"。

#### `paneTree.ts` 注释清理

`SavedWorkspace.paneCwds` 的 TODO 注释被修正：cwd 持久化通过 backend
`PaneTree.panes[id].cwd` → `.wind` JSON → `get_pane_layout` → `extractCwdsFromLayout`
自然工作，原注释误认为"没有持久化"。history-path (`list_workspace_history`) 与 .wind
file-path 是两条独立路径；前者目前不暴露还原 UI，后者工作正常。

**回归**：`pnpm check` 0/0/0 · `vitest` 142/142

---

### 第 59 轮（2026-04-25）— git worktree 监视器修复 + 后端 cargo check 通过

#### ε阶段二 worktree 边界：`.git`-is-file 场景修复

**`src-tauri/src/commands/watch.rs`** `GitWatcher::watch()`

旧行为：`git_dir.exists()` 只检查 `.git` 是否存在，但在 git linked worktree 里
`.git` 是**文件**而非目录，包含 `gitdir: <real-git-dir>` 指针。
`notify` 对文件递归 watch 不会跟进到实际 git 目录，SCM 自动刷新对 worktree 失效。

修法：
```rust
let git_dir_to_watch = if git_dot.is_dir() {
    Some(git_dot)                         // 普通 repo
} else if git_dot.is_file() {
    // 解析 "gitdir: <path>" 行，支持相对/绝对路径
    read_to_string(git_dot) → parse "gitdir: ..." → PathBuf → filter(is_dir)
} else { None };
```

效果：worktree 里 `.git/worktrees/<name>/` 目录被正确监视，HEAD/index/refs
变更同样触发 `scm-repo-changed` 事件，SCM 面板实时刷新。

**回归**：`cargo check` 0 warnings · `cargo test` 68/68 · `vitest` 142/142 · `pnpm check` 0/0/0

#### 后台项确认

- φ (front-matter CRLF + JSON) 已在第 45 轮交付，P1 段落标记过时已清理。
- `β` mock 数据扫描：components/ 无遗留 mock/TODO/FIXME（仅有 `paneTree.ts` 一处
  关于 paneCwds 持久化的已知 TODO，记录为下轮 P1 候选）。

---

### 第 58 轮（2026-04-25）— SearchSidebar 诊断即时显示 + Monaco 原生右键菜单

#### SearchSidebar 诊断即时显示（第 29 轮 MEDIUM 收尾）

**`src/lib/components/SearchSidebar.svelte`**

问题：`text_search_diagnostics` IPC 虽然在 `runSearch` 开头就 fire（与串行 search
loop 并行），但 `invalidGlobs = await diagnosticsPromise` 写在 loop 之后——大仓库多
root 情况下用户要等几秒才看到红圈，抵消了并行的初衷。

修法：改用 `.then()` sidecar 模式——诊断结果一 resolve（通常 <1ms）就立刻写入
`invalidGlobs`，不等 search loop 结束。用 `_diagGen` 单调计数器防止旧搜索的诊断覆
盖新搜索的结果（过时的 `.then()` callback 直接 no-op）。

#### Monaco 原生右键菜单（第 37 轮 LOW 决策）

**`src/routes/+page.svelte`** `handleContextMenu`

旧行为：右击 Monaco 编辑器时，Wind 的全局 `document contextmenu` handler 在 Monaco
自己的 handler 之后触发，叠加显示一个只含"水平/垂直分割 + 关闭窗格"的稀疏菜单，把
Monaco 原生菜单（Go to Definition / Rename Symbol / Format Document / Find All
References…）盖住。

新行为：`target === 'editor'` 时提前 `return`，让 Monaco 独立渲染其原生 contextmenu
（Monaco 的监听器在 editor container 上，event 到达 document 之前已处理）。

理由：Monaco 内置的编辑器操作比 Wind 稀疏菜单实用得多；分割/关闭等 pane 操作有专属
快捷键（Ctrl+Shift+H/V / Ctrl+W），不需要菜单入口。

**回归**：`pnpm check` 0/0/0 · `vitest` 142/142

---

### 第 57 轮（2026-04-25）— 真正根因修复：路径分隔符归一化

#### 根因分析

第 47c/48 轮引入了 `syncPaneLayoutFromBackend` 的 Pass 1 Prune + Pass 2 Seed，
逻辑上正确，但没有解决**最深层根因**：

`pty.rs` 有两条 `PaneCwdChanged` 发出路径：

| 路径 | 场景 | `pane.cwd` 写入 | 事件 payload |
|---|---|---|---|
| Path 1（主读循环 OSC 7）| 几乎所有 cwd 更新 | `pane.cwd = Some(cwd.clone())` ← **无归一化** | `cwd_clone.to_string_lossy()` ← **无归一化** |
| Path 2（EOF/尾冲刷）| PTY 退出时 | `PathBuf::from(normalize_cwd_str(...))` ✓ | `normalize_cwd_str(...)` ✓ |

结果：Git Bash 经由 Path 1 报告 `C:/code/wind`（`file:///C:/code/wind`），
PowerShell shell-integration 经由 Path 1 报告 `C:\code\wind`（`file://host/C:\code\wind`）。
两者在 `paneCwdStore` 里是**不同的 key**，`syncWithPaneCwds` 把它们映射为两列，
无论 Pass 2 Seed 多正确都无法合并——这就是"始终有一个终端没有合并"的真正原因。

#### 修复内容

**`src-tauri/src/engine/pty.rs`**（Path 1 主读循环）

```rust
// 修前
pane.cwd = Some(cwd.clone());  // raw backslash PathBuf
cwd: cwd_clone.to_string_lossy().to_string()  // raw backslash string

// 修后
let normalized = normalize_cwd_str(&cwd.to_string_lossy());
pane.cwd = Some(std::path::PathBuf::from(&normalized));  // forward-slash PathBuf
cwd: normalized  // forward-slash string
```

Path 1 现在与 Path 2 行为完全对称，`pane.cwd` 和事件 payload 均为正斜杠。

**`src/lib/stores/paneTree.ts`**（前端安全网）

新增 `normalizeCwd(cwd: string)` 函数（`\` → `/`），在两处防御性调用：
- `setPaneCwd` — 所有 cwd 写入 `paneCwdStore` 前归一化；
- `extractCwdsFromLayout` — 从 layout 读取 cwd 时归一化（防止后端未来再漏）。

#### 效果

- `syncWithPaneCwds` 收到 `{ 'pane-a': 'C:/code', 'pane-b': 'C:/code' }`（统一正斜杠）
  → cwdToPanes 只有一条 `'C:/code' → [pane-a, pane-b]` → **单列合并** ✓
- 终端关闭后 Prune 按 key 删除，与路径格式无关，zombie 同样修复 ✓

**回归**：`pnpm check` 0/0/0 · `cargo test` 68/68 · `vitest` 142/142

---

### 第 56 轮（2026-04-25）— P0-J Explorer 僵尸终端 & 跨终端合并专项单元测试

**`src/lib/stores/paneTree.test.ts`** — 追加 4 个 describe 测试（T1–T4）

| 用例 | 验证的场景 |
|---|---|
| T1 | Pass 1 Prune：关闭 pane 后 `ws1:pane-b` 僵尸键从 `paneCwdStore` 消失 |
| T2 | Prune 不跨工作区误删 `ws2:pane-x` |
| T3 | Pass 2 Seed：分屏新 pane 继承父 cwd，layout 中存在但 store 不含的键被种入 |
| T4 | Seed 不覆盖 `pane-cwd-changed` 已更新的活跃值（事件优先于 layout 快照） |

**`src/lib/stores/fileExplorer.test.ts`** — 追加 10 个测试（E1–E9，含 E3b）

| 用例 | 验证的场景 |
|---|---|
| E1 | 同 cwd 两个 pane → 合并成单列（Bug B 回归锁） |
| E2 | 不同 cwd → 各自独立列 |
| E3 | 最后一个 pane 关闭 → 列消失（Bug A 回归锁） |
| E3b | 两 pane 中关一个 → paneIds 缩小，列保留 |
| E4 | pane cd 到新路径 → 移到新列 |
| E5 | 其他工作区列不受影响（工作区隔离） |
| E6 | 新 pane 加入既有列 → 缓存树保留（无空白闪烁）+ `needsRefresh=true` |
| E7 | `syncAllWorkspaces` 按 `wsId:` 前缀路由到正确工作区 |
| E8 | 无 paneCwds 的工作区产生 0 列 |
| E9 | 跨工作区同 cwd 不合并（用户 round 47b 明确要求，以此锁住） |

**回归**：`pnpm check` 0 / 0 / 0 · `vitest` **142 / 142**（+14 新测试，128 → 142）

---

### 第 55 轮（2026-04-25）— 杂项收尾：commit 键盘菜单 + runGitOnSelectedRepo SCM 联动

#### commit 行 Shift+F10 / ContextMenu 键打开右键菜单

**`src/lib/components/SourceControl.svelte`**
- `onkeydown` 扩展：原仅处理 `Enter`；现在加 `ContextMenu` 键 和 `F10+Shift` 键。
- 合成 `new MouseEvent('contextmenu', { clientX: rect.left+8, clientY: rect.bottom })`，
  传入现有 `onCommitContextMenu(event, c)`——复用完整菜单路径，无重复逻辑。
- 修复了第 37 轮 review LOW：键盘用户现在可以 Tab 聚焦 commit 行再按 Shift+F10
  打开菜单，与 VS Code Git Graph 键盘体验对齐。

#### runGitOnSelectedRepo 与 SCM panel selectedRepo 联动（MEDIUM 修复）

**`src/lib/stores/scmCache.ts`**
- `ScmCacheState` 新增 `selectedScmRepo: string` 字段（初始值 `''`）。
- 新导出 `setScmSelectedRepo(root)` 和 `getScmSelectedRepo()` —— 写/读 SCM
  panel 当前选中仓库，使外部调用方可访问而无需破坏 SourceControl 的组件封装。

**`src/lib/components/SourceControl.svelte`**
- 导入 `setScmSelectedRepo`；在 `$effect(() => { if (selectedRepo) ... })` 块
  里加 `setScmSelectedRepo(selectedRepo)` ——每次 SCM 切换仓库时同步写 cache。

**`src/routes/+page.svelte`**
- `runGitOnSelectedRepo` 优先读 `getScmSelectedRepo()`；为空（SCM 未打开过）
  时才退回原有"遍历 paneCwdStore 找第一个 git 仓库"逻辑。
- 结果：git-graph 右键菜单里的 Fetch / Pull / Push / Sync 现在精准命中用户
  在 SCM 面板选中的仓库，而不是随机命中 paneCwdStore 里第一个 git 目录。

**回归**：`pnpm check` 0 / 0 / 0。

---

### 第 54 轮（2026-04-25）— 终端右键菜单（复制/粘贴/全选/清屏）

**`src/lib/components/Pane.svelte`**

- 新 `onTerminalContextMenu(e: MouseEvent)` handler：仅在 `mode === 'terminal' && term` 时触发；
  `e.preventDefault()` 阻止系统菜单。
- 菜单项：`复制`（仅有选中文本时出现）/ `粘贴`（读剪贴板 → `term.paste`）/ 分隔线 /
  `全选`（`term.selectAll()`）/ `清空`（`term.clear()` + Tauri `write_to_pty('\x0c')`）。
- 调用签名修正：`showContextMenu(e.clientX, e.clientY, items, 'terminal', paneId, workspaceId)`
  （之前错传 event 对象为第一参数）。
- 终端容器 `<div>` 加 `role="application" aria-label="终端"` 消除 a11y 警告。
- `pnpm check` **0 / 0 / 0**。

---

### 第 53 轮（2026-04-25 14:38）— SCM watcher 客户端 debounce + vitest 全绿确认

#### SCM watcher 客户端 250ms debounce

**`src/lib/components/SourceControl.svelte`**
- `const watcherDebounce = new Map<string, ReturnType<typeof setTimeout>>()` 模块变量。
- `listen('scm-repo-changed', handler)` 由 `async` 改为同步，内部 `setTimeout(fn, 250)`
  per-repo 去抖：同一仓库的多个 `.git/` 写事件（HEAD + index + refs）合并为一次
  `refreshStatus` + `loadGraph`，典型场景（`git commit`）减少到 1 次后端调用而非 3–5 次。
- `onDestroy`：清理所有 pending timers + `watcherDebounce.clear()`。

**回归**：`pnpm check` 0/0/0 · `vitest` 128/128

---

### 第 52 轮（2026-04-25 14:40）— P3-13 linkTrust per-basePath + SearchSidebar 结果限制（后台）

#### P3-13 — linkTrust per-basePath 信任作用域

**`src/lib/utils/linkTrust.ts`**
- `trustedHosts: Set<string>` → `trustedByBase: Map<normalizedBasePath, Set<string>>`
- 新 `normalizeBase(basePath?)` — 去尾斜杠 + toLowerCase
- 新 `getOrCreateSet(basePath?)` — 懒惰创建每个 basePath 的 Set
- `isTrustedUrl(url, basePath?)` + `trustHostFromUrl(url, basePath?)` 增加可选 basePath 参数
- `_resetTrustedHosts_forTests()` 改为清空整个 Map（向后兼容）
- 安全模型：同一 repo 里的 markdown 文件信任 example.com 后，其他 workspace 的
  markdown 文件打同一 host 仍需重新询问——与 VS Code workspace trust 对齐。

**`src/lib/components/MarkdownPreview.svelte`**
- `openExternal()` 里 `isTrustedUrl(href)` → `isTrustedUrl(href, basePath)`
- `trustHostFromUrl(href)` → `trustHostFromUrl(href, basePath)`
- `basePath` 来自组件 prop（父 FileEditor 传入当前文件的父目录），链路已就绪。

回归：`pnpm check` 0 / 0 / 0（3895 文件）

#### P2-8 确认已关闭

`PaneGitPill.commitCreate()` 已有 `branches = []` + `invalidatePaneGitStatusForRepo()`
（round 51 追加 toast 时同步确认）。关闭此条目。

#### SearchSidebar 结果限制（已交付）

派发后台 agent：默认显示前 100 条匹配结果（file header 不计），
超出时显示"显示全部 N 条结果"按钮；新搜索自动重置。
不引入新依赖，pure Svelte 5 runes。

---

### 第 51 轮（2026-04-25 14:34）— P3 Toast 系统 + P3-9 + P3-10 + CLAUDE.md 更新

#### WindToast 系统 (新)

**`src/lib/stores/toast.ts`** — 模块级 API：`showToast(message, type='success'|'error'|'info')`；
`toastStore` writable；每条 toast 3s 后自动移除。ID 单调递增确保正确清理。

**`src/lib/components/WindToast.svelte`** — 固定右下角 `fixed bottom-4 right-4 z-[10000]`；
`{#each $toastStore}` 渲染；`success` 绿色 CheckCircle / `error` 红色 XCircle /
`info` 灰色 Info icon；`aria-live="polite"` 无障碍。

**`src/routes/+page.svelte`** — 引入 `WindToast` 并 mount（`<WindToast />`）。

#### P3-10 — PaneGitPill 操作 toast

**`src/lib/components/PaneGitPill.svelte`**
- `switchTo()` 成功 → `showToast(\`已切换到 ${branch}\`)`
- `commitCreate()` 成功 → `showToast(\`已创建并切换到 ${trimmed}\`)`

#### P3-9 — ScrollbackHistoryModal 复制 toast

**`src/lib/components/ScrollbackHistoryModal.svelte`**
- `copyAll()` 成功 → `showToast('已复制到剪贴板')`（保留原有 1.5s checkmark 按钮状态，toast 是额外层反馈）

**回归**: `pnpm check` 0 / 0 / 0（3895 文件，+2 新文件）

---

### 第 50 轮（2026-04-25 14:26）— ε阶段二 notify crate 确认交付 + P1-4 PaneGitPill 分支过滤器

#### ε阶段二 状态确认

后台 agent 已完成，`cargo check` + `pnpm check` 均 0/0/0。  
6 个文件变更：Cargo.toml / `commands/watch.rs` (new) / `commands/mod.rs` /
`state.rs` / `lib.rs` / `SourceControl.svelte`。  
边界已知：worktree `.git`-is-file 场景静默跳过（下轮可扩展）；
前端 listener 无 debounce（高频 fetch 时一次 debounced event → 一次刷新，可接受）。

#### P1-4 — PaneGitPill 分支列表过滤器

**`src/lib/components/PaneGitPill.svelte`**
- 新 `branchFilter = $state('')` + `filteredBranches = $derived(...)` — 空时全显，
  非空时大小写不敏感 `includes` 过滤。
- Picker 打开时 `branchFilter = ''` 并 `requestAnimationFrame(() => filterInput?.focus())`
  — 键盘用户直接打字就能过滤，不用先 Tab 进输入框。
- Picker 关闭时 `branchFilter = ''` 重置，防止下次打开残留旧查询。
- 过滤框 `onkeydown`：Esc 关 picker + 清 filter；Enter 且 `filteredBranches.length === 1`
  时自动 checkout（单一匹配快捷路径）。
- `{#each branches}` → `{#each filteredBranches}`；无匹配时"无匹配分支"占位。
- 过滤框始终显示（不设 threshold）——VS Code branch picker 同策略，键盘
  直接开搜比先看列表再考虑要不要搜体验更流畅。

回归：`pnpm check` 0 / 0 / 0。

---

### 第 49 轮（2026-04-25 14:14）— P1-3 paneGitStatus 5分钟周期刷新 + ε阶段二 notify crate（后台）

#### P1-3 — paneGitStatus 5分钟周期后台刷新

**`src/lib/stores/paneGitStatus.ts`**
- 新 `refreshAllCachedRepos()` — 对 `cacheByRepo` 所有已知 repo root 各调
  一次 `invalidatePaneGitStatusForRepo`（并行）。
- 模块级 `setInterval(refreshAllCachedRepos, 5 * 60 * 1000)` — 低成本后台
  心跳。`cacheByRepo` 为空时 no-op。覆盖场景：`git pull` from 终端、CI 自动
  merge、teammate 在其他窗口 push 后 ahead/behind 角标自动更新。
- `pnpm check` 0 / 0 / 0。

#### ε阶段二 — notify crate git 文件系统监视器（已交付）

后台 agent 已完成：
- 为 `Cargo.toml` 加 `notify = "6"` + `notify-debouncer-mini = "0.4"`
- 新 `src-tauri/src/commands/watch.rs`：`GitWatcher` struct（每个 repo root
  一个 debouncer，500ms 窗口，watch `.git/` recursive）+ `start_watching_repos`
  Tauri command
- `state.rs` 注入 `GitWatcher`，`lib.rs` 注册命令
- `SourceControl.svelte`：`discoverRepos` 末尾调 `start_watching_repos`；
  `onMount` subscribe `scm-repo-changed` → `refreshStatus` + `loadGraph`

---

### 第 47c 轮（2026-04-25 14:03）— 终端关闭/cwd 切换时资源管理器正确清理文件树

**根因**：`paneCwdStore` 在 pane 关闭后存留僵尸键
- `closePane()` → `syncPaneLayoutFromBackend()` 更新了 `paneTreeStore`（后端布局），
  但没有从 `paneCwdStore` 删除 `"${workspaceId}:${deletedPaneId}"` 这条键。
- Explorer `$effect` 继续收到该僵尸 cwd，`syncWithPaneCwds` 认为还有 pane 指向这个
  目录，保留（甚至重建）对应文件树列。用户体感：关闭终端后资源管理器里的树不消失。

**CWD 切换（`cd` 命令）** — 已正确，不需要修复：
- `setPaneCwd(wsId, paneId, newCwd)` 更新 `paneCwdStore[wsId:paneId]` = newCwd。
- Explorer `$effect` 触发，`cwdToPanes` 仅含新 cwd，旧 cwd 列自然消失。

**根因补充（第 48 轮追加）**：原修复只做了 Prune（清理死 pane），但漏了 Seed（补充新 pane）。
两个 bug 共根：`syncPaneLayoutFromBackend` 不调 `extractCwdsFromLayout`，新 pane 的
cwd 从不加入 `paneCwdStore`。

**最终修复**（同时覆盖两个场景，一次原子 update）：
- **Pass 1（Prune）**：活跃工作区前缀下，paneId 不在 `getAllPaneIds(layout)` 中的条目
  删除（僵尸/关闭的 pane）。
- **Pass 2（Seed）**：`extractCwdsFromLayout(layout, active)` 中存在、但尚未在 store 中
  的条目（新 split pane 的初始 cwd）写入 store。
  Split pane 继承父 pane cwd，后端不发 `pane-cwd-changed`，所以必须从布局主动种入。

**覆盖路径**：`closePane` / `splitPane` / `dockPane` / workspace 切换 → 全部走
`syncPaneLayoutFromBackend`，一处修复全部修。

**回归**：`pnpm check` 0 / 0 / 0。

---

### 第 47b 轮（2026-04-25 13:59）— 撤销 ψ 跨工作区合并（用户要求：只跨终端，不跨工作区）

用户明确：「Explorer cwd 合并文件树不要跨工作区，只跨终端」。

**`src/lib/components/Explorer.svelte`**
- 移除 `normCwd()` helper 和 `primaryCwdOwner $derived`（第 46 轮 ψ Plan B 引入）。
- 移除模板中的单元素 `{#each}` let-binding + `_isSecondary` 分支 + "↑ 已在…" stub。
- 恢复直接渲染每个 column 的完整文件树（跨工作区不再去重）。

**已有的跨终端合并（保留，不受影响）**
- `fileExplorer.ts::syncWithPaneCwds` 已经对同工作区内多个 pane 同 cwd
  情况做合并：列 `id = "${wsId}:${cwd}"`，`paneIds[]` 汇聚所有同 cwd pane，
  Explorer section header 展示多个 pane 角标，只渲染一棵树。
- 这才是正确行为，本轮不动。

**回归**：`pnpm check` 0 / 0 / 0。

---

### 第 46 轮（2026-04-25 13:47）— ψ Explorer 跨工作区同 cwd 合并 + SCM 仓库折叠

两个独立任务，主线直接实现（不派子 agent）。

#### ψ — Explorer 跨工作区同 cwd 合并文件树（Plan B）

**`src/lib/components/Explorer.svelte`**

- 新 `normCwd(s)` helper：`s.replace(/\\/g, '/').replace(/\/+$/, '')`，
  统一 Windows/POSIX 路径比较。
- 新 `$derived primaryCwdOwner: Map<normalizedCwd, {workspaceId, workspaceName}>`：
  - 对 `$explorerWorkspaceGroups` 排序，活跃工作区排首位（优先获得
    "主列"所有权），其余按视觉顺序。
  - 遍历所有列，首次出现的 cwd 记录归属 workspace，后续同 cwd 跳过。
- 渲染时，每个 `col` 的树体包裹在 `{#each [primaryCwdOwner.get(normCwd(col.cwd))] as _cwdOwner}` 单元素 let-binding（Svelte 5 `{@const}` 要求 block 直接父）。
  - `_isSecondary = _cwdOwner?.workspaceId !== group.workspaceId`
  - `_isSecondary === true` → 只显示一行斜体占位：`↑ 已在「<主工作区名>」中显示`。
  - `_isSecondary === false` → 渲染完整文件树（原有路径）。
- 行为：
  - 单工作区场景：`primaryCwdOwner` 的所有 key 都属于同一 ws，无 secondary，无变化。
  - 两工作区同 cwd：活跃 ws 全树，非活跃 ws 只显示 header + 占位文字。
  - 主工作区关闭后，原副工作区变新主，下次渲染立刻显示完整树（`$derived` 自动重算）。
- **不改 store**：纯渲染层决策，零 loadTree 额外调用，零 localStorage 影响。

#### SCM 仓库折叠

**`src/lib/components/SourceControl.svelte`**

- `let collapsedRepos = $state(new Set<string>())`。
- `toggleRepoCollapse(root)` — 不可变 Set 更新；折叠时同时清 `branchPickerOpen = ''`。
- 仓库 header 左侧加 `ChevronRight` 按钮（`h-4 w-4 shrink-0`），旋转 90° = 展开，
  0° = 折叠；与 Explorer workspace/column header 视觉一致。
- 仓库主体 `{#if s}...{/if}` 外层包一层 `{#if !collapsedRepos.has(root)}{/if}`。
- **Bug fix**：`toggleRepoCollapse` 里原来写 `branchPickerOpen = null`（类型 `string`
  不接受 `null`），改为 `''`；同时移除 `<span>` 上多余的 `onclick`（a11y，改为
  只用 `<button>` chevron 响应折叠点击）。
- 顺带清掉 `FileEditor.svelte` 里 `wf-tab-scroll` 的两条残留 CSS 警告（上轮
  切到 overlayScroll 后变 unused）。

#### 回归

- `pnpm check` **0 / 0 / 0**（3893 文件）

---

### 第 45 轮（2026-04-25 13:36）— χ SCM 图谱缓存 + ο ref pills 折叠 + φ front-matter CRLF + FileEditor/FileEditor tab overlayScroll

并行调度两个 sub-agent（χ / ο+φ）+ 主线做 FileEditor tab 滚动条。
所有 4 个任务本轮全部交付。回到主线后处理跨工作区合并文件树计划（ψ），
并更新文档。

#### χ — SCM 图谱缓存

**`src/lib/stores/scmCache.ts`**（扩展）+ **`src/lib/components/SourceControl.svelte`**
- `CommitNode`、`DiffFile`、`GitRepoInfo` 接口从 `SourceControl.svelte`
  迁移到 `scmCache.ts` 并 export，解除循环依赖；`SourceControl` 改为
  从 cache 模块 import。
- `ScmCacheState` 新增三个字段：`graphInfos: Record<string, GitRepoInfo>`、
  `lastGraphLoadAt: Record<string, number>`、
  `selectedCommitHashByRepo: Record<string, string>`。
- `setScmRepoRoots` GC 新三个字段（与 statuses 对称）。
- 新 API：`setScmGraphInfo`、`clearScmGraphInfo`、
  `shouldRefreshGraphOnMount`、`setScmSelectedCommit`、
  `getScmSelectedCommit`。
- `SourceControl.svelte`：
  - `graphInfo` 从 `$state` 改成 `$derived($scmCacheStore.graphInfos
    [selectedRepo] ?? null)`。
  - `selectedCommitHash` 从 `$state` 改成 `$derived(getScmSelectedCommit
    (selectedRepo))`；所有写入改为 `setScmSelectedCommit(…)`。
  - `loadGraph(root, { resetSelection? })` 写 cache；onMount 按
    `shouldRefreshGraphOnMount` 判断 cache-hit（1s 后台刷新）vs cache-miss
    （立刻加载）。
  - spinner 只在 `graphLoading && !graphInfo`（无缓存首次加载）显示。
- **`src/lib/stores/scmCache.test.ts`** +6 个 test（graph GC、stale 判定、
  selectedCommit 跨 mount 保持、cache miss path）。
- 最终 SourceControl.svelte 1394 行（≤1400 软上限）。

#### ο — commit ref pills 折叠

**`src/lib/components/SourceControl.svelte`**
- 常量 `MAX_VISIBLE_REFS = 2`（script 顶部）。
- HEAD 例外：`headOffset = refs[0]==='head:' ? 1 : 0`，maxVisible =
  MAX_VISIBLE_REFS + headOffset（HEAD + 本地分支永远一起出）。
- `splitRefs(refs)` helper 返回 `{ visible, hidden }`；模板里用
  `{#each [splitRefs(c.refs)] as { visible: visibleRefs, hidden: hiddenRefs }}`
  作 let-binding（Svelte 5 的 `{@const}` 在 `<div>` body 可用，此处
  用 single-element each 兜底一致性）。
- 溢出角标：灰色 `bg-[var(--wf-surface)] text-[var(--wf-fg-muted)]` pill，
  `title` = 隐藏 ref 名换行拼接。
- 不加 click-to-expand（hover title 够用，YAGNI）。

#### φ — front-matter CRLF + JSON

**`src/lib/utils/markdown.ts`** + **`src/lib/utils/markdown.test.ts`**
- `stripFrontMatter` 顶部加 `source = source.replace(/\r\n/g, '\n')`。
- 新增 JSON front-matter 识别：`lines[0] === '{'`、闭合 `}`。
- **+3 个 test**（CRLF YAML、JSON front-matter、mid-doc `{` 不误吞）。

#### FileEditor tab bar → overlayScroll

**`src/lib/components/FileEditor.svelte`**
- 行 448：`overflow-x-auto wf-tab-scroll` 替换为
  `use:overlayScroll={{ preset: 'horizontal-tabs', layout: false }}`。
- `layout: false` 保留原有 `flex items-center`（tab 间用 border-right
  分隔，不需要 gap 注入）。

#### overlayScroll layout 注入

**`src/lib/actions/overlayScroll.ts`**
- 新 `OverlayScrollLayout` interface + `layout` option（`OverlayScrollLayout
  | false`）。
- `PRESET_DEFAULT_LAYOUTS`：`horizontal-tabs` 默认 `{ direction:'row',
  align:'center', gap:4 }`；`sidebar` 无默认布局。
- `applyLayout()` 把 `display/flex-direction/align-items/gap` 注入为
  inline style；`destroy()` 清除；`update()` 重新应用。
- `WorkspaceTabs.svelte` 去掉 `flex flex-row items-center gap-1`（由
  action 注入）。

#### 回归

- `pnpm check` **0 / 0 / 0**（3893 文件）
- `vitest` **128 / 128**（+6 scmCache graph tests，+3 markdown CRLF/JSON tests）

#### 新计划项

- **ψ** Explorer 跨工作区同 cwd 合并文件树（用户 13:36 反馈）写入 P1
  候选；同工作区内已合并（`id = "${wsId}:${cwd}"`），跨工作区为缺口。

---

### 第 44 轮（2026-04-25 13:16）— ρ 慢盘 progress + σ image lazy + τ front-matter + υ SCM splitter（4 个 P1 一锅端）

并行调度三个 sub-agent，分管 Explorer / markdown / SCM。回到主线后做
review + 写计划。所有 ρ/σ/τ/υ 候选条目本轮全部消化。

#### ρ — Explorer 慢盘 500ms latency-gated progress bar

**`src/lib/components/Explorer.svelte`**
- 第 43 轮把 spinner 完全清空后，本地 SSD 加载 <200ms 体感即时；但
  网络盘 / WSL `/mnt/c` cold tree 几百毫秒里完全没反馈。VS Code 的
  做法："超过 500ms 才出 indicator，到了就消失"——保持快盘静默承诺。
- 实现：
  - `SLOW_LOAD_THRESHOLD_MS = 500`；`slowLoading = $state(new Set<id>())`；
    `slowTimers = new Map<id, timeout>()`（非响应式 Map，纯定时器簿记）；
    `slowPrevLoading = new Map<id, boolean>()` edge-detect。
  - `$effect` 监 `$fileExplorerStore.columns`，对每列计算
    `now = col.loading && !col.tree`（首次 load 或 needsRefresh 无缓存
    才进入 slow 监控；有缓存的后台静默刷新永远不出 progress）。
  - `now && !prev` → setTimeout 500ms → 加进 `slowLoading` Set。
  - `!now && prev` → 清 timer + Set。
  - 列 unmount → 清所有相关 timer / Set / prev 条目。
  - `onDestroy` → 清所有挂起 timer。
- 渲染：
  - section header 下、tree body 上方 2px `<div class="explorer-progress"
    role="progressbar" aria-busy="true">`。
  - CSS `::before` 30% 宽 sliding band，`@keyframes
    explorer-progress-slide` 1.1s `cubic-bezier(0.4, 0, 0.2, 1)` infinite。
  - `@media (prefers-reduced-motion: reduce)` 慢到 2.4s（避免完全静止
    误导用户以为卡死）。
- header refresh button **不再加 spinner**——progress bar 是唯一 indicator。
- 文件最终 793 行（之前 753 + 删了一段过长 keyboard-nav 注释保持低于
  800 软上限）。

#### σ — MarkdownPreview 图片懒加载

**`src/lib/utils/markdown.ts`**
- 新 `renderer.image = (token: Tokens.Image)`：
  - `loading="lazy"`：浏览器原生 IntersectionObserver，离屏图片不主动
    请求，避免长 README 的瀑布刷屏。
  - `decoding="async"`：解码非阻塞主线程。
  - alt 文本走 `parseInline(tokens, textRenderer) + escape`，与 marked 默认
    行为一致；alt 中的内联 markdown（`![**bold**](src)`）按 HTML 语义
    flatten 成纯文本。
  - title 属性条件渲染。
- 大图 width/height 占位留作未来轮：需要 Tauri IPC 探针 + 后处理 DOM
  pass，破坏当前同步渲染签名，本轮按 YAGNI 跳过（renderer 内 inline
  注释了原因）。

#### τ — front-matter 隐藏

**`src/lib/utils/markdown.ts`**
- 新 `stripFrontMatter(source: string): string`：
  - 仅当 `lines[0]` 严格等于 `---`（YAML）或 `+++`（TOML）才进入。
  - 走纯 line-based 扫描，找闭合 fence 行（同 fence 字符严格相等）。
  - 找不到闭合 → 视作正文，原样返回（避免误吞普通水平线后所有内容）。
  - 找到 → 把 `[0..closeIdx]` 整段替换为空字符串再 join，**保留行数**：
    下游 `[data-wf-md-src-line]` 注释的行号与用户编辑器一致，preview
    ↔ source 同步不破。
  - 严格相等避免误吞 setext heading underline（`title\n---`）和缩进的
    `---`。
- 接入：`renderMarkdown(source)` 先 `stripFrontMatter` → 再
  `normaliseWindowsPathLinks`（front-matter 内的 YAML 不应被反斜杠重写）。
- 已知 edge：CRLF 行尾 + JSON `{...}` front-matter + Pandoc `---yaml`
  info 字符串均不识别。CRLF 在 round 45 跟进。

#### υ — SCM 上下分屏拖动条复刻侧边栏 resize 样式

**`src/lib/components/SourceControl.svelte`** `<style>` 段
- 用户 13:10 反馈：changes ↔ graph 之间的拖动条太宽太抢眼。
- 旧：`background: var(--wf-border)`（常态 1px 实线）+ hover 全段 accent。
- 新：默认 `background: transparent` + `transition: background-color
  150ms ease`；hover `color-mix(in oklab, var(--wf-accent) 20%,
  transparent)`；active `30%`——与 sidebar resize handle（行 1086）的
  `/20` `/30` 透明度梯度完全一致。
- splitpanes 的 dragging 状态类是 `splitpanes__splitter__active`（双下
  划线，不是 BEM `--`）——直接看 `node_modules/svelte-splitpanes/dist/
  Pane.svelte:89` 确认。同时挂 `:active` 兜底 mousedown 那一帧。
- 物理高度仍 1px，`::before` 上下 -3px 共 7px hit area 不变——只动视觉
  不动可点中性。

#### 回归

- `pnpm check` **0 / 0 / 0**（3893 文件）
- `vitest` **119 / 119**（+9：3 image renderer + 6 stripFrontMatter）

#### Module-level review（VS Code 对标）

- ρ：500ms threshold 是 VS Code 经验值；indeterminate sliding 与
  Workbench progress 视觉同款。无 HIGH。
- σ + τ：渲染管道的 pre-pass + renderer override 模式干净；test 覆盖
  足够。CRLF / JSON front-matter 列入下轮 LOW。
- υ：纯 CSS 微调，无 JS 逻辑面，无回归风险。
- 4 个文件单独 audit，未发现新 CRITICAL / HIGH。

#### 下轮启动建议

按计划现在该做 **ο（commit ref pills 折叠 + 数字气泡 menu）**——用户
之前明确反馈，纯前端，量级小。

---

### 第 43 轮（2026-04-25 13:05）— 文件树静默打开 + md 链接 4 个真实 bug 修复（π）

并行调度两个 sub-agent，分别处理 sidebar 流畅度 + markdown 链接拦截。
回到主线后做 module-level review + 写计划文档。

#### π-1：Explorer / FileTree 打开静默化

1. **`src/lib/components/Explorer.svelte`**
   - 移除首次加载时 section header 里的 `<RefreshCw animate-spin>`：
     无论 first-load 还是后台静默刷新，header 上始终是 hover-show 的
     刷新按钮。spinner 是"加载提示"，与用户诉求"打开文件夹时不要有
     加载提示"直接冲突。
   - 移除"空目录"占位文字：`col.tree === null && !col.loading` 这条
     分支以前会渲染 `<div>空目录</div>`，但 loadTree 会迅速把数据塞
     进来，导致用户看到"空目录 → 真实树"的两帧切换。现在 body 在
     首次 load 期间保持空白，原子 swap 到真实树。

2. **`src/lib/components/FileTree.svelte`** — 真正的闪烁源根因
   - 旧实现：用户点 chevron 展开目录 → `loadChildren()` 走 IPC →
     回程数十毫秒里 `children = []` → 渲染空 → 数据回来后再渲染。
     肉眼能看见"先空一帧"的闪烁。
   - 修：`loadTree(depth=3)` 已经预取了三层 `node.children`。新增
     `$effect` 在挂载/prop 变化时把 `node.children` 同步进本地
     `children = $state`，并把 `hasLoaded = true`。这样 expand 时
     直接用 prefetched children 渲染，根本不进 IPC roundtrip。
   - 仅当 `node.children` 完全缺失（深度 4+ 子目录、刷新后第一次展开）
     才走异步 `loadChildren` 兜底。
   - 同 `$effect` 还负责"父级 needsRefresh / 用户刷新后 node 重建"
     的场景：node.children 替换为新数据时本地 children 一并 swap，
     避免渲染滞留旧值。

3. **回归**
   - `pnpm check` **0 / 0 / 0**（3893 文件）
   - `vitest` **110 / 110**

#### π-2：MarkdownPreview 链接拦截 4 个真实 bug

逐一在源代码中复现，全部为 Windows / 非 ASCII 场景：

1. **反斜杠路径被 marked 吞掉** — `[a](docs\sub.md)` → CommonMark 把
   `\` 当作转义字符，渲染后 href 变成 `docssub.md`。新 utility
   `normaliseWindowsPathLinks(source)`：
   - 在 `marked.parse` 前预处理源码，按 `` ` `` 切片跳过 inline /
     fenced code spans（避免误改代码示例里的 `like\this`）。
   - 用 `[label](target)` 形态的 regex 只改 link target；URL scheme
     (`http://`、`file://`、`mailto:`、`tel:`、`data:`、`javascript:`) +
     fragment-only `#x` + protocol-relative `//foo` 一律跳过。
   - **故意保留 `C:` / `D:` 不当作 scheme**：Windows drive letter
     长得像 RFC 3986 scheme，但里面正是需要 `\ → /` 的反斜杠源头。
   - 命中后整段 target `replace(/\\/g, '/')`。下游
     `decodeURIComponent + joinPath` 已经 posix-friendly。

2. **`?query` 把相对路径搞成不存在的文件** — `[img](./logo.png?v=2)`
   旧路径会去 `read_file_for_editor("./logo.png?v=2")` 直接报错。
   `handleAnchorClick` 先 split off query string 再做路径解析。

3. **`[here](.)` / `[here](./)` 错误"打开当前目录文件"** —
   `joinPath(basePath, '.')` 会构造一个非文件路径，`openFile` 报错。
   改成识别"纯当前目录"href → 调用 `reveal_in_file_manager(basePath)`，
   行为对齐 VS Code（点目录 → 打开 OS 文件管理器）。

4. **`decodeURIComponent` 抛异常** — 用户在 markdown 里手写一个 `%`
   而不是合法的 URL-encoded 序列（如 `[x](./100%2008.md)` ✓ 合法 vs
   `[x](./100%.md)` ✗ malformed），`decodeURIComponent` throw
   `URIError`。包 try/catch，失败时退回原始字符串（read_file_for_editor
   会自然报"找不到文件"，比静默 no-op 更可见）。

5. **回归**：`vitest src/lib/utils/markdown.test.ts` **18 / 18**。

#### π-3：模块 review（VS Code 对标）

针对本轮触碰的 4 个文件做了一次 review。整体合格，没有 CRITICAL/HIGH，
列入 next-loop 的 ρ / σ 候选见下。

#### 不该做 / 已经在做的事

- **MarkdownPreview 不接 overlayScroll**：preview 容器的 wheel 在
  absolute-positioned 父级里和 overlayscrollbars 的 wheel hijack 互
  斥（早期轮次踩过坑）。preview 维持原生 `overflow-y-auto` +
  `wf-scroll`。
- **GitGraph 不接 overlayScroll**：纯 SVG，没有内部滚动域；外层
  SCM 容器已经在用 overlayScroll。
- **同文档 anchor `[here](./README.md#sec)` 微优化**：当前重复
  `openFile` 是无害短路，加 prop "currentPath" 仅为美感，YAGNI 跳过。

---

### 第 42 轮（2026-04-25 12:52）— SCM tab 缓存 MVP（ε 阶段一）

切到源代码管理 tab 不再每次重新 discover + 全量 status 拉取。

1. **新模块 `src/lib/stores/scmCache.ts`**
   - 模块级 `scmCacheStore`：保留 `repoRoots[]`、`statuses{}`、cwd
     签名、repo 签名、`lastDiscoverAt` 时间戳。SourceControl 卸载
     不再丢数据；重新 mount 立刻 hydrate。
   - 写入 API：`setScmRepoRoots(roots, cwdSig, repoSig)`（同时把
     不再存在的仓库的 status 一并 GC）+ `setScmRepoStatus(root, s)`
     + `clearScmRepoStatus(root)`。
   - `shouldRefreshOnMount(maxAgeMs=30_000)` —— 缓存空 / >30s 旧
     则 `true`，否则 `false`（信任缓存）。
   - 7 个 vitest case 覆盖 GC、签名、stale 判定。

2. **SourceControl 接入**
   - `repoRoots` / `statuses` 改为 `$derived($scmCacheStore.repoRoots
     / .statuses)`，模板零修改。
   - `discoverRepos` / `refreshStatus` 写入 cache（不再写组件内
     `$state`）。
   - onMount：`shouldRefreshOnMount()` true → 立即 schedule
     discover；false → 缓存即时显示 + 1s 后后台 refresh，让用户感觉
     "瞬间出 + 自动更新"。
   - 重 mount 时 `selectedRepo` 也从缓存 seed，避免空值闪烁。

3. **效果**：sidebar tab 在 files / search / git / claude 之间切换
   现在 git → 立即出（相比之前每次都 round-trip + render flicker）。

4. **回归全通**
   - `pnpm check` **0 / 0 / 0** · `vitest` **110 / 110**（+7 scmCache）
   - `cargo test` **68 / 68** · `pnpm e2e` **7 / 7 (12s)**

5. **ε 阶段二（未来轮）**：真正的 `notify` crate 文件系统监视器，
   监听 `.git/HEAD`、`.git/index`、refs/heads、工作树 mtime 变化，
   事件驱动 invalidate 替代 30s 定时刷新。store 形状不变，仅替换
   refresh trigger。

---

### 第 41 轮（2026-04-25 12:45）— overlayScroll preset 拓展 + WorkspaceTabs 横向滚动适配（ξ）

1. **`overlayScroll.ts` 加 preset 形参**
   - 新 `preset?: 'sidebar' | 'horizontal-tabs'` 参数；不传则默认
     `sidebar`（保留旧行为：`{x:'hidden', y:'scroll'}` + autoHide=
     leave + 600ms delay）。
   - 新 preset `horizontal-tabs`：`{x:'scroll', y:'hidden'}` +
     autoHide=leave + 800ms delay（横向给用户多看一会儿，知道有
     更多 tab 在右边）。
   - `mergeOptions` 帮手做"preset + override"二级合并：scrollbars 与
     overflow 嵌套对象按 key 合并，其它字段平铺覆盖。callers 单 knob
     微调不会丢掉整段 preset。
   - 现有 6 处 callsite 全部继续走默认 sidebar preset，无 breaking。

2. **WorkspaceTabs 适配**
   - 之前 `use:overlayScroll={{ options: { overflow: { x:'scroll',
     y:'hidden' } } }}`——一坨硬编码。改成
     `use:overlayScroll={{ preset: 'horizontal-tabs' }}` 一行。
   - 加 `onwheel` 处理 shift+wheel：把 deltaY/deltaX 转 scrollLeft +
     preventDefault。给"鼠标只能竖滚"的用户一条横向 pan 路径，且
     与 ζ 轮 commit message 同模式。
   - 加 `flex-row` 显式（之前 flex 默认 row 但 explicit 更安全）。
   - 注释解释为什么 `min-w-0` 在这个 flex parent 里是触发 overflow
     的关键。

3. **回归全通**
   - `pnpm check` **0 / 0 / 0** · `vitest` **103 / 103**
   - `cargo test` **68 / 68**（未触碰后端）· `pnpm e2e` **7 / 7 (12s)**

4. **未来 preset 候选**
   - `'modal-body'`（diff editor / scrollback modal 内）
   - `'always-visible'`（autoHide:'never'，调试模式）
   - `'thin'`（更窄的 thumb，给 secondary 区域）

---

### 第 40 轮（2026-04-25 12:39）— pane git pill cwd-down 语义 + 多仓库切换器（θ）

用户连续 4 轮关注 git pill 行为；本轮按用户的心智模型重写：cwd 是
"容器"，git 仓库是"内部"东西。

1. **新语义**：`paneGitStatus.ts::resolveInfoForPane(paneId, cwd)`
   - 改用 `find_git_repos_below(cwd, max_depth=1)`——扫 cwd 自身 +
     直接子目录里的 `.git/`。
   - **不再向上找 ancestor**（之前 `find_git_repo_root` 走 git 标准
     语义，但用户的预期不同）。
   - 0 repos → null → pill 不渲染；1 repo → 单一渲染；>1 → 渲染
     +switcher。

2. **多仓库选择**：
   - `PaneGitInfo.availableRepos: string[]`（新字段）。
   - 模块级 `selectedRepoByPane: Map<paneId, repoRoot>` 记住用户选
     择；当当前选择仍在 availableRepos 中就用它，否则回退到第一个。
   - 新 `setPaneSelectedRepo(paneId, repoRoot)` API：更新 pick + 调
     `resolveInfoForPane` 重新解析。

3. **`PaneRepoSwitcher.svelte`（new）**：
   - 仅在 `availableRepos.length > 1` 时渲染——单仓库情况完全不出现，
     避免噪音。
   - 灰色 Folder pill 显示当前仓库 basename；点击 → 下拉列表（每行
     basename + 完整路径 tooltip + 当前选中 ✓）。
   - 全局 mousedown / Esc 关闭，与 PaneGitPill / SourceControl 同模式。

4. **SplitContainer 挂载**：repo switcher → branch pill → diff pill 顺序，
   用户的视觉移动方向 = 选 repo → 看分支 → 看改动。

5. **invalidatePaneGitStatusForRepo 适配**：除 `info.repoRoot ===
   repoRoot` 外，还检查 `info.availableRepos.includes(repoRoot)`——
   兄弟仓库的 stage/commit 也应触发同 pane 的刷新。

6. **vitest 契约扩展**：4 个新 case
   - 单仓库 cwd → availableRepos 一项
   - 多仓库 cwd → availableRepos = N，repoRoot 默认第一个
   - setPaneSelectedRepo 切换 + availableRepos 保留
   - cwd 变化后 stale pick 自动落回 availableRepos[0]

7. **回归全通**
   - `pnpm check` **0 / 0 / 0** · `vitest` **103 / 103**（+4）
   - `cargo test` **68 / 68** · `pnpm e2e` **7 / 7 (13s)**

---

### 第 39 轮（2026-04-25 12:32）— SCM untracked stage-all + commit msg 横向滚 + sidebar 80% 宽

3 个独立小切片，全前端。

1. **λ — Untracked group "暂存全部" hover 按钮**
   - 旧 header 是单 `<button>`，缺少 staged/changes 那种 hover-show
     batch 按钮。重构成 `<div class="group/grp">` wrapper + 内嵌
     toggle button + `+ 暂存全部` 按钮 + count。
   - 调用现有 `stage(root, s.untracked.map(f => f.path))`，与 changes
     stage-all 共用一份逻辑。

2. **μ — Commit message Shift+wheel 横向滚动**
   - commit row 的 message `<span>` 从 `truncate` 改成
     `whitespace-nowrap overflow-x-auto`；新 `wf-msg-scroll` 类把
     webkit + firefox 滚动条都隐藏（per-row overlayscrollbars 太重）。
   - `onwheel` handler：仅在 `e.shiftKey` 时把 deltaY/deltaX 转
     scrollLeft + preventDefault，不影响默认竖向滚动 UX。

3. **ν — Sidebar 最大宽度 40% → 80%**
   - `windowWidth40` $derived → `viewportInnerWidth $state +
     sidebarMaxPx $derived (innerWidth * 0.8)`。
   - 新 window resize listener：实时更新 `viewportInnerWidth`；
     若现行 `sidebarWidth > sidebarMaxPx` 则 clamp + 持久化（避免
     缩窗后 sidebar 残留过宽）。
   - 拖拽 handle 上限同步走 `sidebarMaxPx`，3 处 callsites 一并改名。

4. **回归全通**
   - `pnpm check` **0 / 0 / 0** · `vitest` **99 / 99**
   - `cargo test` **68 / 68** · `pnpm e2e` **7 / 7 (12s)**

---

### 第 38 轮（2026-04-25 12:23）— 全局 DnD regression 修复 + WorkspaceTabs overlayscrollbars

1. **κ — 全局 DnD 失效根因**（CRITICAL，用户体感"所有拖拽都不可用"）
   - 根 `<div>` 上有 `data-tauri-drag-region` —— 这把整个窗口都标
     成 OS-window 拖拽区，Tauri 在 mousedown 阶段就吞掉事件，HTML5
     DnD 的 `dragstart` 永远不触发。
   - 涉及面：WorkspaceTabs reorder / SplitContainer pane drag /
     FileTree DnD / FileEditor tab reorder——4 个独立 DnD 链路同时
     断。
   - 修：从根 `<div>` 移除 `data-tauri-drag-region`；OS-window 拖拽
     仍由顶部 `<header data-tauri-drag-region>`（行 1102）承担，那一
     段没有可拖拽 child，互不冲突。
   - 加 e2e 锁：扫所有 `[data-tauri-drag-region]` 元素，禁止任何
     viewport ≥80% 宽 + ≥50% 高的元素持有这个属性。未来再被无意
     加上立刻 fail。

2. **ι — WorkspaceTabs → overlayscrollbars**
   - 之前用 `wf-scroll`（webkit thin），切到 `use:overlayScroll
     options.overflow={x:'scroll', y:'hidden'}`，与 Explorer/SCM
     视觉一致（浮层 + idle 隐藏）。
   - workspace tab 元素本身是 `flex shrink-0`，不影响 DnD。

3. **回归全通**
   - `pnpm check` **0 / 0 / 0** · `vitest` **99 / 99**
   - `cargo test` **68 / 68** · `pnpm e2e` **7 / 7 (12s)**（+2 DnD guards）

---

### 第 37 轮（2026-04-25 12:14）— 全应用右键菜单系统化（ζ）

把第 34 轮"能弹起"基础上每个 target 的菜单从 stub 转成真实功能 +
补全有用项。

1. **terminal/editor/pane-content** —— 加 `复制 cwd 路径` /
   `在文件管理器中显示 cwd`。`关闭当前窗格` / `关闭其他窗格` 从
   `() => {}` stub 接到 `closePane` / 批量 close（带确认）。

2. **splitter** —— 删掉 stub `均分窗格`（后端没 reset-ratios 命令），
   保留分屏两项。

3. **sidebar** —— 加 `搜索` tab 入口；`Git` 标签改名 `源代码管理`
   与 rail tooltip 一致。

4. **workspace-tabs** —— `重命名工作区` 接 `promptDialog` →
   `renameWorkspace`。`保存为 .wind` 入口去掉避免双入口（Explorer
   头部已有）。

5. **git-graph** —— `Fetch` / `Pull` / `Push` / `Sync` 接真实命令：
   新 helper `runGitOnSelectedRepo(cmd, label)` 从 paneCwdStore 探出
   一个 git 仓库 → invoke → 触发 SCM 刷新。`刷新` 改成 `打开源代码
   管理` 直接切到 SCM tab。

6. **pane-header** —— stub `mode 切换` 删除（无后端），改成实用
   `水平/垂直分割` + `复制 cwd` + `在文件管理器中显示` + `关闭窗格`。

7. **default** —— 删 stub `设置`（已有底部 rail 齿轮）；只剩
   `新建工作区`。

8. **新增 helpers**：`closeCurrentPane(paneId)` / `closeOtherPanes`
   （带 confirmDialog） / `renameActiveWorkspace`（promptDialog）/
   `runGitOnSelectedRepo` / `copyPaneCwd` / `revealPaneCwd`。
   - 失败一律走 `alertDialog` themed 错误。

9. **回归全通**
   - `pnpm check` **0 / 0 / 0** · `vitest` **99 / 99**
   - `cargo test` **68 / 68**（未触碰后端）· `pnpm e2e` **5 / 5 (11s)**

10. **inline 清理（用户 12:14 反馈：移除资源管理器 claude pane status）**
    - `WorkspaceSummaryPanel` 删掉 "N pane · M 历史" 的 Claude badge，
      只保留通用的 "N pane" 计数（Activity icon）。Claude 信息现在
      仅活在 round 27/34 的独立 ClaudeCodePanel tab 里，Explorer
      工作区头不再混入。

11. **review 留给下轮**
    - **MEDIUM** `runGitOnSelectedRepo` 当 paneCwdStore 中有多个 git
      仓库时随机挑第一个；应该让用户选（或与 SCM 当前 selected repo
      联动）。
    - **LOW** 终端右键还缺 `复制选中` / `粘贴` / `清屏` / `选择全部`
      这些原生终端心智项 —— 需要触达 xterm.js 实例（Pane 内部）。
    - **LOW** Monaco 已有原生 contextmenu；当前 wind 的
      `oncontextmenu` 在 .monaco-editor 上覆盖掉它。可能用户想要
      Monaco 原生菜单（含 Go to Definition 等）—— 需要决策。

---

### 第 36 轮（2026-04-25 12:07）— pane git pill 真实数据契约锁 + 用户验证文档

用户连续 3 轮反馈"git 按钮在非 git 仓库也显示 / 用模拟数据"——前两
轮的 fix 没让用户信服。本轮做硬锁 + 写自助验证文档。

1. **数据流端到端审计** —— grep `mock` / `placeholder` / 字面 git
   字段在 `PaneGitPill` / `PaneDiffPill` 全文，唯一命中是 input
   placeholder 文案，**0 处 mock 数据**。两个 pill 的渲染严格
   `{#if info && info.branch}`，info 由 `paneGitStatusStore` 喂养，
   store 由 `resolveInfoForCwd → find_git_repo_root` 真实喂养。

2. **新 vitest contract 锁**（3 个 case）
   - `clears the store entry when cwd is null` —— null cwd 必清
     store entry。
   - `returns null for cwd outside any git repo` —— backend null →
     store null → pill 隐藏。
   - `debounces rapid cwd bounces — only the last cwd resolves` ——
     250ms debounce 锁，cd 链不会 N 次 IPC。
   - 未来任何回退（删 gate / 引 mock seed / 退化 debounce）会立刻
     fail。

3. **`trackPaneGitStatus` 微优化** —— `prev === cwd` 比对前两边都
   normalize 成字符串（之前 prev='' vs cwd=null 比对永真不等，
   重复 null 调用做无效 store update + 删除 noop）。

4. **`docs/PANE_GIT_PILL_VERIFY.md`（new）** —— 用户自助验证 3 步：
   - 在确定不在任何 git 仓库下的目录 (`C:\Users\<you>\Music`) 开个
     pane；
   - 看标题栏 GitBranch / FileText pill **不应该**出现；
   - 如果还能看到，运行 `git rev-parse --show-toplevel` 在该 cwd
     验证——若返回路径，pill 显示是**正确**的（你 cd 进了某个
     git 仓库的子目录）。

5. **回归全通**
   - `pnpm check` **0 / 0 / 0** · `vitest` **99 / 99**（+3 paneGitStatus）
   - `cargo test` **68 / 68** · `pnpm e2e` **5 / 5 (10s)**

---

### 第 35 轮（2026-04-25 11:59）— Agent Teams teammates 分屏能力研究报告

调研轮次，无代码改动。产出 `docs/AGENT_TEAMS_TEAMMATES.md` 完整链路
报告：

1. **链路图**：Claude Code → tmux shim binary → HTTP API
   `/api/v1/split-window` → `route_split` → `teammate_split_pane` →
   `PaneTree::split` → `ensure_pane_pty_workspace` → 前端
   `teammate-layout-changed` 事件 → SplitContainer 渲染新 pane（绿色
   Bot pulse）。

2. **结论：Wind 已经真支持** teammates 自动分屏。`cmd_split` /
   `route_split` 端到端连通，每个 teammate 进入独立 Wind pane，cwd
   继承，命令 PTY 真起。空闲 pane 复用（`allow_idle_reuse`）已加。

3. **PARTIAL 缺口（不阻塞主流程）**：
   - `new-session` shim 内 stub，不真分会话；
   - `kill-pane` 故意 no-op（避免误关用户 pane）；
   - `resize-pane` 故意 no-op（Wind 用户用 splitpanes 拖拽控制）；
   - `new-window` 路由把"新 window"翻译为"新 pane"——Wind 没有
     tmux window 概念；
   - tmux 模板 `#{...}` 渲染靠查表，未涵盖的占位符返回原文；
   - `rename-window` 当前没有路由。

4. **验收 3 步给用户测**：build shim → PATH 配置 → `claude` 启
   teammate → 期望 Wind 立刻新分 pane 并出现绿色 Bot 标记。

5. **回归**
   - `pnpm check` **0 / 0 / 0** （无代码改）

---

### 第 34 轮（2026-04-25 11:53）— 右键菜单复活 + Explorer 清 Claude

1. **`<ContextMenu />` 全局未挂载**（α 根因 — CRITICAL bug）
   - 第 33 轮（或更早）+page.svelte 只 `import` 了 `ContextMenu`
     组件，**从未** mount。`showContextMenu()` 一直在 update store，
     但没有任何 subscriber → 用户右键看不到任何菜单，"所有右键菜单
     失效"症状本因。
   - 在 modal 块里加 `<ContextMenu />` 单实例。
   - 新增 e2e 回归测试：右键 `.wf-workspace-tabs` 应渲染
     `[role="menu"]`。锁住，下次再被无意中拆掉立刻 fail。

2. **resolver class typo 修**
   - `getContextMenuTarget` 检查 `.wf-terminal` 但实际类是
     `.wf-terminal-surface`（Pane.svelte），改名匹配。
   - `.wf-editor` 不存在，删除该分支只留 `.monaco-editor`。
   - `SplitContainer.svelte` 的 leaf header 加 `wf-pane-header` 类，
     pane header 右键终于能命中正确 target（之前一直退到
     `pane-content`）。

3. **Explorer 移除 Claude 插件**（γ）
   - `plugins/index.ts` 删除 `claudeHistory` 的 register/unregister
     逻辑 + settingsStore subscribe。Claude UI 现在仅活在 round-27
     的 `ClaudeCodePanel.svelte` 独立 tab 里。
   - `ClaudeHistoryPanel.svelte` 文件保留（无 register 等于零运行时
     成本），便于未来"用户自定义 plugin"的演示。
   - Explorer / FileTree / SidebarPluginRegion 内 grep 确认无遗留
     Claude reference。

4. **回归全通**
   - `pnpm check` **0 / 0 / 0** · `vitest` **96 / 96**
   - `cargo test` **68 / 68** · `pnpm e2e` **5 / 5 (11s)**（+1 contextmenu）

---

### 第 33 轮（2026-04-25 11:43）— 统一 WindDialog 替换原生 prompt/confirm/alert

1. **`WindDialog.svelte`（new，全局单实例）**
   - 模块级 promise API：`alertDialog(opts)` / `confirmDialog(opts) →
     boolean` / `promptDialog(opts) → string|null`。
   - 队列：多个并发 open 排队，前一个 resolve 后下一个 pump 出来。
   - 主题化：z-9998 modal slot，danger=true → 红色 OK 按钮，icon
     prefix（AlertTriangle）。
   - Esc cancel + Enter confirm + IME composition guard（review HIGH 修
     —— 用户用中文 IME 时按 Enter 选候选词不应误提交）。
   - tick 后自动 focus 输入框（prompt）或 OK 按钮（confirm/alert）。
   - 背景点击 cancel —— 但 prompt 已输入内容时不响应背景点击，避免
     误点丢失输入（review LOW 修）。
   - `_resolveCurrent` 不导出，instance 脚本通过模块作用域调用，避免
     外部双 resolve（review MEDIUM 修）。

2. **6 处 native dialog 迁移**
   - SourceControl 右键菜单：分支创建 prompt + checkout-detached / revert
     confirm + 冲突 abort confirm + 复制失败 alert。
   - MarkdownPreview 链接 host trust prompt → confirmDialog（与第 23
     轮的 trust prompt 同款问题一并修）。
   - Explorer：删除工作区文件 confirm + 失败 alert。
   - FileTree：删除文件 confirm + 部分失败 alert。

3. **+page.svelte 全局挂载**
   - 与 ScrollbackHistoryModal / DiffEditorModal / ClaudeAgentLauncher
     一起 mount 一次，z-9998 modal registry 加新 entry（CLAUDE.md
     待补）。

4. **回归全通**
   - `pnpm check` **0 / 0 / 0** · `vitest` **96 / 96**
   - `cargo test` **68 / 68**（未触碰后端）· `pnpm e2e` **4 / 4 (9s)**

5. **review 留给后续轮**
   - **LOW** 仍有 8 处 bare alert/confirm 未迁移（ClaudeAgentLauncher
     启动失败、FileTree 拖放失败、Explorer 粘贴失败等）。属于"非阻塞
     性提示"，可以批量在 P0-I 一并清。

---

### 第 32 轮（2026-04-25 11:35）— commit 右键菜单 + 冲突恢复路径 + 杂项

1. **后端 commit 操作**（P0-G 阶段二）
   - `git_cherry_pick(repo_root, hash)` —— `git cherry-pick HASH`，
     非空 hash 校验，stderr/stdout fallback。
   - `git_revert(repo_root, hash)` —— `git revert --no-edit HASH`，
     同上 error shape。

2. **冲突恢复机制**（review HIGH 修）
   - `GitOpInProgress` 结构 + `git_op_in_progress` 命令：探测
     `.git/CHERRY_PICK_HEAD` / `.git/REVERT_HEAD` / `.git/MERGE_HEAD` /
     `rebase-apply|merge` 目录，告诉前端当前是否处于暂停态。
   - `git_cherry_pick_abort` / `git_revert_abort` —— `git ... --abort`
     恢复工作树。
   - 前端 `runCommitOp` 在 catch 路径里调 `git_op_in_progress`：发现
     mid-op 时把 alert 升级成 confirm，"要 abort 并恢复吗？"，确认后
     直接发起对应 abort 命令。Always-finally 重新 loadGraph +
     refreshStatus，让"半应用"状态用户立刻看见。

3. **commit 行右键菜单**
   - `oncontextmenu={(e) => onCommitContextMenu(e, c)}` 触发；
     showContextMenu(target='git-graph') 已有 z-index 9999 + keyboard
     nav。
   - 菜单：复制短 hash / 复制完整 hash / [---] / 从此 commit 创建分支
     （window.prompt name → git_checkout create:true base:hash）/
     Checkout (detached, confirm) / [---] / Cherry-pick / Revert
     (confirm)。
   - 打开菜单同时 `selectedCommitHash = c.hash`，让用户视觉确认目标。

4. **clipboard 容错**（review MEDIUM 修）
   - `copyToClipboard(text, label)` 帮手：缺 API / 写失败都 alert，
     不再吞掉。

5. **未知 ref 形态 fallback**（round-31 LOW 修）
   - 模板 `{:else}` 分支：渲染原文 ref 串到灰色 pill，title=ref。
     未来 git 出新装饰格式不会再被默默丢。

6. **回归全通**
   - `pnpm check` **0 / 0 / 0** · `vitest` **96 / 96**
   - `cargo test` **68 / 68**（未触碰已有测试）· `pnpm e2e` **4 / 4 (8s)**

7. **review 留给下轮**
   - **HIGH** `window.prompt` / `window.confirm` 用 3 处（branch name
     创建 + checkout-detached + revert）—— 与 round 22 SCM branch picker
     的 inline-create UX 风格不一致。下一轮统一换成 inline modal
     或自家 `<Dialog>` 组件（也覆盖第 23 轮 markdown 链接 trust prompt
     的同款问题）。
   - **LOW** commit 行无 `Shift+F10` / `ContextMenu` 键打开菜单的
     keyboard 路径；ContextMenu 内部已支持键盘 nav，仅缺 row 的入口。

---

### 第 31 轮（2026-04-25 11:25）— 图谱 ref 装饰 + commit 选中 + 关键 git_log 修复 + md 预览滚动

1. **`CommitNode.refs: Vec<String>`**（P0-G 阶段一）
   - 新增字段；用 `git log --decorate=full %D` 拿装饰，`parse_decorations`
     转成 `head:` / `branch:main` / `branch:origin/main` / `tag:v1.0`
     结构化串。5 cargo 单测覆盖空/HEAD-only/HEAD+branch/branch+tag+remote/
     未知形态。

2. **SCM commit 行 ref pills + 选中态**
   - 行内 `{#each c.refs}` 渲染：HEAD = amber pill / 本地分支 = emerald /
     远程分支 = blue / tag = violet w/⛳。
   - `selectedCommitHash $state` + 点击/Enter 切换；`role=button` +
     tabindex=0 + `e.target===e.currentTarget` 守卫。

3. **🔥 review HIGH 修复（3 个真实 bug，影响图谱可见性）**
   - 旧 `parse_git_log` 的 `output.split("%n")` 是字面串 split，但 git
     把 `%n` 展开成 `\n`——分隔符永远 match 不到，整个输出当 1 个 commit
     处理。改用 `\x1e` (RECORD SEPARATOR) 真实控制字符。
   - 旧 `--format=format:" + format"` 拆成两个 argv 元素，git 把第二个
     当 revspec → 整个 output 空白。合成单一
     `--pretty=format:%H...{RECORD_SEP}` 参数。
   - 字段分隔符从 `|` 换成 `\x1f` (UNIT SEPARATOR)：`user.name = "A|B"`
     不再让 `parts[5]` 错位。新增 `parse_git_log_handles_pipe_in_author_name`
     单测锁定。

4. **review MEDIUM 修复**
   - `selectedCommitHash` 在 `loadGraph` 起始处清空 —— refresh / commit /
     rebase 后旧 hash 不再 hover 当前列表，下一轮的右键菜单不会指向
     不存在的 commit。

5. **md 预览滚动 fix**（用户实时反馈）
   - `FileEditor.svelte` 的 markdown 预览 wrapper 之前用
     `use:overlayScroll` + `absolute inset-0`，overlayscrollbars 注入的
     synthetic viewport 在绝对定位 host 下没有稳定 height，wheel 滚动
     失效。换成原生 `overflow-y-auto + wf-scroll` —— 与其它 sticky-rail
     区域保持一致，scroll 行为确定。

6. **回归全通**
   - `pnpm check` **0 / 0 / 0** · `vitest` **96 / 96**
   - `cargo test` **68 / 68**（+6：5 decoration + 1 pipe-in-author）
   - `pnpm e2e` **4 / 4 (9s)**

7. **review 留给下轮**
   - **LOW** 未知 ref 形态前端 `{#if/else if}` 链没有 fallback，会被
     默默丢弃（虽然 backend 注释说"keep visible"）。下一轮加 `{:else}`
     渲染原始字符串。

---

### 第 30 轮（2026-04-25 11:13）— 真正的 SVG 分支图谱（替换扁平 commit 列表）

1. **`gitGraphLayout.ts`（new，纯 TS）**
   - 核心算法：维护 `lanes: (string|null)[]`，每个 commit 找/分配 lane，
     发出 dot，把 lane 替换为第一个 parent；其余 parent 走新 lane + 三次
     bezier 曲线（matches `git log --graph` 视觉）。
   - 8 色 palette + `colorForHash`：commit hash 前 6 hex → palette index，
     同分支不论滚动/合并都保持同色。
   - GC：trailing-null lanes 压缩，避免宽度漂移。
   - 导出 `DEFAULT_DX/DY/PAD_X/PAD_Y` 常量——SCM 端用 `DEFAULT_DY` 做行
     高，**单一来源**，dot 与 text row 永远不会失同步（review HIGH 修）。
   - `LayoutOutput.totalHeight` 修正旧 SVG height 计算：旧 `n*dy` 在
     `padY > dy/2` 时会裁掉最后一个 dot 的下半部，新 `padY*2 +
     (n-1)*dy` 始终留满 padding（review MEDIUM 修）。

2. **`GitGraph.svelte`（rewritten，原文件是 orphan canvas widget）**
   - 几十行 SVG renderer：`<path>` 划线 + `<circle>` 划点（line 先画，
     dot paint 在上层）；`aria-hidden`，纯装饰。
   - props 改用 `DEFAULT_DX/DY` 默认值。

3. **`GitGraph.test.ts`（new，8 vitest cases）**
   - 线性链 lane 0 / merge 开新 lane / 曲线 path / freed lane 复用 /
     色彩确定性 / 空输入 / **每 commit 唯一 dot key 不变性**（locks
     review HIGH-2 latent risk）/ totalHeight 在大 padY 下覆盖最后 dot。

4. **`SourceControl.svelte` 接入**
   - "提交记录" 面板 → "图谱"。原 3 行 commit row 块换成 `<GitGraph>` +
     flex 兄弟单行 row：branch tag + subject + short hash + author。
     row 高 inline `style="height: ${GRAPH_ROW_HEIGHT}px"`，常量从
     `gitGraphLayout` 导入。

5. **回归全通**
   - `pnpm check` **0 / 0 / 0** · `vitest` **96 / 96**（+8 GitGraph）
   - `cargo test` **62 / 62**（未触碰后端）· `pnpm e2e` **4 / 4 (45s)**

6. **review 留给后续轮**
   - **MEDIUM** `lanes.indexOf` O(n)；50 commit cap 下不可见，未来加
     pagination 时换 `Map<hash, laneIdx>`。
   - **LOW** parity gap：HEAD marker / tag labels / commit 选中/右键
     cherry-pick / revert 都还没；这些是独立功能点，挂到下一轮 P0-G。

---

### 第 29 轮（2026-04-25 10:59）— SCM untracked 点击 + Search 非法 glob 装饰

1. **SCM untracked file rows 接 diff editor**
   - 加 `cursor-pointer` / `role=button` / `tabindex=0` / `onclick=
     showDiff(root, path, false)`，进新 Monaco DiffEditorModal —— 后端
     `git_get_file_versions` 已支持空-original 场景，渲为整文件加号 diff
     （VS Code 的 "U" 文件 diff 行为）。
   - Stage 按钮加 `e.stopPropagation()`，避免点击 Stage 同时触发行 click
     打开 diff modal。
   - 三个 row 的 onkeydown 都加 `e.target === e.currentTarget` 守卫，
     防止焦点在 Stage 按钮时按 Enter 既触发 stage 又触发 diff —— review
     HIGH 修复。

2. **SearchSidebar 非法 glob 红圈装饰**
   - 新增 `InvalidGlob` interface + `invalidGlobs $state` + 两个
     `$derived`（include/excludeGlobErrors）。
   - `runSearch` 与 per-root search 并发调 `text_search_diagnostics`，
     非致命（catch → 空数组）。Promise 在最后 await，主结果先到 UI。
   - 两个 glob input 在有错时切到 `border-rose-500/60 ring-1
     ring-rose-500/30`，title 列出每个 pattern → error；正常态保留
     原 accent border。
   - 空查询时清空 `invalidGlobs` —— review HIGH 修复，否则红圈永远
     不退场。

3. **回归全通**
   - `pnpm check` **0 / 0 / 0** · `vitest` **88 / 88**
   - `cargo test` **62 / 62**（未触碰后端）· `pnpm e2e` **4 / 4 (45s)**

4. **review 待办留给后续轮**
   - **MEDIUM** SearchSidebar 注释说"parallel"但 per-root 是串行；
     diagnostics await 应该在 dedup 之前就 surface，避免大型 monorepo
     下用户先看 results 几秒后才看到红圈。
   - **LOW** `compileGlobList` 客户端 `new RegExp` 错误也静默丢；
     注释说明它和 backend 诊断的非对称是有意的。

---

### 第 28 轮（2026-04-25 10:51）— review 待办大清单（5 个 MEDIUM/LOW 一次扫干净）

1. **`git_sync` upstream 检测改用 porcelain 解析**（review MEDIUM）
   - 旧逻辑：fetch/pull/push 失败后用 `err.contains("no upstream")` 字符串
     嗅探返回友好提示——locale 一变（`LC_ALL=zh_CN.UTF-8`）就失效。
   - 新逻辑：开头先跑 `git status --porcelain=v1 -b --untracked-files=no`，
     用现成的 `parse_porcelain_v1` 提取 `has_upstream`。无 upstream 直接
     返回中文友好错，跳过 fetch/pull/push 三步，节省 3 个 spawn。

2. **`fs/search.rs` 非法 glob 错误回传**（review MEDIUM）
   - 新增 `InvalidGlob { pattern, error, field }` 结构。
   - `search_text_with_globs` 收集而非吞掉 `Pattern::new` 错误；旧
     `search_text` 保留兼容（丢弃后只回 results）。
   - 新命令 `text_search_diagnostics(includeGlobs, excludeGlobs)` 仅
     做解析返回 `Vec<InvalidGlob>`，给前端独立调用以装饰输入框（红圈
     + tooltip），与 VS Code `files.exclude` 失败提示同模式。

3. **DiffEditorModal 三处打磨**（review MEDIUM/LOW）
   - reload 时 `await tick()` 在 `createDiffEditor` 之前——保证 modal
     flex 至少 layout 过一帧再让 Monaco measure host，否则有 size=0 的
     时序窗口。
   - 不再把 `renderSideBySide` 传进 createDiffEditor 选项；只走专门的
     `updateOptions` $effect。toggle inline ↔ side-by-side 不再触发后端
     reload。
   - error 状态下 host div 加 `visibility:hidden`——refresh-after-error
     不会再让旧错误浮层与新编辑器重叠。

4. **`settings.ts` 单 key runtime 类型校验**（review LOW）
   - `load()` 改为对每个已知 key 单独类型 check：
     `claudeExtensionEnabled` 必须是 boolean；否则该 key 落回默认值，
     不再 spread Partial 让 `"yes"` 字符串污染。
   - 不上 zod——单 key 不值得 dep；注释里挂"扩到 3-4 个字段时升级"。

5. **`plugins/index.ts` 注释升级**（review LOW）
   - 把 init-order invariant 写明白：settingsStore 是同步 load，
     subscribe 立即首发，所以首次回调拿到的就是 persisted 值；
     如果改成异步 load 会出现 register-then-unregister flicker。

6. **回归全通**
   - `pnpm check` **0 / 0 / 0** · `vitest` **88 / 88**
   - `cargo test` **62 / 62** · `pnpm e2e` **4 / 4 (45s)**

---

### 第 27 轮（2026-04-25 10:44）— Claude Code 扩展独立 tab + 全局开关

1. **`settings.ts` 新通用偏好 store**
   - `claudeExtensionEnabled` 持久化到 localStorage 的 `wind-settings`
     key（单一 JSON blob，原子读写，SSR-safe）。
   - 默认 true 不破坏现状；`setClaudeExtensionEnabled` 便捷写入。
   - 形状预留以容纳后续偏好（字体、主题、telemetry）。

2. **`ClaudeCodePanel.svelte` 独立 sidebar tab**
   - 左 rail 加第 4 个 Bot 图标按钮（gated on enabled）→ 切到该 tab。
   - 面板渲染当前活动工作区的所有 pane：agent_state badge（idle/busy/
     launching 三色 + busy pulse）、cwd preview、Play 按钮唤起
     ClaudeAgentLauncher、per-pane history（5 条 inline + 展开/收起 +
     清空）。
   - 头部 Settings dropdown："关闭 Claude Code 扩展" 一键禁用，
     mousedown capture + Esc 双路径关闭（mirror PaneGitPill 模式）。

3. **三处全局 gating**
   - `+page.svelte`：rail 第 4 按钮 + tab 体两处 `{#if
     $settingsStore.claudeExtensionEnabled}`。tab=='claude' 但开关 off
     时 fallback 到 'files'。
   - `SplitContainer.svelte`：pane 标题 Bot 按钮整段 gated。
   - `plugins/index.ts`：claudeHistory plugin 改为
     `settingsStore.subscribe` 驱动 register/unregister，运行时切换立即
     生效，无需重启。

4. **重启路径：rail 底部 `mt-auto` Settings 齿轮**
   - 始终可见（不论扩展开关状态）；点击直接 toggle 扩展开关。
   - on 时正常色，off 时 opacity-50 视觉提示。tooltip 同步说明当前
     状态 + 点击行为。

5. **review 修复（同轮内）**
   - **HIGH** ClaudeCodePanel 在 `{#each $workspacesList}` 里渲染
     `flattened`（仅活动工作区 pane）→ 多工作区时每个 ws header 下都
     出现同一份 pane 列表。改为只渲染活动工作区单 block；其他工作区
     仍可通过顶部 WorkspaceTabs 切换查看。
   - **HIGH** `handleOpenSidebarTab` 的 detail 白名单缺 `'claude'`。
     补上 + 同时 gate on 扩展开关。
   - **MEDIUM** Settings dropdown 没有 click-outside / Esc 关闭路径。
     补上 capture-phase mousedown + keydown 监听。

6. **review 待办留给后续轮**
   - **MEDIUM** 底部齿轮的 dual-semantics（图标含义 vs 实际 toggle 行为）
     —— 后续等真正的 settings panel 落地时统一改造。
   - **LOW** localStorage spread-over-defaults 没有 schema 校验；后续随
     `UserSettings` 扩字段时上 zod。
   - **LOW** plugins/index.ts subscribe 初始化顺序的注释还可更清晰。

7. **回归全通**
   - `pnpm check` **0 / 0 / 0** · `vitest` **88 / 88**
   - `cargo test` **62 / 62**（未触碰后端）· `pnpm e2e` **4 / 4 (47s)**

---

### 第 26 轮（2026-04-25 10:30）— Monaco DiffEditor 替换 `<pre>` modal

1. **后端 `git_get_file_versions`**（P0-D 后端）
   - 新命令返回 `{original, modified}` 两段 blob：
     - `cached=false`：original = `git show :<path>`（index blob），
       modified = 工作树文件磁盘读取。
     - `cached=true`：original = `git show HEAD:<path>`，modified =
       `git show :<path>`（已暂存版本）。
   - 任一侧文件不存在（新增 / 删除）→ 空字符串而非错误，前端按"新文件
     / 已删除"渲染。
   - **路径穿越防护（review HIGH）**：`fs::read` 之前 canonicalize 双侧，
     断言 target 仍在 repo 内，抵挡 `../../etc/passwd` 这类前端误传。
   - **二进制对称性（review HIGH）**：working-tree 侧改 `fs::read +
     from_utf8_lossy`，与 git show 侧一致——之前 `read_to_string`
     会在第一个非 UTF-8 字节 bail out，制造"git show 这边静默替代但 fs
     这边报错"的不对称体验。

2. **`DiffEditorModal.svelte`**（P0-D 前端）
   - 模块级 `openDiffEditor(args)` / `closeDiffEditor()`，单实例 mount
     在 `+page.svelte`，z-index 9998。SourceControl 调一行函数即可。
   - `monaco.editor.createDiffEditor`：readOnly + automaticLayout +
     `vs-dark` + renderWhitespace=boundary，与 FileEditor 视觉一致。
   - 头部 toggle：side-by-side / inline，`updateOptions` 切换不重建。
     默认 ≥900px 走 side-by-side，否则 inline——窄抽屉里用 inline。
   - 头部 Refresh / Esc / 背景点击关闭。`disposeEditor` 加 early-return
     避免 $effect cleanup + onDestroy 双触发。
   - `langFromPath` 从 `fileEditor.ts` export 出来复用，diff 与编辑器
     同一套语言推断。

3. **SourceControl 接入**
   - 旧 `<pre>` modal + `diffOpen/diffTitle/diffContent/diffLineClass`
     全部删除；`showDiff(root, path, cached)` 缩成一行 delegating call。

4. **回归全通**
   - `pnpm check` **0 / 0 / 0** · `vitest` **88 / 88**
   - `cargo test` **62 / 62** · `pnpm e2e` **4 / 4 (10s)**

5. **review 发现待办**（`code-reviewer` 第 26 轮）
   - **MEDIUM** `automaticLayout: true` 与 modal flex 容器有时序窗口；
     `await tick()` 在 `createDiffEditor` 前更稳。
   - **MEDIUM** untracked 文件点击 → 走 `cached=false` 路径会渲成"新增
     diff"，行为正确但当前 SCM 不给 untracked 行接 onclick；加注释或
     接上点击。
   - **LOW** `renderSideBySide` 首次还在 `createDiffEditor` 选项里，
     toggle 时第一个 $effect 也会重跑全 reload。改成只走 `updateOptions`
     不重建。
   - **LOW** error 状态下 Monaco host div 还挂着，refresh 命中错误时
     新编辑器会 mount 在错误 overlay 之下。错误时 `display:none` 把
     host 藏掉。
   - **LOW** VS Code 平价缺口：diff editor 没有 gutter 级 stage / unstage
     hunk。挂到 P3 的 SCM 体验打磨。

---

### 第 25 轮（2026-04-25 10:21）— SCM per-file +N -N + 顶部编辑器抽屉开关

1. **SCM 文件行 per-file +N -N**（P0-C）
   - 后端 `ScmFile` 加 `additions: u32` / `deletions: u32`（`#[serde(default)]`）。
   - 新增 `parse_numstat` 把 `git diff --numstat` 输出 → `HashMap<path,
     (added, removed)>`：处理 binary `-` 字面量（clamp 0）+ rename
     `old => new`（key=new）。3 个 cargo 单测覆盖。
   - `get_scm_status` 各跑一次 numstat（working tree + cached）回填到
     changes / staged；untracked 没有 diff 不填。这两次 git 调用是 O(1)
     而非 O(files)，比 modal 时代每个点击 spawn 一次便宜得多。
   - 前端 `ScmFile` interface 加可选字段；file row 在 ml-auto 区域用
     单 grid cell 叠 +N -N（默认显示）+ 操作按钮（hover 浮现）；行宽
     稳定（min-w-52px）防止 hover 抖动。

2. **顶部头部"展开文件编辑器"按钮 + 编辑器头部收起/关闭**（用户实时反馈）
   - `+page.svelte` 顶部栏 split-pane 按钮左侧加 `PanelRightOpen` 图标
     按钮，bound 到 `fileEditorStore.toggleVisibility()`；按钮高亮态跟
     `$fileEditorStore.isVisible`，title 同步 "展开/收起" 文案。
   - `FileEditor.svelte` 头部最左加 `PanelRightClose` 收起按钮（drawer
     和 floating 两种模式都常驻）；floating 模式下右侧 search 旁加 `X`
     关闭按钮（drawer 模式不重复，左收起足够）。
   - 两个按钮都加 `wf-no-drag` + `onmousedown stopPropagation` 防止 floating
     模式下被拖拽 handler 截胡。

3. **验证 pane git 按钮真实数据 + 非 git 仓库不显示**（用户复述确认）
   - `paneGitStatus.ts` 链路：`find_git_repo_root` → null 则全链路返回
     null → `_store[paneId] = null`。
   - `PaneGitPill` / `PaneDiffPill` 两个组件最外层都 `{#if info && info.branch}`
     gate，没仓库时整个按钮不渲染。
   - SplitContainer 没有任何独立的 git 字段，全部走两个 pill 组件。
     无 mock 数据。

4. **回归全通**
   - `pnpm check` **0 / 0 / 0** · `vitest` **88 / 88**
   - `cargo test` **62 / 62**（+3 numstat）· `pnpm e2e` **4 / 4 (9s)**

---

### 第 24 轮（2026-04-25 10:08）— pill 拆分 + SCM 滚动分层 + 图谱诚实标签

1. **PaneGitPill / PaneDiffPill 拆分**（P0-B）
   - 新增 `PaneDiffPill.svelte`：渲染 file count + +N -N（不含 ahead/behind，
     这俩留在分支 pill）；点击发 `wind:open-sidebar-tab=git` +
     `wind:scm-focus-repo` 双事件。clean (0/0/0) 状态用降低对比度的灰色，
     dirty 状态走 accent。
   - `PaneGitPill` 移除 dirty/+N -N 渲染，保留 branch + ahead/behind +
     upstream-warn。tooltip 同步缩短。
   - `SplitContainer` 在 PaneGitPill 后挂 PaneDiffPill。
   - `SourceControl` 新增 `wind:scm-focus-repo` 监听：用
     `[data-wf-scm-repo]` 找仓库 → 展开三个 group → scrollIntoView →
     1.5s `.wf-scm-flash` 高亮（CSS keyframe）。仓库未渲染时用 250ms
     退避重试 8 次（≤2s）兜住"刚切到 SCM tab，discoveryRepos 还没回来"
     的 race。

2. **SCM 滚动分层 — 仓库头 + group 头双层 sticky**（用户 +1 反馈）
   - 仓库头加 `sticky top-0 z-30 backdrop-blur-md`：滚动正文时仓库名 +
     分支 picker + sync 按钮始终钉在 viewport 顶部。
   - 三个 group sub-header（已暂存 / 更改 / 未跟踪）加
     `wf-scm-group-sticky` 类（sticky top-29px z-20）：被仓库头压在下面，
     与 Explorer 的两层 sticky 同思路（workspace top-0 z-20 + cwd
     top-8 z-10）。
   - 所有 sticky 都用 `var(--wf-surface-2)/92 + backdrop-blur-md`，与
     Explorer 视觉一致。

3. **"图谱" → "提交记录" 诚实标签**（用户 +1 反馈）
   - 当前面板只渲染线性 commit list（hash + 分支标签 + subject + 作者/
     日期），不是真正的 graph。先把标题改为更准确的"提交记录"，title
     里加注释说真图谱见后续轮次，避免给用户错觉。
   - 真正的分支线条 + merge dot 渲染留作 P0-F 独立轮次（候选库：
     `@gitkraken/gitgraph-js`、`gitgraph.js`，或自研 canvas）。

4. **回归全通**
   - `pnpm check` **0 / 0 / 0** · `vitest` **88 / 88**
   - `cargo test` **59 / 59**（未触碰后端）· `pnpm e2e` **4 / 4 (8s)**

---

### 第 23 轮（2026-04-25 09:39）— text_search globs / pill upstream 提示 / md 外链 host 信任

1. **后端 `text_search` 接 include/exclude globs**
   - `SearchOptions` 新增 `include_globs: Vec<String>` 和 `exclude_globs:
     Vec<String>`，`#[serde(default)]` 兼容旧 payload。
   - 在 `WalkDir` 阶段就过滤路径（include 必匹配 / exclude 命中即跳过），
     避免对大量文件做读取 + binary 检测；前端原本要拉到全量再 JS 端筛。
   - matches 同时跑 OS-sep（Windows 反斜杠）和 forward-slash 归一版本，
     用户可写 `src/**/*.ts` 跨平台。
   - 解析失败的 glob 静默丢弃（typo 不应让整轮搜索 error out）。

2. **`PaneGitPill` upstream 缺失提示** _(P1-5)_
   - `parse_porcelain_v1` 改为返回 `(branch, ahead, behind, has_upstream,
     staged, changes, untracked)`：用 `splitn(2, "...")` 检测 head 后是否
     真的有 upstream segment（`## main` / `## main...` 都算 false）。
   - `ScmRepoStatus` 新增 `has_upstream: bool` (`#[serde(default)]`)；
     `PaneGitInfo` 新增 `hasUpstream: boolean`。
   - `PaneGitPill` 在 ahead/behind 数字旁额外渲染琥珀色 `↑↓?` 标记，
     按钮 title 增加“⚠ 当前分支没有 upstream”一行。
   - 5 个 cargo 单测覆盖：`main...origin/main` 有 upstream / 仅 `main` 无 /
     `feature/x...` 末尾空 rhs 无 / ahead/behind 解析正确 / detached HEAD
     无 upstream。

3. **Markdown 外链 host 首次打开确认** _(P3-9)_
   - 新增 `src/lib/utils/linkTrust.ts`：模块作用域 `Set<string>` 维护本次
     会话的 trusted hosts；`hostKeyFromUrl` 把 `www.example.com` 与
     `example.com` 视为同一 key、不同子域（`api.github.com`）独立记账。
   - `MarkdownPreview.openExternal` 在打开前调 `isTrustedUrl`；首次命中
     `window.confirm()` 询问 host + URL，确认后 `trustHostFromUrl` 加入
     trusted，本次会话内同 host 不再问。
   - `mailto:` / `tel:` 跳过 prompt（OS 端有自己的 picker），无效 URL
     返回 false。
   - 9 个 vitest 覆盖 host 归一 / 子域隔离 / mailto-tel 直通 / 无效 URL。

4. **回归全通**
   - `pnpm check` **0 / 0 / 0** · `vitest` **88 / 88**（+9 linkTrust）
   - `cargo test` **59 / 59**（+5 porcelain）· `pnpm e2e` **4 / 4 (9s)**

---

### 第 22 轮（2026-04-25 09:02）— git pill base ref / SCM 联动 / modal 复制

1. **`PaneGitPill` 创建分支可选 base ref**
   - 后端 `git_checkout` 加可选 `base` 参数：非空时 `git checkout -b
     <name> <base>`，空 / 缺省 = 沿用旧 HEAD 行为；trim + ignore 空白
     base 让前端默认空就 work。
   - 前端 inline create 行从单 line 改成两行：第一行新分支名，第二行
     "基于：<select>" + Enter ↵ 提示。`<select>` 默认 `HEAD（当前）`，
     option 列表用已经加载好的 `branches`。
   - 移除 input 上的 `onblur=cancelCreate` —— 否则点 select 会取消。
     依赖 PaneGitPill 自身的全局 mousedown handler 和 Esc keydown 关闭。

2. **SCM 写操作联动 `paneGitStatus` 失效**
   - `SourceControl.refreshStatus(root)` 内部新增
     `void invalidatePaneGitStatusForRepo(root)`。
   - 所有 stage / unstage / discard / commit / fetch / pull / push / sync
     最后都会调 refreshStatus，自动级联到 pill 数字刷新。
   - 之前要等 cwd change 触发；现在 SCM 操作完成就同步更新。

3. **`ScrollbackHistoryModal` 复制全部按钮**
   - Header download 旁加 Copy 图标按钮，调
     `navigator.clipboard.writeText(cleaned)`，1.5 s 切换为 emerald Check
     做 "已复制" 视觉反馈。
   - clipboard 在 Tauri webview / dev localhost 都是 secure context，无 fallback。
   - 错误也 alert，与现有写操作一致。

4. **回归全通**
   - `pnpm check` **0 / 0 / 0** · `vitest` **79 / 79**
   - `cargo test` **54 / 54** · `pnpm e2e` **4 / 4 (8s)**

---

## ⚠️ 下一轮候选

### P0 — 大项

#### 用户 2026-04-25 11:43 新增任务（下一轮处理）

α. **所有右键菜单不可用** ✓ 第 34 轮已交付（根因：ContextMenu 组件
   import 但从未 mount——store 永远没 subscriber）。
   _（旧排查清单保留以供未来类似问题对照：）_
   _所有右键菜单不可用_ — 用户报告 ContextMenu 无法触发或交互失败。
   需要排查范围：
   - workspace tabs / pane header / terminal / editor 各个 target 的
     右键 wiring；
   - PreToolUse `contextmenu` event 是否被全局 preventDefault 截走又
     没分发到 ContextMenu store；
   - `isResizeInProgress()` 卡死状态导致永远 return 忽略；
   - ContextMenu 组件 mount 状态（被 v-if 误隐藏？）；
   - 第 32 轮新加的 commit row `oncontextmenu` 是否能触发；
   - 新加 WindDialog z-9998 与 ContextMenu 9999 是否抢焦点。
   先做端到端 e2e + 现场冒烟，再定位具体 broken site。

β. **所有 mock 数据展示替换为真实数据** — 全仓 grep `mock` /
   `placeholder` / 假数据：
   - `ClaudeCodePanel` 当前显示活动工作区 panes（实数据），但 history
     的 prompt 计数其实是 localStorage 缓存——需要确认是否还有静态
     fallback；
   - `WorkspaceSummaryPanel` / `GlobalStatusPanel` plugin 内是否有
     stub 数据；
   - `Dev Issue` / `MoreHorizontal` 菜单是否塞了占位项；
   - SCM 上次 sync 时间 / Claude 状态 badge 等小角色文案有没有
     硬编码；
   - 所有 `TODO` / `FIXME` / `// mock` 注释扫一遍。

γ. **Explorer 内移除所有 Claude 相关内容** ✓ 第 34 轮已交付。
   _（旧设计说明保留：）_
   _Explorer 内移除所有 Claude 相关内容_ — 第 27 轮把 Claude Code
   提到独立 tab 后，`claudeHistory` plugin 还以 scope='pane' 形式
   挂在 Explorer 每个 pane 列下面。用户要求 Explorer 完全干净，
   Claude UI 只活在 Claude Code tab 里。
   - `plugins/index.ts` 把 `claudeHistory` plugin 整段删除（不再 register/
     unregister）。
   - 把 plugin component `ClaudeHistoryPanel.svelte` 的功能合并到
     `ClaudeCodePanel.svelte` 的 per-pane 行（如果还有缺的功能）。
   - 顺手 audit `SidebarPluginRegion.svelte` 的 scope='pane' 是否还有
     其它 Claude-related plugin。
   - 重新跑 e2e 确认 Explorer 无 Bot 图标 / Claude 历史块。

ε. **SCM tab 缓存 + 无感刷新** ✓ 第 42 轮交付了 MVP（模块级
   `scmCacheStore` 让 SourceControl 卸载不丢数据 + 30s 内信任
   缓存 + 后台 refresh），真正的 `notify` 监视器留作阶段二。
   _（旧设计说明保留：）_
   - 当前每次切到源代码管理 tab 触发 `discoverRepos` + 全量
     `refreshStatus`——大仓库时切 tab 卡顿明显。
   - 思路：
     - 在 store 里保留上次的 `repoRoots` + `statuses` snapshot；切回
       时直接显示，再后台跑 invalidation；
     - 用 `notify` crate / 文件系统监视器订阅 `.git/HEAD`、
       `.git/index`、工作树 mtime 变化，事件驱动 invalidate（替代
       cwd-store-subscribe 的"任何 cwd 变就扫"模式）；
     - SCM tab mount 不再触发 `discoverRepos`，由文件事件触发；
   - 验证：用户感知"切 tab 即出，且数据始终新鲜"。

ζ. **全应用右键菜单系统化**（用户 11:59 反馈）
   - 第 34 轮修了"菜单不显示"，但很多 target 还没接菜单 / 菜单项
     设计粗糙。把所有右键 target 系统化：
     - **terminal**：复制 / 粘贴 / 选中 / 清屏 / 字体大小 /
       split / 关闭；
     - **editor**（Monaco）：复制 / 粘贴 / Go to definition（如果
       有 LSP）/ 折叠 / 拆分编辑器；
     - **pane-header**：分屏 / 关闭 / 重命名 / 复制 cwd / 在文件
       管理器打开；
     - **pane-content**（终端工作区背景）：开新 pane / 粘贴上次
       命令 / 隐藏其它 pane；
     - **sidebar workspace 行**：保存 / 删除 / 重命名 / 关闭；
     - **sidebar 文件行**：所有 FileTree 现有右键 + 在新 pane
       打开 / 复制路径；
     - **SCM 文件行**：stage / unstage / discard / open changes /
       open file / 历史；
     - **commit row**（已有，第 32 轮）：保留 + 加 "show this commit
       file changes" / "compare with this";
     - **WorkspaceTabs 标签页**：复制 / 关闭其它 / 重命名 / pin。
   - 现状审计：grep `getContextMenuItems` 的 switch 哪些 case 是空的。

ο. **commit 行 ref pills 宽度不够时折叠成数字气泡 + menu**
   （用户 12:54 反馈）
   - 当前 commit row 把 `c.refs` 全部 inline 渲染（HEAD pill / branch
     / branch:remote / tag）。多分支 + tag 同 commit 时会撑爆 row，
     subject 被 truncate 得很惨。
   - 设计：固定显示**前 N 个 ref pill**（N 视宽度自适应，先按固定
     N=2 实现），剩余压缩成 `+M` 数字气泡。点击气泡 → 用现有
     `showContextMenu` 弹出菜单，列每个被折叠的 ref（按类型分组：
     HEAD → branches → tags），点击单项可触发 checkout / 跳转等
     （阶段一只展示 + 复制名字，行为留给阶段二）。
   - 实现位置：`SourceControl.svelte` 的 commit row `{#each c.refs}`
     块改成"分头几个 + 折叠 +M"。或抽出小组件
     `CommitRefPills.svelte`，保留 row 模板清爽。
   - 注意：HEAD 永远展示在前（pin），不可折叠——它语义最重要。
   - 数字气泡颜色：用中性灰 + hover accent，不要混进 head/branch/tag
     的语义色，避免误读。
   - 自适应 N：进阶版可监听 row resize / row 宽度，按可用宽度计算
     可塞下几个 pill；MVP 先 N=2 + tooltip 列折叠项，足够好用。

ξ. **`overlayScroll.ts` action 拓展 + WorkspaceTabs 水平滚动适配**
   ✓ 第 41 轮已交付（preset 形参 + horizontal-tabs preset + shift+wheel
   handler）。
   _（旧设计说明保留：）_
   - 第 38 轮把 WorkspaceTabs 改成 `use:overlayScroll` + `{x:'scroll',
     y:'hidden'}`，但用户报告水平展示/滚动还有问题。可能症状：
     - 横向滚动条在 flex-row 容器里没正确显示；
     - 拖动 tab 时滚动条与 sticky 头部重叠；
     - autoHide 让用户找不到滚动状态；
     - 默认 theme `wf-os-theme` 是为竖向调的，横向 thumb 太短/太细。
   - **action 拓展点**：`overlayScroll.ts` 加一个 preset 形参，比如
     `use:overlayScroll={{ preset: 'horizontal-tabs' }}`，内部展开成
     `{x:'scroll', y:'hidden', scrollbars: {theme:'wf-os-theme-h',
     autoHide:'never'}}` 等。当前调用方每个都要拼一坨 options，
     重复且易写错；preset 让常见场景一行搞定。
   - 现有 callsites 检查：Explorer / SourceControl / SearchSidebar /
     ScrollbackHistoryModal / FileEditor markdown / WorkspaceTabs ——
     看哪些适合切到 preset。
   - WorkspaceTabs 现状审计：
     - 是否真的触发横向溢出（tabs 总宽 > container？min-w-0 是否
       让 flex-1 真正生效？）；
     - shift+wheel 横向滚是否和 ζ 轮 commit message 那个 onwheel
       handler 协调一致（或全交给 overlayscrollbars 处理）；
     - tabs 底部是否需要给滚动条留出 padding（避免覆盖最后一行 tab
       的下边框）。
   - **第 41 轮做这个**——纯前端 + 1 个 ts 文件 + 1 个 .svelte 改动，
     量级小但可见性高。

λ. **SCM 未跟踪栏 stage-all 按钮** ✓ 第 39 轮已交付。
μ. **commit message Shift+wheel 横向滚动** ✓ 第 39 轮已交付。
ν. **sidebar 最大宽度 80%** ✓ 第 39 轮已交付。

_（旧设计说明保留：）_
λ. **SCM 未跟踪栏也加一个"全部添加 (stage all)"按钮**（用户 12:25
   反馈）
   - 当前 staged / changes group 的 sub-header 都有 hover-show
     "全部" 按钮（changes 是 + plus icon stage all、staged 是 − minus
     icon unstage all）。
   - untracked group 缺这个 batch 按钮，每个文件得单独点 +。
   - 加按钮：调 `stage(root, s.untracked.map(f => f.path))`，与
     changes 的 stage-all 共用一份函数。
   - hover-show + opacity-0 → group-hover/grp:opacity-100 visual
     pattern 保持。

μ. **图谱 commit message 部分支持 Shift + 鼠标滚轮 横向滚动**（用户
   12:25 反馈）
   - 长 commit message 当前 truncate 隐藏，用户希望能 shift+wheel
     横向滚（不破坏总体竖向滚动 UX）。
   - 实现思路：commit row 的 `<span class="truncate flex-1 ...">`
     改成自身可横向滚动的容器（`overflow-x-auto whitespace-nowrap`），
     加 `onwheel` 处理：`e.shiftKey` 时把 deltaY 转成 deltaX 平移。
   - 或更通用：在 SCM 图谱面板的滚动容器加监听 —— shift+wheel 时把
     deltaY 给当前 hover 的 commit message 元素。
   - 注意不要影响 overlayscrollbars 的 wheel 行为；overlayscrollbars
     一般会让原生 wheel 透出。

ν. **侧边栏最大宽度 = Wind 窗口的 80%**（用户 12:25 反馈）
   - 当前侧边栏 resize 上限是 `windowWidth40`（40% 宽，硬编码于
     `+page.svelte` line ~145）。用户希望可以拖到 80%。
   - 改：`windowWidth40` 重命名 → `sidebarMaxPx`，公式
     `window.innerWidth * 0.8`；resize handler 上限同步。
   - 注意：80% 是上限不是默认；现有持久化 `sidebarWidth` 不变。
   - window resize 时也要重算（否则缩小窗口后 sidebar 残留过宽）。

ι. **WorkspaceTabs 改用模拟滚动条 (overlayscrollbars)** ✓ 第 38 轮
   已交付。
   _（旧设计说明保留：）_
   - `WorkspaceTabs.svelte` 当前用 `wf-scroll`（webkit thin scrollbar）
     做横向溢出滚动；用户希望和 Explorer/SCM 一样统一为
     overlayscrollbars 浮层。
   - 用现有 `use:overlayScroll` action，pass options
     `{ overflow: { x: 'scroll', y: 'hidden' } }`（横向溢出）。
   - 注意 wf-no-drag / data-tauri-drag-region 的位置——overlayscrollbars
     注入新元素不能落在拖拽区里。

κ. **项目全局拖拽功能当前都不可用** ✓ 第 38 轮已修复（根因：根 div
   持有 `data-tauri-drag-region` 把整个窗口都标成 OS-window 拖拽区，
   Tauri 在 mousedown 吞掉事件让 HTML5 DnD 永远 dragstart 不到）。
   E2e 锁住未来再被加上立刻 fail。
   _（旧排查清单保留：）_
   - 用户报告整个项目所有 drag-and-drop 都失效。涉及面：
     - WorkspaceTabs reorder（draggable + ondragstart/over/drop）
     - SplitContainer pane drag（pane title bar → 拖到其他 pane 互换/
       靠边分屏）
     - FileTree DnD（拖文件移动/复制 + auto-expand-on-hover）
     - FileEditor tab reorder（draggable）
     - Explorer drag-from-FileSystem（外部文件拖入打开）
   - 排查思路：
     - 是不是某次给 root `<div>` 加了全局 `dragstart preventDefault`？
     - `data-tauri-drag-region` 与 HTML5 dnd 是否有冲突（已知 Tauri
       拖拽区会"吃掉"鼠标事件）？
     - 第 34 轮 `<ContextMenu />` 全局 mount + 全局 `contextmenu`
       handler 是否拦截了 mousedown 链？
     - 第 33 轮 WindDialog 的 `onclick={onCancel}` 背景层是否在某些
       情况下挂着拦截？
     - 是不是 dropEffect / effectAllowed 设置错位？
   - 验证：每个 DnD 场景写 1 个 e2e 用例锁住，未来再坏立刻 fail。

θ. **pane git pill 改用"cwd 及子目录中的 git 仓库"语义 + 多仓库
   切换器** ✓ 第 40 轮已交付（4 个新 vitest case 锁住，新
   `PaneRepoSwitcher.svelte` 仅在 N>1 时渲染）。
   _（旧设计说明保留：）_
   - 当前：`find_git_repo_root` 沿目录树**向上**走找 `.git`——这是
     git 标准语义（你 cd 进 repo 子目录，git 工具仍把你当作在 repo
     里）。但用户的心智模型不一样：cwd 是"容器"，git pill 应该
     反映"cwd 内部"的 git 仓库（VS Code multi-root 的概念）。
   - 新语义：**只在 cwd 自身 + 直接子目录**找 `.git`：
     - 0 个 → 不渲染 pill；
     - 1 个 → 像现在一样渲染，pill 描述那个仓库；
     - >1 个 → branch pill 左侧再加一个 **仓库切换 pill**，按钮
       展示当前 selected 仓库名，点击下拉切换。
   - 后端：复用现有 `find_git_repos_below`（已有，用于 SCM 扫描），
     但 max_depth=1 + 把 cwd 自身也纳入。新命令
     `find_repos_in_cwd(cwd) -> Vec<String>`。
   - 前端：`PaneGitInfo` 加 `availableRepos: string[]`（含 selected）；
     `paneGitStatus.ts` 改为：第一次发现多仓库时把 selected 默认设
     第一个，存到 store；新组件 `PaneRepoSwitcher.svelte` 渲染下拉。
   - `PaneGitPill` / `PaneDiffPill` 用 selected repo 的数据。
   - **第 38 轮做这个**——用户连续 4 轮关注 git pill 行为，必须落地。

η. **再次验证 pane git pill 真实数据 + 非 git 仓库不展示** ✓ 第 36 轮
   已交付：3 个 vitest contract 锁 + `docs/PANE_GIT_PILL_VERIFY.md`
   用户自助验证文档。结论：代码 0 处 mock，pill 严格 gate，常见误解
   是用户 cd 进了某个 git 仓库的子目录（git 真当作 git 仓库处理是
   正确行为）。
   _（旧排查清单保留：）_
   - 用户报告仍看到假数据 / 非 git 仓库也展示按钮——需要再次复现
     并定位。
   - 路径：
     - 在非 git cwd 的 pane 实测 PaneGitPill / PaneDiffPill 是否真
       hide；
     - 检查 `paneGitStatus.ts::resolveInfoForCwd` 的 null 流是否真
       走到 store；
     - 是否有 stale store 项（关 pane 没清）？
     - SplitContainer 是否还有别的位置展示 git 信息；
     - 检查是否仍有 placeholder / mock 数据残留。
   - 加 e2e：在非 git 的 cwd pane 标题栏不应有 GitBranch icon。

δ. **确认 Claude Code Agent Teams 的 teammates 分屏能力是否真正支持**
   ✓ 第 35 轮已交付：`docs/AGENT_TEAMS_TEAMMATES.md` 报告，结论
   "已真支持" + 6 条 PARTIAL 缺口列清单。
   _（旧调研清单保留：）_
   — 用户问能否让 Claude Code 的 "Agent Teams" 模式真的把 teammates
   分屏展示（每个 teammate 占一个 pane）。
   - 复盘 CLAUDE.md "Claude Code Agent Teams (TmuxBackend)" 段落。
     当前架构走 tmux shim → 后端 register_teammate_agent，每个 agent
     绑一个 pane；Wind 是 Claude 的 tmux backend。
   - 验证："/agent" 或类似 Claude Code 指令真触发新 pane 时，新 pane
     是否真在 paneTreeStore 出现 + 是否有 split-pane 触发；
     Backend `pane.rs` 的 `split_pane_at_path` 是否被 shim 调用过；
     `tmux split-window` 翻译路径是否到 split_pane。
   - 现状文档 + 缺失功能立项：哪几条 tmux 命令还没翻译、哪些
     Claude Code 信号没接（如 `tmux send-keys`、`tmux select-pane`）。
   - 先做研究 + 写报告，不动代码——用户问的是"是否支持"，明确给
     yes/no/partial 答复后再决定要不要补。

#### 用户 2026-04-25 之前明确要求（合并入计划，按子项落地）

A. **Claude Code 扩展独立成 sidebar tab** ✓ 第 27 轮已交付
   - 当前：Bot 按钮在 pane 标题、claudeHistory plugin 嵌在 Explorer 内、
     ClaudeAgentLauncher 是 modal —— 三处分散，与文件树混在一起，交互
     杂乱。
   - 目标：把 Claude Code 做成"用户可选的安装项"——加 Settings 开关
     `claudeExtensionEnabled`（localStorage 持久化，默认 on 不破坏现状），
     启用时在左 rail 加第 4 个图标（Bot），切到独立的 ClaudeCodePanel
     tab；该 tab 内承载所有 Claude 相关功能（按 pane 列出历史、agent
     状态、launcher 入口）。禁用时：rail 按钮消失、pane 标题 Bot 按钮
     消失、claudeHistory plugin 不注册。
   - 验收：禁用时 Explorer 干净；启用时 ClaudeCodePanel 提供"在此 pane
     启动 Claude / 查看历史"完整路径。

B. **PaneGitPill 拆成两个按钮（分支 + diff）** ✓ 第 24 轮已交付
   - 当前 pill 把 branch + dirtyFiles + +N -N + ahead/behind 全塞一颗
     胶囊；用户 reading 体验"像 mock"，且改动数据没有可点击钻入。
   - 目标：拆成 `PaneBranchPill`（保留分支选择 / 创建 / picker）和
     `PaneDiffPill`（只渲染 +N -N / 改动文件数）。点 diff pill 触发
     `wind:open-sidebar-tab=git` + 新事件 `wind:scm-focus-repo` 携带
     repoRoot，让 SCM 滚到对应仓库且展开。
   - 二者均在 `!info.branch` 时不渲染（已有逻辑保留）。

C. **SCM 文件行展示 per-file +N -N** ✓ 第 25 轮已交付
   - 当前 SCM file row 只有 status letter；用户希望"文件标题后方展示
     自己的改动行数"。
   - 后端：扩展 `ScmFile` 加 `additions: u32` / `deletions: u32`；
     `get_scm_status` 内部跑一次 `git diff --numstat HEAD`（不分组，
     按 path 索引）后回填到 staged + changes（untracked 留空）。
   - 前端：file row basename 后 + status letter 前插入 `+12 -3`，
     绿/红 dim，font-mono 9px。

D. **点击 SCM 文件 → Monaco DiffEditor**（替换当前 `<pre>` modal）✓ 第 26 轮已交付
   - 后端新增命令 `git_get_file_versions(repo_root, path, cached) ->
     (original, modified)`：original = `git show HEAD:<path>` 失败则
     空（新增文件）；modified = working tree（cached=false）或 `git
     show :<path>`（cached=true，已暂存版本）。被删除的文件 modified
     为空。
   - 前端 `DiffEditorModal.svelte` 用 `monaco.editor.createDiffEditor`
     ({ readOnly: true, renderSideBySide: 默认 true，可切 inline })。
     Monaco 已经在 FileEditor 加载，复用 loader。
   - SourceControl `showDiff` 改为打开 modal；旧的 `<pre>` 路径删除。

F. **真正的分支图谱渲染** ✓ 第 30 轮已交付（SVG 自研，8 cases 单测）
   - 候选：`@gitkraken/gitgraph-js`（MIT，纯 JS，无 React 依赖）或
     自研 SVG/canvas 走 `git log --graph --oneline` 解析。
   - 必须能渲染：分支线条、merge dot、HEAD 标记、tag 标签、commit
     hover → 右键菜单（cherry-pick / revert / checkout）。
   - 与提交记录共用 `git log` 数据源，避免双拉取。
   - 独立大轮，依赖 D（diff editor）完成后做。

E. **标准要求保持（不可回归）**
   - 终端虚拟滚动 / 块分批加载 / resize 安全：已在 round 17-21 落地，
     新工作不能破坏 `Pane.svelte` mount-time replay 256KiB tail 的语义。
   - 终端标题行 git 按钮：仅在 cwd 是 git 仓库时显示——B 项拆分后
     `PaneBranchPill` / `PaneDiffPill` 都通过 `info && info.branch`
     gate，不是 git 仓库不出现。
   - sidebar 插件机制：scope=workspace/pane/global 三层不动，仅 A 项
     把 claudeHistory 从 plugin 迁移到自家 tab。
   - 搜索 tab + 全文 search/replace：已落地；下面 P0-1 的 tantivy
     索引是性能加速层，不替换现有 ripgrep 路径。

0. **第 23 轮 review 发现的真实 bug 集中处理**（来自 code-reviewer）
   - `git.rs::get_git_diff_internal` 同时传 `--numstat --porcelain` 是无效
     组合：git 会忽略 `--porcelain`，函数仍按 porcelain 状态字节解析 numstat
     输出，`parts[1]` 取的是 deletions 而不是路径——`get_git_info_with_cwd`
     的 diff 区永远为空。修复：去掉 `--porcelain`，按 `<added>\t<removed>\t
     <path>` 重新解析。
   - `git.rs::get_scm_status`：detached HEAD 时 `branch_from_status` 拿到
     的是裸 `"HEAD"`（`split_once(' ')` 把 `(no branch)` 截掉），pill 上
     就显示 `HEAD` 而不是 `(detached at <sha>)`。修复：检测 `Some("HEAD")`
     → 退到 `get_current_branch` 拿 detached 友好串。
   - `git.rs::git_sync` 还在用 `err.contains("no upstream")` 字符串嗅探
     做 i18n-fragile 错误识别；现在前端已知 `has_upstream`，应该在 push
     前就走 `--set-upstream` 或抛友好错（与 git_push 的 set_upstream 路径
     对齐）。
   - `fs/search.rs`：用户写 `[unclosed` 这种非法 glob 时 `Pattern::new(s).ok()`
     吞错，搜索像没设过滤一样跑全量。VS Code 在 input 上画红线提示。改为
     收集解析错误返回给前端 toast。

1. **搜索索引 tantivy spike**（用户最久未动批次）
   - 后端启动后台 index 每个 workspace cwd（跳过 SKIP_DIRS）；新命令
     `tantivy_search(root, query)` 毫秒级。
   - `notify` crate 增量更新 + `~/.wind/cache/<root-hash>.idx` 持久化。
   - 客户端 SearchSidebar 优先索引；超时 / cold fallback 到当前 ripgrep。
   - **独立一轮**（spike + 选型 + 写入）。

### P0-J — Explorer 僵尸终端 & 跨终端合并文件树专项单元测试

**背景**

第 47c/48 轮修复了两个 Explorer 顽固 bug：
- **僵尸终端**：关闭/分屏 pane 后 `paneCwdStore` 残留死键，Explorer 文件树列不消失。
- **新分屏不合并**：`splitPane` 继承父 cwd 但不发 `pane-cwd-changed`，导致同 cwd 的
  两个 pane 各自渲染独立文件树，不合并成单列。

修复是对 `syncPaneLayoutFromBackend` 做"两次原子 update"（Pass 1 Prune 死键 +
Pass 2 Seed 新 pane 的初始 cwd）。代码逻辑已正确，但**没有配套单元测试**——
一旦有人重构这个函数，bug 会悄悄复现。

**目标**：为这两个 bug 的修复路径写**红绿可重复**的 vitest 测试，并同时为
`syncWithPaneCwds`（`fileExplorer.ts`）的合并逻辑写对称测试。

---

**测试文件位置**

| 文件 | 内容 |
|---|---|
| `src/lib/stores/paneTree.test.ts` | `syncPaneLayoutFromBackend` 两个 bug 的契约（已有文件，追加 describe block） |
| `src/lib/stores/fileExplorer.test.ts` | `syncWithPaneCwds` / `syncAllWorkspaces` 的新测试文件 |

---

**`paneTree.test.ts` — 追加 describe: "syncPaneLayoutFromBackend — zombie & merge"**

```typescript
// T1: Pass 1 Prune — 关闭 pane 后僵尸键必须从 paneCwdStore 消失
it('removes dead pane keys from paneCwdStore when a pane is closed', async () => {
  // Arrange
  paneCwdStore.set({ 'ws1:pane-a': '/code', 'ws1:pane-b': '/home' });
  activeWorkspaceId.set('ws1');
  invoke.mockResolvedValue({ type: 'leaf', id: 'pane-a' }); // 布局只剩 pane-a

  // Act
  await syncPaneLayoutFromBackend();

  // Assert
  const store = get(paneCwdStore);
  expect(store).toHaveProperty('ws1:pane-a');
  expect(store).not.toHaveProperty('ws1:pane-b'); // 僵尸键已清除
});

// T2: 僵尸键不跨工作区误删
it('does not remove keys from other workspaces when pruning', async () => {
  paneCwdStore.set({ 'ws1:pane-a': '/code', 'ws2:pane-x': '/home' });
  activeWorkspaceId.set('ws1');
  invoke.mockResolvedValue({ type: 'leaf', id: 'pane-a' });

  await syncPaneLayoutFromBackend();

  const store = get(paneCwdStore);
  expect(store).toHaveProperty('ws2:pane-x'); // 其他工作区不受影响
});

// T3: Pass 2 Seed — 分屏后新 pane 的 cwd 被种入 paneCwdStore
it('seeds cwd from layout into paneCwdStore for new panes that never fired pane-cwd-changed', async () => {
  paneCwdStore.set({ 'ws1:pane-a': '/code' }); // 只有父 pane 有 cwd
  activeWorkspaceId.set('ws1');
  invoke.mockResolvedValue({
    type: 'split', id: 'root', direction: 'horizontal', ratios: [50, 50],
    children: [
      { type: 'leaf', id: 'pane-a', cwd: '/code' },
      { type: 'leaf', id: 'pane-b', cwd: '/code' }, // 新 split pane，继承父 cwd
    ],
  });

  await syncPaneLayoutFromBackend();

  const store = get(paneCwdStore);
  expect(store['ws1:pane-a']).toBe('/code');
  expect(store['ws1:pane-b']).toBe('/code'); // 新 pane 已种入
});

// T4: Seed 不覆盖已有活跃 pane 的 cwd（优先保留 pane-cwd-changed 上报的值）
it('does not overwrite an existing cwd for a live pane during seed pass', async () => {
  // pane-a 已经通过 pane-cwd-changed 切换到 /new
  paneCwdStore.set({ 'ws1:pane-a': '/new' });
  activeWorkspaceId.set('ws1');
  invoke.mockResolvedValue({ type: 'leaf', id: 'pane-a', cwd: '/old' }); // 布局里是旧 cwd

  await syncPaneLayoutFromBackend();

  // Seed Pass 2 只写"尚未在 store 中"的条目，不覆盖已有值
  expect(get(paneCwdStore)['ws1:pane-a']).toBe('/new');
});
```

---

**`fileExplorer.test.ts` — 新文件**

```typescript
// E1: 同工作区两个 pane 同 cwd → 合并成一列，不渲染两棵树
it('merges two panes with the same cwd into a single column', () => {
  store.syncWithPaneCwds('ws1', { 'pane-a': '/code', 'pane-b': '/code' });
  const cols = get(store).columns.filter(c => c.workspaceId === 'ws1');
  expect(cols).toHaveLength(1);
  expect(cols[0].paneIds).toContain('pane-a');
  expect(cols[0].paneIds).toContain('pane-b');
});

// E2: 两个不同 cwd 各自独立列
it('keeps distinct cwds as separate columns', () => {
  store.syncWithPaneCwds('ws1', { 'pane-a': '/code', 'pane-b': '/home' });
  const cols = get(store).columns.filter(c => c.workspaceId === 'ws1');
  expect(cols).toHaveLength(2);
});

// E3: pane 关闭（paneCwds 里删掉）→ 对应列消失
it('removes the column when its last pane is closed', () => {
  store.syncWithPaneCwds('ws1', { 'pane-a': '/code', 'pane-b': '/home' });
  store.syncWithPaneCwds('ws1', { 'pane-a': '/code' }); // pane-b 已关
  const cols = get(store).columns.filter(c => c.workspaceId === 'ws1');
  expect(cols).toHaveLength(1);
  expect(cols[0].paneIds).toEqual(['pane-a']);
});

// E4: pane cd 到新路径 → 旧列 pane 减少（或消失），新列出现
it('moves a pane to a new column when it cds to a different cwd', () => {
  store.syncWithPaneCwds('ws1', { 'pane-a': '/code' });
  store.syncWithPaneCwds('ws1', { 'pane-a': '/home' }); // pane-a cd 了
  const cols = get(store).columns.filter(c => c.workspaceId === 'ws1');
  expect(cols).toHaveLength(1);
  expect(cols[0].cwd).toBe('/home');
});

// E5: 其他工作区的列不受影响
it('does not touch columns from other workspaces', () => {
  store.syncWithPaneCwds('ws1', { 'pane-a': '/code' });
  store.syncWithPaneCwds('ws2', { 'pane-x': '/other' });
  store.syncWithPaneCwds('ws1', { }); // ws1 全部 pane 关闭
  const ws2Cols = get(store).columns.filter(c => c.workspaceId === 'ws2');
  expect(ws2Cols).toHaveLength(1); // ws2 不受影响
});

// E6: syncAllWorkspaces — 多工作区批量 sync 正确分发到各自列
it('syncAllWorkspaces correctly routes paneCwds by workspaceId prefix', () => {
  const workspaces = [{ id: 'ws1', name: 'WS1' }, { id: 'ws2', name: 'WS2' }];
  store.syncAllWorkspaces(workspaces, {
    'ws1:pane-a': '/code',
    'ws2:pane-x': '/home',
  });
  const ws1Cols = get(store).columns.filter(c => c.workspaceId === 'ws1');
  const ws2Cols = get(store).columns.filter(c => c.workspaceId === 'ws2');
  expect(ws1Cols[0].cwd).toBe('/code');
  expect(ws2Cols[0].cwd).toBe('/home');
});
```

---

**实现顺序**

1. 先写 `fileExplorer.test.ts`（纯 store 逻辑，mock 量小，E1–E6 覆盖合并/拆分/跨工作区）。
2. 在 `paneTree.test.ts` 追加 T1–T4（需要 mock `invoke` 返回 layout，
   已有框架可复用）。
3. 跑 vitest —— 如果 T3/E1/E3 红了说明还有 bug，定点修复。
4. 全绿后，vitest 数量应 +10 条（128 → 138），锁住两个历史顽固 bug。

**优先级**：P1（重要但不阻塞主流程），建议下一轮和 SearchSidebar 并行诊断一起处理。

---

### P1 — 流畅度

_ρ / σ / τ / υ 已在第 44 轮交付（见上）。_

χ. **SCM 图谱缓存（对齐 ε scmCacheStore 模式）**
   （用户 2026-04-25 13:18 反馈）
   - 现状：`SourceControl.svelte::loadGraph(root)` 把
     `graphInfo: GitRepoInfo` 写进**组件本地 `$state`**——SCM tab 一卸载
     就丢；每次 mount / 切 selectedRepo / 切 tab 都触发
     `get_git_info_with_cwd`（IPC + git2 walk + ref 装饰），大仓库 100 \-
     500ms 卡顿肉眼可见。
   - 第 42 轮 ε 已经为 `repoRoots` + `statuses` 建了 `scmCacheStore`
     （`src/lib/stores/scmCache.ts`），mount 时 hydrate + 30s 后台
     refresh，是同样的痛点。本任务把图谱接进同一个模式。
   - 改造（与 ε 对称）：
     - **`scmCache.ts` 扩字段** `graphInfos: Record<repoRoot, GitRepoInfo>`
       + `lastGraphLoadAt: Record<repoRoot, number>`。GC 与 statuses
       一致：`setScmRepoRoots(...)` 时把不再存在的仓库的 graph 一并清。
     - 新 API：`setScmGraphInfo(root, info)`、
       `clearScmGraphInfo(root)`、
       `shouldRefreshGraphOnMount(root, maxAgeMs=30_000)`。
     - **`SourceControl.svelte`**：
       - `graphInfo` 由 `$state` 改成 `$derived($scmCacheStore
         .graphInfos[selectedRepo])`，模板零修改（消费 `graphInfo
         .commits` 不变）。
       - `loadGraph` 写 cache 而非组件内 state；onMount
         `shouldRefreshGraphOnMount` true → 立刻 schedule；false →
         缓存即时显示 + 1s 后后台刷新（VS Code GitLens 同款"瞬出 +
         自动新鲜"）。
       - **`selectedCommitHash` 跨 mount 持久化**：图谱缓存 hit 时不要
         无条件清 hash（当前 `loadGraph` 顶部 `selectedCommitHash = ''`
         的逻辑要拆成 "stale data → clear" / "cache hit → keep"）。
         配套在 `scmCacheStore` 里挂 `selectedCommitHashByRepo`，
         hash 不再属于组件局部 state。
     - **invalidation triggers 全保留**：commit / stage / checkout /
       cherry-pick / revert / 用户点刷新 → 现在调 `loadGraph` 的地方
       原样调用，写入 cache 即可（已有同步路径，差一行
       `setScmGraphInfo(root, info)` 替换）。
     - **graphLoading state**：缓存命中时不显 loading，仅后台刷新；
       无缓存（首次 mount / GC 后第一次访问）才 spinner。
   - **vitest 扩展**：与第 42 轮 scmCache.test.ts 同模式，6 个 case
     覆盖 graph GC、stale 判定、selectedCommitHash 跨 mount 保持、cache
     miss path。
   - **ε 阶段二联动**：将来 notify watcher 监听 `.git/HEAD`、
     `refs/heads/`、`refs/remotes/` mtime → 事件驱动 invalidate
     graph + statuses 一起，30s 定时器作降级兜底。store 形状不变。

ψ. **Explorer 跨工作区同 cwd 合并文件树**
   （用户 2026-04-25 13:36 反馈："两个终端的cwd如果相同，需要合并展示file tree"）

   **现状分析**
   - 同工作区内多个 pane 共用同一 cwd → `syncWithPaneCwds` 已经在做：
     以 `"${workspaceId}:${cwd}"` 为列 ID，所有 paneIds 合并进同一列，
     Explorer 里只出一个区段（头部展示多个 pane 角标）。**此情形已处理。**
   - 跨工作区：workspace-A pane-1 和 workspace-B pane-1 同在 `/code/wind`
     → 各自的 `syncWithPaneCwds(wsId, ...)` 产生两个不同列（id 不同），
     Explorer 在两个工作区标题下各渲染一棵一样的树。**此情形未处理。**

   **改造目标**
   同 cwd 列跨工作区共享 FileTree 渲染和数据（树、expandedPaths、
   selectedPath）——不合并工作区标题本身，只合并树体：

   ```
   ▼ WORKSPACE A (active)       ← workspace header 保留
     ● 终端 1  ● 终端 2          ← 显示两个工作区里这个 cwd 的所有 pane 角标
     src/                        ← 只渲染一棵树（共用数据）
       ...

   ▼ WORKSPACE B
     [已在上方合并显示，此 cwd 折叠或不重复渲染]
   ```

   **实现思路（两个方案选其一）**

   方案 A — "全局 cwd 主列"（推荐）：
   - `fileExplorerStore` 增加 `globalColumns: Map<cwd, ExplorerColumn>` —
     跨工作区以 cwd 为 key 的全局列注册表。
   - `syncWithPaneCwds` 改为 upsert 进 globalColumns，paneIds 汇聚所有
     工作区里这个 cwd 的 pane；column.workspaceIds = string[] 记录归属。
   - Explorer 按 cwd 聚合渲染：同 cwd 只出一个区段，section header 里
     按工作区分组显示 pane 角标（`[WS-A] 终端1 | [WS-B] 终端2`）。
   - 优点：最彻底，不重复渲染树。
   - 代价：Explorer.svelte 的分组逻辑要重构（目前按 workspaceGroups 走）。

   方案 B — "副列引用主列"（保守）：
   - 保留现有 per-workspace 列结构。
   - 每次渲染某个 cwd 列时，检测全局是否已有另一个工作区的同 cwd 列在
     渲染中；若是，次出现的工作区里的区段改为只显示 header（pane 角标），
     树体渲染改为 `<p class="text-[11px] text-muted pl-4">↑ 已在 WS-A 显示</p>`。
   - 优点：改动量小，不破坏现有列结构。
   - 代价：有重复列存在于 store，后台 loadTree 仍各跑一次（可优化为共享
     同一个 tree ref）。

   **建议优先实现方案 B**，让用户先感受到消重，待 UX 验证后再升级方案 A。

   **边界**
   - 同工作区内同 cwd 已合并（不受影响）。
   - "主列"优先级：active workspace 的列为主，其他工作区的列为副。
   - 路径规范化：`/code/wind` vs `/code/wind/` vs `C:\code\wind` 要 normalize
     后再比较（参考现有 Explorer 里的 `normalise(s)` helper）。
   - Expand/select 状态全局共享（同一 tree ref），不分别保存。

φ. **markdown front-matter CRLF 兼容** ✓ 第 45 轮已交付
   - `stripFrontMatter` 已在行 274 加 `source.replace(/\r\n/g, '\n')`；JSON `{...}` front-matter 同步识别。

2. **`.wind` 文件持久化 paneCwds** ✓ 已确认工作（第 63 轮代码分析）
   - `Pane.cwd` derives `Serialize`，`snapshot_workspace` 已将其序列化到 `.wind`；
   - `open_workspace_from_file` 反序列化后调用 `refreshWorkspaces`，
     `get_pane_layout` 读 `pane.cwd` → `extractCwdsFromLayout` → `paneCwdStore`。
   - 完整路径已通。

3. **SearchSidebar 结果行渲染优化** — 第 66 轮交付
   - 已改用 CSS `content-visibility: auto; contain-intrinsic-size: 0 22px;`
     (class `wf-search-row`) 对每条结果行延迟 paint，避免 500+ 条全渲时 GPU
     负担。注意：不能对 `.search-file` 容器用此属性，否则 layout containment
     会破坏内部 `position:sticky` 的文件头，必须逐行应用。
   - 如需真正的虚拟滚动（DOM 节点数量也减少），考虑扁平化 groups → flat items 后
     接入 `@tanstack/svelte-virtual`，但实现复杂度高，先看 CSS 效果。

4. **`paneGitStatus` 5 分钟周期 fetch** ✓ 第 49 轮已交付（`refreshAllCachedRepos` 5 min interval）

5. **PaneGitPill base ref 列表过长时折叠** ✓ 第 65 轮已交付（`<select>` → `<datalist>` combobox）

6. **SearchSidebar 接入第 23 轮新加的后端 globs**
   - 现在 SearchOptions 已经支持 include/exclude，但前端只把它们当成
     "用户填的过滤"传过去。可以扩成：默认隐藏 SKIP_DIRS / 二进制扩展，
     用户能 override；和 ripgrep 的 `--glob` 形成行为对齐。

### P2 — 整洁

7. **`ScrollbackHistoryModal` 大段历史下载性能**
   - 现在一次性 cleaned；几 MiB 时 `<pre>` 渲染 + Blob 都不便宜。考虑 Save
     时直接走原 `bytes`（少一次 stripAnsi），或后端流式 export。

8. **`PaneGitPill` 创建分支后 `branches` cache 清掉但下次打开 picker 才重拉**
   - 体验上没问题（picker 关了再开），但 ahead/behind 数字延迟到下次 cwd
     变化才更新。本轮的 SCM 联动只覆盖 SCM 内部操作，pill 自身的 checkout
     调用没走 SCM 联动。需在 commitCreate 末尾 explicit
     invalidatePaneGitStatusForRepo（已经做了）。

### P3 — 体验打磨

9. **modal "复制全部" 之后给个 toast 替代 inline checkmark** ✓ 第 51 轮已交付
   - `ScrollbackHistoryModal.svelte` `copyAll()` 成功 → `showToast('已复制到剪贴板')`

10. **PaneGitPill：上次操作的 toast** ✓ 第 51 轮已交付
    - `switchTo()` → `showToast('已切换到 ${branch}')` / `commitCreate()` → `showToast('已创建并切换到 ${branch}')`

11. **`linkTrust` 信任管理界面**
    - 第 23 轮加的 host trust 是纯隐式 Set，用户没办法看到 / 撤销已信任
      host。可在 Settings 里加个面板列出本次会话已信任 hosts，带 revoke
      按钮；或在 confirm 框里加 "始终允许 / 仅本次"。

12. **MarkdownPreview confirm 用自家 modal 替换 `window.confirm`** ✓ 第 60 轮已交付
    - `openExternal` 改用 `choiceDialog`（"始终允许" / "仅本次" / "取消"）；
      `window.confirm` 已清零。

13. **`linkTrust` 改为 per-basePath 信任作用域** ✓ 第 52 轮已交付
    - `Set<string>` → `Map<basePath, Set<string>>`；
      `MarkdownPreview` 把 `basePath` 传入 `isTrustedUrl` / `trustHostFromUrl`。

---

## 🧭 下一轮建议起点

**推荐按顺序：**

_ε阶段二 / P1-3 / P1-4 / P0-I 已在第 49–50 轮交付。_
_P3 Toast / P3-9 / P3-10 / P1-4 / P3-13 / P2-8 / SearchSidebar 结果限制 / SCM watcher debounce 已在第 50–53 轮交付。_
_终端右键菜单（ζ LOW 收尾项）已在第 54 轮交付。_
_commit 行 Shift+F10 + runGitOnSelectedRepo SCM 联动已在第 55 轮交付。_
_δ PARTIAL 缺口（kill-pane / rename-window / tmux `#{...}` 模板扩展）已在第 62–63 轮交付。_
_P1-2（`.wind` paneCwds 持久化）已在代码分析中确认已工作（`Pane.cwd` derives Serialize → `.wind` → 还原路径 `openWorkspaceFromFile → refreshWorkspaces → extractCwdsFromLayout`）。_

_横向 Tab 滚动修复 + SCM 刷新降频 已在第 64 轮交付。_
_DnD 功能（工作区 tab / 文件编辑器 tab / SplitContainer pane 拖拽）第 64 轮确认均已实现：_
_`WorkspaceTabs`（handleDragStart/Over/Drop/End + `onReorder` → `reorderWorkspaces`）、_
_`FileEditor`（onTabDragStart/Over/Drop + `fileEditorStore.reorder`）、_
_`SplitContainer`（paneDragSourceId + `dockPane`）。如仍有问题可能是视觉 tab 堆叠影响了用户感知（横向滚动已修复）。_

_PaneGitPill base ref combobox + CLAUDE.md 同步已在第 65 轮交付。_
_SearchSidebar content-visibility + 计划整理已在第 66 轮交付。_
_终端 Web Links + 字体大小控制已在第 67 轮交付。_
_终端内搜索（Ctrl+F）已在第 68 轮交付。_

0. **P0-1（tantivy 索引 spike）** — 大轮独立规划，需独立 branch + Rust 侧 spike。目前最高优先级大项。
   备注：ripgrep+ignore 已达 <1s，tantivy 约 50ms 提升量级有限；spike 前应确认用户是否感知到搜索延迟。
1. **P1 剩余**：
   - SearchSidebar 真正虚拟列表（flatten groups → @tanstack/svelte-virtual；现在只有 CSS paint skip）
   - 其余 P1 项已全部交付
2. **P2-7 ScrollbackHistoryModal** 大段历史下载性能（边缘场景）
3. **P3-11 linkTrust 管理界面**（设置面板列出 / 撤销已信任 hosts）— 低优先级

> 本文档更新规约：每次 /loop 结束时，Agent 把本轮完成项追加到「✅ 历史轮次已完成」
> 段，把新发现的问题追加到对应优先级段，把"下一轮建议起点"刷新到最紧迫的 2–3 项。
