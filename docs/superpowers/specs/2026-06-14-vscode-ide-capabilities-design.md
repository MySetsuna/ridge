# Ridge 桌面端 VSCode 级 IDE 能力 设计

日期：2026-06-14
分支：develop
目标：文件搜索 palette、Ctrl+Click 跳转（路径 + 符号 LSP）、行级 Git 记录。

## 现有基建（勘探结论）

| 能力 | 已有 | 缺口 |
|---|---|---|
| 文件搜索 | `QuickOpen.svelte`（完整但**未挂载**：fuzzy `filenameSearch` + 方向键 + Enter dispatch `openFile`）；`filename_search` Rust 命令；`fileEditorStore.openFile(path,{line,column})` | 挂载 + 绑 Ctrl+P |
| 路径跳转 | `linkResolver.ts`（`classifyLink`/`resolveLink`/`executeAction`，已支持 `open-file{path,line,col}`/`reveal`） | Monaco 未注册 link/mousedown provider |
| 符号跳转 | Monaco（含 ts.worker） | 未接 LSP（用户选**全量 LSP**） |
| Git 行记录 | `git_diff_file`（git2，ridge-core）；`fileEditorStore` reveal | 无 `git_blame`/行历史命令 + UI |
| 索引 | `.codegraph/` 存在 | app 未集成（LSP 路线不依赖它） |

关键复用点：`fileEditorStore.openFile(path,{line,column,matchLength})` 是三项共同的"跳到目标"出口。

## 实施分阶段

### 阶段 1 · 文件搜索 palette（轻，主要接线）
- `+page.svelte`：`quickOpenVisible` $state；`handleGlobalKeydown`（line 437）加 `Ctrl+P` / `Ctrl+Shift+P` → 开 palette（preventDefault）。
- 挂 `<QuickOpen>`，`on:openFile`→`fileEditorStore.openFile(path)`，`on:close`→关闭。
- 验证：vitest + CDP 自助（见 [[feedback_self_verify_via_cdp]]）。

### 阶段 2a · 路径 Ctrl+Click（任意语言，复用 linkResolver）
- FileEditor 给 Monaco 编辑器加 `onMouseDown`：Ctrl/Cmd 按下且命中"路径样 token"（import 说明符、`path:line`、相对/绝对路径）→ `resolveLink` → `open-file`/`reveal`。
- 用 `registerLinkProvider` 给路径加下划线 + cmd-click 语义（Monaco 原生 link）。
- 落点用 `openFile(path,{line,column})`。

### 阶段 3 · 行级 Git 记录
- 新 Rust 命令（ridge-core `commands::git` + desktop/remote 薄壳 + dispatch arm）：
  - `git_blame(repo_root, path) -> Vec<BlameLine{line, commit, author, date, summary}>`（git2 `Repository::blame_file`）。
  - `git_log_file(repo_root, path, line?) -> Vec<Commit{sha, author, date, summary}>`（触碰该文件/行的提交；git2 revwalk + blame -L 近似）。
- UI：Monaco gutter/hover 显示 blame（作者·相对时间·摘要）；右键/hover "查看本行历史" → 提交列表 → 选中复用 `git_diff_file` 看 diff。
- 远程白名单：`git_blame`/`git_log_file` 加入 `remoteAllowlist`（只读）。

### 阶段 2b · 全量 LSP（符号 go-to-definition，多步、跨会话）

**架构（薄自研客户端，不引 monaco-languageclient 重依赖；契合本仓 Tauri-command + 远程桥）**

```
Monaco providers (definition/hover/references)
   │  invoke('lsp_request',{serverId,method,params})  +  event 'lsp://notify'
   ▼
src-tauri lsp/ 模块
   • LspManager：按 (language, workspaceRoot) 起一个 LSP 子进程（std::process stdio）
   • JSON-RPC over stdio（Content-Length 分帧），request/notify/response 路由
   • initialize 握手 + textDocument/didOpen|didChange 文档同步
   • 命令：lsp_request(serverId,method,paramsJson)->resultJson；lsp_did_change(...)
   • 进程生命周期：按需起、空闲超时、崩溃重启
   ▼
language server（typescript-language-server / rust-analyzer …，stdio）
```

- **传输**：Tauri command（请求/响应）+ Tauri event（server→client 通知，如 diagnostics）。天然经现有 invoke-request 白名单桥支持 web-remote（host 跑 LSP，远端转发）。
- **Monaco 桥** `lsp/lspClient.ts`：`registerDefinitionProvider` 等 → `invoke('lsp_request',{method:'textDocument/definition',...})` → LSP Location ↔ Monaco Range 互转 → `openFile(targetPath,{line,column})`。配 didOpen/didChange 同步（编辑器内容变更节流推给 server）。
- **供给（provisioning）**：先 typescript-language-server（npm）。检测顺序：项目 node_modules → 全局 PATH → 提示安装/打包。第一阶段先 TS（本仓自身 = TS，便于 dogfood 验证）；后续 rust-analyzer。
- **LSP 子分阶段**：
  - P1：单 server（TS）起停 + initialize + didOpen/didChange + `textDocument/definition` + Monaco definition provider + Ctrl+Click 端到端。
  - P2：hover / references / diagnostics（event 推送 + Monaco markers）。
  - P3：多语言（rust-analyzer）+ 供给检测 UI + 远程路径验证。

**风险**：LSP server 体积/安装（rust-analyzer ~50MB，倾向检测而非打包）；文档同步一致性；web-remote 下的转发时延；monaco 版本与 LSP 能力映射。先 TS 端到端打通验证架构，再铺开。

## 验证策略
- F1/F2a/F3：vitest（纯函数：路径分类、blame 解析）+ CDP 自助运行时验证（`pnpm tauri:dev:cdp`，改 .rs 自动重建，不杀正式会话）。
- LSP：起 TS server，CDP 触发 definition，断言跳转落点。
- 后端改动需 rebuild（杀会话）→ 最终回归用 CDP 调试实例，见 [[feedback_self_verify_via_cdp]] / [[env_cdp_dev_testing]]。

## 顺序
先 F1（最快赢）→ F2a（复用 linkResolver）→ F3（git blame）→ F2b LSP（P1 TS 端到端，多步）。

## 实施结果（2026-06-14）

### F1 文件搜索 palette ✅（svelte-check 0/0）
- `+page.svelte`：`quickOpenVisible` $state + `handleGlobalKeydown` 加 Ctrl+P / Ctrl+Shift+P（裸 Ctrl+P 在 TUI 活跃 pane 让位 shell）+ 挂 `<QuickOpen>`（复用现成组件）→ `fileEditorStore.openFile`。

### F2a 路径 Ctrl+Click ✅（svelte-check 0/0 + pathToken 8 单测）
- 新 `src/lib/utils/pathToken.ts`（`pathTokenAt`：提取光标下路径 token + 解析 `:line:col` 后缀，URL 端口不误判）+ 8 vitest。
- `FileEditor.svelte`：编辑器选项 `multiCursorModifier:'alt'`（VS Code 一致，腾出 Ctrl+Click）+ `onMouseDown`（Ctrl/Cmd+左键 → pathTokenAt → `resolveLink`（basePath=文件目录, knownCwds=[工程根]）→ `executeAction` 开文件到行列）。非路径 token 不拦截，留给 LSP。

### F3 行级 Git blame ✅（cargo check + 2 解析器单测 + svelte-check 0/0）
- ridge-core `commands/git.rs`：`git_blame`（`git blame -w --line-porcelain` + 纯函数 `parse_blame_porcelain`）→ `Vec<BlameLine{line,commit,author,timestamp,summary}>`；`git_file_log`（`git log --follow`）→ `Vec<FileCommit>`。
- desktop `commands/git.rs` 薄壳 + `lib.rs` 注册 `git_blame`/`git_file_log`。
- `FileEditor.svelte`：Alt+B / 右键「切换 Git 行注释」开关（`editor.addAction`）→ `invoke('git_blame')` → 行尾 `after`-decoration 注入「作者 · 相对时间 · 摘要」灰字（`.rg-blame-annotation`）；blame-sync `$effect`（读 `current` → tab 切换刷新；同步读 `blameVisible` → 开关刷新）；每次重建装饰集合绑定当前 model（跨 tab 不残留）。

⚠️ F3 后端需 rebuild 本地 ridge 才在桌面生效（后端改动）；F1/F2a/F3 运行时回归用 CDP（`pnpm tauri:dev:cdp`，见 [[feedback_self_verify_via_cdp]]）。
⚠️ 远程白名单 + server.rs dispatch（web-remote 下的 blame/file_log）+「本行历史」面板（git_file_log 列表 UI）= 后续 polish，未做。

### F2b 全量 LSP — P1 已完成（cargo 0 警告 + svelte-check 0/0 + lspClient 13 单测 + vitest 799）

**P1 = TypeScript/JavaScript go-to-definition 端到端（Ctrl+Click）已实现：**
- **Rust LSP host** `src-tauri/src/lsp/mod.rs`：`LspManager`（OnceLock 全局，按 workspace_root 起 server）+ `Server`（tokio::process spawn typescript-language-server `--stdio`；Windows 经 `cmd /c` + CREATE_NO_WINDOW）+ JSON-RPC `Content-Length` 分帧读循环（响应路由回 pending oneshot）+ `initialize`/`initialized` 握手 + request/notify。Tauri 命令 `lsp_did_open`/`lsp_did_change`/`lsp_definition`（lib.rs 注册）。`path_to_uri` 单测。tokio 加 `process`+`io-util` feature。
- **前端** `src/lib/lsp/lspClient.ts`：`pathToUri`/`uriToPath`（Windows 盘符冒号不编码、回程转反斜杠）+ `lspLanguageId`（TS/JS 家族）+ `parseDefinition`（Location|Location[]|LocationLink[]|null → 1-based LspTarget[]）+ invoke 包装；13 vitest。
- **FileEditor 接线**：`onMouseDown` 非路径 token 分支 → `lspDefinition(root, path, line0, char0)` → `fileEditorStore.openFile(target)`（LSP 无结果回退路径 reveal）；TS/JS 文件激活 `$effect` → `lspDidOpen`（每路径一次）；编辑 → `lspDidChange`（版本递增）。

**⚠️ 需 rebuild 本地 ridge 才运行时生效**（Rust host + 命令）；typescript-language-server 须全局安装（本机 5.3.0 已装）。运行时验证：开 .ts 文件 → Ctrl+Click 符号 → 跳定义。

**P2 已完成**（cargo 0 警告 + svelte-check 0/0 + lspClient 20 单测 + vitest 806）：
- **F12 / 右键「转到定义」**：`editor.addAction('rg.gotoDefinition', F12)` → `gotoDefinitionAt`（与 Ctrl+Click 共用：`lspDefinition` → `openFile`）。**刻意不用 Monaco `registerDefinitionProvider`+`registerEditorOpener`**——其返回的 `monaco.Uri.file` 会把盘符小写化 + 冒号编码，与本机 tab 路径口径不一致易致重复 tab；用 editor action 复用已验证的 path 口径更稳。
- **Hover**：新 Rust 命令 `lsp_hover`（textDocument/hover）+ 前端 `registerHoverProvider` → `lspHover` → `parseHover`（MarkupContent | MarkedString | MarkedString[] → Markdown，7 单测）。
- **诊断**：Rust read_loop 转发 `textDocument/publishDiagnostics` 通知 → 全局 `APP_HANDLE`（setup 注入）emit Tauri event `lsp://diagnostics` → 前端 `onLspDiagnostics` → `setModelMarkers`（红/黄波浪线）。initialize 已声明 hover/publishDiagnostics 能力。
- providers/监听 onDestroy 统一释放。

**P3 已完成**（cargo 0 警告 + svelte-check 0/0 + lspClient 21 单测 + Rust server_kind 单测编译过）：
- **多语言**：`ServerKind`（按扩展名路由 TS/JS / Rust）；`ensure(root, kind)` 按 (语言,工作区) 起 server；rust-analyzer 命令（原生 exe 直调）。**rust-analyzer 本机未装** → 装上即用（`rustup component add rust-analyzer`）。
- **references**：`lsp_references`（textDocument/references, includeDeclaration）+ Monaco `registerReferenceProvider`（Shift+F12/右键「查找所有引用」→ peek）。
- **editorOpener**：`monaco.editor.registerEditorOpener` 把 Monaco 内部导航（references peek 点击等）路由到 `fileEditorStore.openFile`；配合 `uriToPath` 盘符大小写归一（避免重复 tab）。
- **供给检测**：`ServerKind::install_hint` 起进程失败时给语言专属安装提示。

**LSP 仍待（P4）**：F12 的 Ctrl+hover 下划线视觉提示（需 definition provider，与现 onMouseDown/F12-action 取舍）；诊断的 web-remote event 中继白名单；供给检测 UI（缺 server 当前静默 console.warn）；rust-analyzer 真机验证。

## 源代码管理（SCM）对标 VSCode

### 可编辑 Diff ✅（svelte-check 0/0；需 rebuild 运行时验）
- `FileEditor`：`applyDiffEditable` —— **仅「工作区改动」diff**（`!commit && !cached`）的 modified 侧 `readOnly:false`，历史 commit / 已暂存 diff 保持只读（对标 VSCode）。
- diff 编辑器自带 `Ctrl/Cmd+S` → `saveDiffModified`：把 modified 内容 `write_file` 到 `repoRoot/git-relative path` + `markRecentlyWritten` 抑制自写的 fs-changed 提示。**自包含、不碰 store 的 saveFile**（diff tab 是合成路径 `__diff__:…`，避免误写）。

### 待续（需 repro / 澄清，未盲改避免不可验证的回归）
- **改动文件行右键菜单**（确认缺口）：staged/changes/untracked 行**无 `oncontextmenu`**（只 commit 行有）。VSCode 缺口=加 Open File / Open Changes / Stage·Unstage / Discard 右键菜单（复用现有 stage/unstage/discard/openDiffTab + showContextMenu）。
- **模式切换更新不及时**：嫌疑=diff-load effect 的 `if (c.path === diffCurrentPath) return` 早返回 + `diffModelCache` 缓存 → 暂存/提交后重开同一文件显示陈旧 diff。需运行时 repro 确认。
- **图谱展示优化**：需明确诉求（布局/性能/交互？）。
