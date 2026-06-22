# UI 缺陷批量修复设计 (2026-06-22)

五个独立的桌面端 UI 缺陷/小特性，根因均已在代码中核验。每项单独 commit。
交互方式已与用户确认（见各节"决策"）。

架构前提：SvelteKit 前端（`src/`）+ Tauri/Rust 后端（`src-tauri/`）+ 纯逻辑下沉
`packages/ridge-core`。桌面窗口在 `src-tauri/src/lib.rs:191` 以 `WebviewWindowBuilder`
编程式创建（`tauri.conf.json` 的 `windows` 为空）。

---

## ① 开关圆钮溢出 → 抽取可复用 `Toggle.svelte`

**根因**：全代码库唯一的开关组件在 `SettingsPanel.svelte:441-467`（设置→扩展→远程控制）。
其轨道宽用 `rem`（`w-9`=2.25rem），而圆钮位移用**固定 px**（`translate-x-[18px]`）。
两套单位基准不一致：在非 16px 根字号或 WebView2 缩放下，轨道按 rem 缩放而 18px 位移不变，
圆钮（`h-4 w-4`）右缘越过轨道右边界 → 截图所示溢出。

**决策（用户拍板）**：抽取可复用 `Toggle.svelte`，几何全部用自洽基准、数学上不可能溢出。

**修复**：
1. 新建 `src/lib/components/Toggle.svelte`：
   - 轨道 `relative` + 圆钮 `absolute`，圆钮垂直用 `top-1/2 -translate-y-1/2`，
     水平用 `left`/`right` inset（`off → left:2px`；`on → right:2px`，对称），
     **不用固定 px 的 `translate-x`**。轨道/圆钮尺寸同基准（如轨道 `h-5 w-9`、
     圆钮 `h-4 w-4`，圆钮直径 < 轨道高度，左右各留 2px），任意根字号下圆钮恒在轨道内。
   - props：`checked: boolean`、`onchange: (next:boolean)=>void`、`disabled?`、
     `ariaLabel?`、`title?`。`role="switch"` + `aria-checked` 由组件内部维护。
2. 把 `SettingsPanel.svelte` 远程控制处的内联开关替换为 `<Toggle>`（onchange 内沿用
   现有 `set_remote_enabled` invoke + `setSetting` + `refreshRemoteRunning` 逻辑）。

**⚠️ 待确认/范围说明（"智能体设置"开关）**：用户指出截图含第二处开关位于"智能体设置"，
但当前 `develop` 分支 src **不存在该面板**（"智能体"仅见于 `docs/plans/agent-teammate`，
功能在建；SettingsPanel 仅 appearance/language/font/terminal/extensions/debug 六区）。
本批次先交付可复用 `Toggle.svelte` 并修远程控制处；智能体设置面板一旦落地/指明入口，
直接套用同组件即可，无需再改组件本身。

**测试**：纯展示组件，靠组件结构正确性 + 运行时目测（含非 100% 缩放）。

---

## ② 工作区 `+` 按钮与左侧 tab 间距

**根因**：`+` 按钮经 `{#snippet trailingActions()}`（`src/routes/+page.svelte:1681`）渲染，
在 `WorkspaceTabs.svelte:336` 紧跟 tab 滚动区（`dndzone`，tab 间 `gap-1`），
trailing 包裹层无左间距 → `+` 紧贴最后一个 tab。

**修复**：给 `+` 按钮（`+page.svelte:1684` class）加 `ml-2`，或在 `WorkspaceTabs.svelte`
的 trailingActions 包裹 `<div class="shrink-0 rg-no-drag">` 加左间距。取前者（改动局部、
不影响其它使用者）。

**测试**：目测。

---

## ③ 每个 pane 切换终端类型（仅修原地重建，不加 split 兜底）

**根因**：`PaneShellSwitcher.svelte` 已存在于每个 pane 头部（`SplitContainer.svelte:659`），
`selectShell` 调 `change_pane_shell`，后者（`terminal.rs:64`）已做"拆 PTY→原地重建"。
但两个缺陷：
1. 后端 `change_pane_shell` **未持久化 `pane.shell_kind`**（对照 `create_pane_inner:130-137`
   有写）；且 `shell_kind` 未经 `get_pane_layout` 暴露给前端。
2. 前端 `PaneShellSwitcher.getCurrentLabel()` / `selectShell` 读 **全局** `$settingsStore.defaultShell`
   → 标签恒显示全局默认（非本 pane）、`if (shell.program === defaultShell) return` 导致
   无法把某 pane 切回全局默认 shell。

**决策（用户拍板）**：原地重建不存在失败场景，不加"split 新终端"兜底（YAGNI）。

**修复**：
- 后端：
  1. `change_pane_shell` 重建后写入 `pane.shell_kind = Some(shell)`（复用
     `create_pane_inner` 的持久化片段）。
  2. `get_pane_layout`（及其序列化的叶子结构）带上 `shell_kind`。需定位该序列化点
     （pane tree → PaneNode 的 Rust 侧 to-frontend 转换），给 leaf 加 `shell_kind`。
- 前端：
  1. `src/lib/types.ts` 的 `PaneNode` leaf 分支加 `shell_kind?: string`。
  2. `SplitContainer.svelte:659` 传 `currentShell={node.shell_kind}` 给 `PaneShellSwitcher`。
  3. `PaneShellSwitcher` 新增 prop `currentShell?: string`：
     - `getCurrentLabel()` 按 `currentShell`（回退全局默认）匹配 `shells` 取 label。
     - `selectShell` 的"已选中"判断与高亮改用 `currentShell`（而非全局默认）；
       相等才 `return`，从而可切回全局默认 shell。
     - 切换成功后本地乐观更新 + 等 `change_pane_shell` 落盘后由 layout 同步回真值。

**测试**：后端对 `change_pane_shell` 加单测（重建后 `pane.shell_kind` 已更新）；
若 `get_pane_layout` 序列化可纯逻辑断言则补一例。前端目测每个 pane 标签独立。

---

## ④ 终端类型检测补全（原生增强枚举）

**根因**：`ridge-core/src/commands/shell.rs::detect_available_shells` 仅扫 PATH，
Windows 分支只加 pwsh/powershell/cmd/git-bash/**单条** wsl/nu/clink。
对照截图的 Windows Terminal 列表，缺：各 WSL 发行版、VS 开发者命令提示符/PowerShell。

**决策（用户拍板）**：原生增强枚举（不依赖 Windows Terminal 安装）。Azure Cloud Shell
不做（WT 的云连接器，非本地 shell）。

**`ShellInfo` 带参扩展（关键前置）**：现有 `ShellInfo { id, label, program }` 三元组无法
表达 `wsl -d <distro>`、VS 的 `cmd /k VsDevCmd.bat`、`pwsh -NoExit -Command "...Launch-VsDevShell..."`
这类**带参启动**。决策：给 `ShellInfo` 加 `#[serde(default)] args: Vec<String>`，
并让 PTY 启动路径（`ensure_pane_pty_workspace` 的 shell → StructuredPtyCommand 解析）
支持带参。需核查 `change_pane_shell`/`create_pane` 当前只传 `program: String` 的链路，
评估是改为传 program+args 还是把 args 编码进一个可解析串——实现阶段细化，
优先**显式 program+args**，避免再解析空格/引号歧义。

**修复**（仅 Windows 分支增强，跨平台逻辑不变）：
1. **WSL 各发行版**：跑 `wsl.exe -l -q`（输出为 UTF-16LE，需解码 + 去除 NUL/空行），
   每个发行版生成 `{ id: "wsl-<distro>", label: "WSL: <distro>", program: wsl.exe, args: ["-d","<distro>"] }`。
   保留/替换原笼统 `wsl` 条目（若枚举到发行版则用具体条目，枚举失败回退原笼统条目）。
   `wsl.exe` 不存在则整段跳过。
2. **VS 开发者环境**：用 vswhere（`%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe`）
   定位 VS 安装根；存在 `Common7\Tools\VsDevCmd.bat` →
   "Developer Command Prompt for VS"（cmd + `/k "...VsDevCmd.bat"`）；
   存在 `Launch-VsDevShell.ps1`（或经 `Tools\Microsoft.VisualStudio.DevShell.dll`）→
   "Developer PowerShell for VS"（powershell/pwsh + 启动脚本）。vswhere/VS 缺失则跳过。
3. 现有 PATH 扫描与按 program 去重逻辑保留（带参条目去重需把 args 纳入键，避免同
   program 不同 distro 被误判重复）。

**测试**：`cargo test -p ridge-core`——纯函数部分（解析 `wsl -l` 输出、拼 ShellInfo）
可单测（喂模拟 UTF-16LE 字节断言发行版列表）；实际 vswhere/wsl 调用属环境依赖，目测。

---

## ⑤ 拖拽终端头部停靠失效 → 指针事件重写（保留 OS 文件拖放）

**根因**：`src/routes/+page.svelte:1141` 用 `getCurrentWebview().onDragDropEvent` 接收
**OS 文件拖放**（拖文件进终端插入绝对路径），这要求 Tauri `drag_drop_enabled=true`
（窗口 builder 未显式关闭 → 默认 true）。而该项在 Windows/WebView2 上会**屏蔽 webview 内
HTML5 DnD 的 `drop` 事件**——正是 `SplitContainer.svelte` 停靠覆盖层
（`draggable`/`ondragstart`/`ondragover`/`ondrop`，:557-597）所依赖的机制。
tab 重排能用是因为它走 svelte-dnd-action 的**指针**事件。二者在 WebView2 上互斥。

**决策（用户拍板）**：指针事件重写（`drag_drop_enabled` 保持默认，文件拖放不受影响）。

**修复**：
1. 新建 pane 拖拽控制器（`src/lib/actions/paneDockDrag.ts` 或同等 action/模块）：
   - `pointerdown`（pane 头部拖拽手柄）：记录 source paneId、起点坐标，**不立即**进入拖拽；
     `setPointerCapture` 保证后续 move/up 可靠（即便指针移到终端画布上）。
   - `pointermove`：超过移动阈值（如 4px）才正式开始 → 设 `paneDragSourceId.set(src)`；
     用 `document.elementFromPoint(clientX, clientY).closest('[data-pane-id]')` 命中目标 pane，
     `closest('[data-ws-tab-id]')` 命中工作区 tab；据目标 pane 矩形用现有 `regionAtPoint`
     算 region 并驱动 `dockHover`（覆盖层视觉复用 `dockHintClass`）。命中非活动 tab 时
     沿用 `HOVER_SWITCH_MS`（250ms）自动切工作区逻辑。
   - `pointerup`：命中有效目标 pane（且 ≠ source）→ 调现有
     `dockPane(src, targetId, region)`（后端 `dock_pane` 已支持跨工作区迁移）；
     清理 `paneDragSourceId`/`dockHover`/capture。`pointercancel` 同样清理。
   - 小阈值前的 `pointerup` 视为点击 → 沿用原 `activePaneId.set(node.id)` 聚焦语义。
2. `SplitContainer.svelte`：
   - 叶子容器（:547 的 wrapper）加 `data-pane-id={node.id}`，供 `elementFromPoint` 命中。
   - pane 头部拖拽手柄（:582-598）：移除 `draggable`/`ondragstart`/`ondragend`，
     改挂指针控制器（`use:` action 或 `onpointerdown`）。
   - 停靠覆盖层（:551-577）：移除 `ondragover`/`ondragleave`/`ondrop`；`dockHover` 改由
     指针控制器写入（仍据 `$paneDragSourceId` 决定是否渲染覆盖层）。保留 `regionAtPoint`/
     `dockHintClass`/`onDockDrop` 中的 dockPane 调用（迁进控制器）。
3. `WorkspaceTabs.svelte`：tab 元素加 `data-ws-tab-id={ws.id}`；移除 pane 跨工作区用的
   `ondragover`/`ondragleave`（`onTabDragOver`/`onTabDragLeave`），其 hover-switch 逻辑迁进
   指针控制器（命中 tab → 起定时器切工作区）。**tab 重排仍用 svelte-dnd-action 不动**。
4. `paneDragSourceId` store、`dockPane`、后端 `dock_pane` 全部不变——只换驱动源。

**测试**：可下沉的纯逻辑（region 计算、阈值判定、tab 命中）加 TS 单测；拖拽手感/跨工作区
切换/松手停靠需运行时目测（含拖到终端画布上的 capture 可靠性、文件拖放仍工作）。

---

## 提交计划（每项独立 commit）

1. `feat(ui): 抽取可复用 Toggle 组件并修远程控制开关圆钮溢出` —— ①
2. `fix(workspace): + 按钮与 tab 间距` —— ②
3. `fix(terminal): per-pane shell 切换持久化 shell_kind + 标签按 pane 显示` —— ③
4. `feat(shell): 枚举 WSL 各发行版 + VS 开发者环境（ShellInfo 带参）` —— ④
5. `fix(workspace): pane 头部拖拽停靠改指针事件（绕开 WebView2 DnD 屏蔽）` —— ⑤

实现顺序建议：② → ① → ③ → ④ → ⑤（由易到难；④/⑤ 牵动面较大放后）。

## 开放项 / 风险

- **①** 智能体设置面板不在本分支，第二处开关待该面板落地后套用同组件。
- **③/④** 共享一处链路：`shell_kind`/带参 shell 都涉及 `ensure_pane_pty_workspace` 的
  shell→命令解析。④ 给 `ShellInfo` 加 `args` 后，③ 的 `change_pane_shell` 与
  `PaneShellSwitcher.selectShell` 仅传 `program` 是否够用需一并核查（带参 WSL/VS 条目
  被选中时要把 args 也传给 `change_pane_shell`）。实现 ④ 时一并打通。
- **⑤** WebView2 下 `setPointerCapture` + `elementFromPoint` 在指针悬于 GPU 画布上的命中
  可靠性需运行时验证；若 capture 期 `elementFromPoint` 返回画布而非覆盖层，靠
  `closest('[data-pane-id]')` 上溯仍能拿到目标 pane（叶子 wrapper 是画布祖先）。
