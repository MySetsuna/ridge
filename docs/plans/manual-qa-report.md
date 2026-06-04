# Ridge 远控/TUI/编辑器 实测 QA 报告（GM 综合）

> 日期 2026-06-04。本轮以"GM 主驱动 live 测试 + 多 subagent 并行修复"的模式完成。详细分项报告见同目录：`file-editor-review.md`、`terminal-resize-input-review.md`、`remote-controller-ux-review.md`、`mobile-remote-resize-fix.md`、以及 `../ridge-cli-usage.md`。证据截图：`test-desktop-*.png`、`test-browser-*.png`。

## 测试环境
- Host：`target/release/ridge.exe`（2026-06-04 03:55 构建，含 cloudHostBridge），WebView2 CDP :9222，工作区 C:/code/wind。
- LAN 远程服务：**HTTPS** `0.0.0.0:9527`（本地 CA TLS），TOTP 鉴权。
- 控制端浏览器：独立 Chrome（chrome-devtools MCP，与 host 的 WebView2 隔离）——即真人浏览器行为，非测试假象。

## 一、桌面 host 实测（✅ 全部正常）
| 项 | 结果 |
|---|---|
| 文件树 / git / 启动 | ✅ |
| 文件编辑器 open/load/语法高亮 | ✅ |
| 点击中部行 → 光标定位到该行 | ✅（基础正常；早期"光标卡顶"是误测——点到了 `.native-edit-context` 隐藏输入元素而非 view-line） |
| 终端 + **opencode TUI** 渲染 | ✅（logo/状态栏/字形对齐正确） |

## 二、文件编辑器（Monaco 0.55.1）—— 已修
- **CRITICAL `each_key_duplicate`**（live console 实测发现）：`fileEditor.ts` openFile 的 TOCTOU 竞态 → 同 path 重复 tab → Svelte 丢渲染 → "切 tab 内容错/不可编辑"。**已修**（原子 update 内去重 + dndItems 去重兜底）。
- **HIGH 光标随外部 reconcile 跳顶**：`setValue` 重置光标/滚动 → **已修**（saveViewState/restoreViewState）。
- 回归测试已固化：`src/lib/stores/fileEditor.test.ts`（5✓，含并发 double-click 竞态）+ `tests/e2e/fileEditor.spec.ts`（3✓，断言无 each_key_duplicate）。

## 三、终端 resize / 鼠标 / 键盘
- **桌面终端**（manager.ts/RidgePane）：resize/fit 接线审查**正确**（window-resize 与 split 共用 reflow 路径、kernel-grid 自愈覆盖 window resize、shrink-then-grow 正常）。
- **F2 鼠标滚轮（已修，MEDIUM）**：alt-screen 且未开 mouse-reporting 的 TUI（less/man/claude `/theme` 菜单）滚轮失灵 → 加 `wheelAltScroll`（xterm alternateScroll → 方向键）。
- F3（INFO）：OS 窗口边缘 resize 落在 500ms 防抖，可能感觉"自适应"偏慢，待 live 判断。
- F4（deferred）：窄屏 resize 残留杂字 = WebGPU host-canvas stale 像素 = **P4 渲染内核范畴**，未越界改。

## 四、控制端可用性（✅ 桌面 + 手机）
- **桌面浏览器控制端**：✅ HTTPS+TOTP 鉴权通过、LAN WS 连上、渲染完整桌面 UI 壳。
- **手机端控制端**（iPhone 模拟，UA 分叉到 `src/remote/MainApp.svelte`）：✅ 专属移动 UI（工作区下拉/底部触控栏/WebGPU 渲染）。

### 4.1 缺省工作区（指令："任何 host 启动需默认工作区"）—— 已定位真因 + 已修 host 侧
- 现象：桌面控制端连上后停在"请先选择一个工作区"，无法操作。
- **真因（live console 实测）**：boot 的 `refreshWorkspaces()` 调 `invoke('list_workspaces')`，但该命令**不在 host 远程 invoke 白名单**（`server.rs`）→ 抛 "command not available remotely: list_workspaces" → 工作区初始化链中断 → activeWorkspaceId 空。（手机端走 WS `list-workspaces` 协议、是另一条路，所以手机有工作区。）
- **修复（host 侧，本 GM 直接改）**：`src-tauri/src/remote/server.rs` 白名单补 `list_workspaces`（只读，镜像相邻 `get_active_workspace_id`）。boot 其余命令 `get_active_workspace_id`/`get_pane_layout`/`get_pane_layout_for` 已在白名单。
- E 的前端 `ensureActiveWorkspace()` 兜底守卫为补充（白名单修复后才真正生效）。
- **待 live 验证**：需 Rust rebuild + 重启 host。

### 4.2 "remote 中也能创建终端"（指令）—— 已修 + LIVE 验证通过
- 现象：手机端空态文案"在桌面端打开一个终端以开始"是死路。
- 真因：`MainApp.svelte:171` 死文案，未用已存在的 `ws.createPane()`（`wsRemote.ts:322`，host 协议支持）。
- 修复：空态改为可用的"新建终端"按钮 → `ws.createPane()` → 订阅新 pane；TopBar `+` 也补错误透传。
- **LIVE 验证通过**：手机端点"新建终端" → 真实创建 PTY pane（`PS C:\Users\12867>`），**键盘输入 live 通过**（输入 `claude` 回车执行），**claude TUI 渲染通过**。

### 4.3 resize → TUI 自适应全屏（指令重点）—— 已定位 + 已修（手机端）+ 待 live 复验
- 现象：手机端 viewport 变化后 claude TUI 不重排、内容错位/截断，需手动点"锁定渲染尺寸到本端并刷新"才正确铺满。
- **真因（host 协议层）**：自动 resize 走 `resize` 消息 → host 只记录 fallback 尺寸、**不动 PTY、不广播 pty-resized**；手动按钮走 `refresh-pane` → host 真实 resize PTY + 重排 + 广播。host 早有 `claim-pane`（完整 reflow 路径）但前端从没发过。
- 修复（前端）：resize 改发 `ws.claimPane()`；防抖 500→100ms；补 `visualViewport`/`orientationchange` 触发 refit。`build:remote` 通过。
- **待 live 复验**：重载手机端（已重建 static/remote bundle 即可，无需 Rust rebuild）。

## 五、⚠️ 安全发现（CRITICAL）
LAN 服务 `GET /info` **明文返回完整 `otpauth://` URI 含 TOTP secret 种子**——任何能访问 `/info` 者可推算所有动态码，TOTP 形同虚设（本次即据此过鉴权门）。**建议**：`/info` 不返回 secret/otpauthUri，仅在桌面端本地展示二维码/当前码。未修，待确认。

## 六、⚠️ 工作区并发状态（影响提交）
测试期间检测到**两个并发外部进程**在改本仓库（非本 GM 的 subagent）：
1. **ridge-tmux 抽取**：`packages/ridge-tmux/` 新建 + `src-tauri/src/teammate/native.rs`(1449→13 行 re-export) + `Cargo.toml`/`src-tauri/Cargo.toml`/`Cargo.lock` + `src/lib/plugins/index.ts`（native-sessions 门控 !webRemote）。
2. **i18n 重构**：`SaveWorkspaceDialog.svelte:209` 处 `{@const}` 误放，导致 `pnpm check` 现有 1 个**外部**报错（非本轮改动）。
→ 因此**本 GM 未提交**：提交属用户协调（应只提交本轮 QA 改动、排除上述外部在制文件）。

## 七、本轮改动文件（仅本 GM/subagent，待用户决定提交）
- 编辑器：`src/lib/components/FileEditor.svelte`、`src/lib/stores/fileEditor.ts`
- 终端：`src/lib/components/RidgePane.svelte`、`src/lib/terminal/manager.ts`
- 远控前端：`src/remote/MainApp.svelte`、`src/remote/TopBar.svelte`、`src/remote/lib/{TerminalCanvas,terminalController,wsRemote}.ts`、`src/routes/+page.svelte`
- 远控 host 白名单：`src-tauri/src/remote/server.rs`（+`list_workspaces`）
- 测试：`src/lib/stores/fileEditor.test.ts`、`tests/e2e/fileEditor.spec.ts`
- 文档：`docs/ridge-cli-usage.md` + 本目录各分项报告
- **排除（外部在制）**：`Cargo.*`、`src-tauri/Cargo.toml`、`src-tauri/src/teammate/native.rs`、`packages/ridge-tmux/`、`src/lib/plugins/index.ts`、`SaveWorkspaceDialog.svelte` 及其它 i18n 文件。

## 八、待 live 复验清单（需一次干净 rebuild + 重启 host）
1. 桌面控制端缺省工作区（白名单 `list_workspaces` 修复）——需 Rust rebuild。
2. 手机端 resize 自适应（claim-pane 修复）——仅需重载（已重建 bundle）。
3. claude TUI 在桌面 host 的 resize/鼠标滚轮（F2/F3）——重建后复验。
4. `/info` TOTP 泄露——确认后修。
