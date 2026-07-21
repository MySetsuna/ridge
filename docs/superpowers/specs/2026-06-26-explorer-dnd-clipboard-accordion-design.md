# 资源管理器三项改造设计：拖入终端走粘贴 / 系统剪贴板过期判定 / 手风琴顶节点常驻

- 日期：2026-06-26
- 状态：设计待用户复核
- 范围：3 个相互独立的改动，将拆成 3 个独立 commit

## 背景与目标

本轮针对终端 + 资源管理器（文件树）交互的三处不满，均为对既有功能的修正/改造，互不依赖：

1. **文件拖入终端 → 走 bracketed paste**：当前拖放只是把绝对路径原样写进 PTY，TUI（Claude Code 等）不把它当"粘贴"，因此拖入的图片无法被识别为图片附件。
2. **系统剪贴板被内部剪贴板遮住**：在 Windows 资源管理器复制文件后粘进文件树，粘出来的却是上次在 ridge 内复制的旧文件——内部剪贴板陈旧未失效，挡住了系统剪贴板。
3. **手风琴顶节点常驻**：希望工作区头与 cwd 头始终可见（多个 cwd 可同时展开），文件列表在各自区域内部滚动，不把顶层节点推走。

> 备注：用户最初提到"参考 c:\code\moon-catcher 的折叠面板文件目录"。经两次核查（含 git 历史，仅 2 个 commit）+ 全 `c:\code` 扫描确认：moon-catcher 是标准 Vike Star Wars 脚手架，整个 `c:\code` 都不存在该参考实现。故 #3 按用户口述目标"滚动区内始终展示顶层节点（工作区 + cwd）"直接定义，不依赖外部参考。

---

## Feature 1：文件拖入终端 → 全部走 bracketed paste

### 现状

- OS 原生拖放：`src/routes/+page.svelte` `insertDroppedPaths()`（约 1098 行）——对每个绝对路径加引号（含空格时）、空格连接、末尾补空格，经 `invoke('write_to_pty', ...)` **原样写入** PTY。
- 文件树拖到终端 pane：`src/lib/components/FileTree.svelte` `pasteToTerminal()`（约 433 行）——同样的引号+空格+`write_to_pty` 逻辑。
- 真正的图片感知粘贴管线在 `RidgePane.svelte` `pasteFromClipboard()` → `pasteIntoPane()` → `manager.paste(paneId, text)`（`src/lib/terminal/manager.ts:2925`），后者在 `?2004` 模式下用 bracketed-paste 标记包裹，TUI 据此识别为"粘贴"。

### 决策（用户已选：全部走 bracketed paste）

把"文件拖入终端"统一改为走终端管理器的 bracketed-paste 接口 `manager.paste(paneId, text)`，**不再** `write_to_pty` 原样写：

- 所有拖入路径（不区分是否图片）都走 `manager.paste`。图片路径经 bracketed-paste 后，Claude Code 等 TUI 即可识别为图片附件——这正是"先尝试粘贴，因为可能是图片"的目的。
- 多个文件：以单个空格分隔、合并为**一次** bracketed paste（沿用既有"空格连接"形态）。
- **不再做引号包裹**：与既有 clipboard paste 管线保持一致（`pasteFromClipboard` 也粘裸文本），且裸路径对图片识别最可靠（`resolve_pasted_image_path` 在文本侧也是先 `trim_matches('"')`）。

### 改动点

- `+page.svelte`：`insertDroppedPaths()` 改为按落点解析目标 paneId 后调 `manager.paste(pid, text)`（`text` = 裸路径，多文件空格连接），删除 `write_to_pty` 与引号逻辑。需引入 terminal manager 单例。
- `FileTree.svelte`：`pasteToTerminal()` 同步改为 `manager.paste(paneId, text)`，与 OS 拖放保持一致（保证两条"拖入终端"路径行为统一）。

### 权衡 / 风险（已与用户确认接受）

- 裸路径不再加引号：把含空格的路径拖进**命令行当参数**时，shell 会按空格拆词。用户在两种处理方式中明确选择"全部走 bracketed paste"，接受此代价。多图同时拖入时若路径含空格，多路径解析也可能有歧义——属同一权衡。

### 可测试性

- 抽一个纯函数 `formatDroppedPathsForPaste(paths: string[]): string`（裸路径、空格连接），vitest 单测；副作用（解析 paneId、调用 manager）留在组件层。

---

## Feature 2：系统剪贴板序列号过期判定 + 右键"粘贴"

### 根因

`src/lib/components/Explorer.svelte` `pasteClipboard()`（约 454 行）逻辑为"内部剪贴板优先，仅当其为空才回退读系统剪贴板"：

```js
let clip = get(explorerClipboard);
if (!clip || clip.paths.length === 0) {        // ← 只有内部为空才读系统
  const sysFiles = await invoke('read_clipboard_file_paths');
  if (sysFiles?.length) clip = { paths: sysFiles, mode: 'copy' };
}
```

用户上次在 ridge 内复制过，内部 `explorerClipboard` 仍存旧路径未清 → `clip` 非空 → 永不回退读系统剪贴板 → 在 Windows 资源管理器的复制被"挡住"，粘出旧文件。

> 注意：ridge 内**复制**时会同时写系统剪贴板（`write_clipboard_file_paths` 写 CF_HDROP + 文本镜像）；**剪切**时**不**写系统剪贴板（见 Explorer.svelte Ctrl+C 分支注释）。因此"系统剪贴板"无法表达 ridge 的剪切态——这是必须用序列号而非"系统优先"启发式的原因。

### 决策（用户已选：序列号判过期）

用 Windows 剪贴板**序列号**（`GetClipboardSequenceNumber`，内容每次变化即自增、无需打开剪贴板）判定内部剪贴板是否被外部应用改写过：

- 数据结构：`ExplorerClipboard`（`src/lib/stores/fileExplorer.ts:836`）新增 `seq: number`。
- 后端：新增 Tauri 命令 `read_clipboard_sequence() -> u32`（Windows 调 `GetClipboardSequenceNumber`；非 Windows 返回 0）。放在 `src-tauri/src/commands/clipboard_files.rs`。
- 复制（Ctrl+C）：写完 `write_clipboard_file_paths` 后读一次序列号，连同 `{paths, mode:'copy', seq}` 存入内部剪贴板。
- 剪切（Ctrl+X）：读当前序列号，存 `{paths, mode:'cut', seq}`（不写系统剪贴板）。
- 粘贴（`pasteClipboard`）：
  1. `cur = read_clipboard_sequence()`。
  2. 若内部 clip 存在且 `cur === clip.seq` → 内部剪贴板权威（外部未改写过），按其 `mode`（copy/cut）执行。
  3. 否则（序列号变了或无内部 clip）→ 内部作废：读 `read_clipboard_file_paths()`；非空 → `{paths, mode:'copy'}`；为空 → 退回内部 clip（若有）。

正确性覆盖：

| 场景 | 结果 |
|---|---|
| ridge 复制后直接粘 | seq 未变 → 用内部（复制） ✓ |
| ridge 复制后，外部又复制别的，再粘 | seq 变 → 用系统（复制外部文件） ✓ 修掉本 bug |
| ridge 剪切后直接粘 | seq 未变 → 用内部（剪切/移动） ✓ |
| ridge 剪切后，外部复制别的，再粘 | seq 变 → 用系统（复制外部） ✓ |

### 右键"粘贴"（用户已选：加）

当前只能 Ctrl+V（且要求文件树持有键盘焦点），右键菜单无"粘贴"。补充可发现性：

- 抽公共动作：把 `pasteClipboard` 重构进 `fileExplorer` store，签名形如 `pasteClipboard(target?: { columnId?: string; targetPath?: string }): Promise<void>`，供两处复用（保留 Explorer 的 Ctrl+V 调用路径）。
- 节点右键（`FileTree.svelte` `handleContextMenu`）：加"粘贴"项，目标目录 = 右键节点（是目录则其自身，是文件则其父目录）。
- cwd 空白右键（`Explorer.svelte` `showCwdContextMenu`）：加"粘贴"项，目标 = `col.cwd`。
- "粘贴"项始终显示（无法同步探测系统剪贴板内容）；点击时读剪贴板，无内容则静默 no-op。
- i18n：新增 `explorer.ctxPaste`，与 `explorer.ctxCopy` 同步补全所有语言（`src/lib/i18n/messages/explorer.ts`）。

### 可测试性

- 抽纯函数 `resolveActiveClipboard(internal: ExplorerClipboard | null, currentSeq: number, systemFiles: string[]): ExplorerClipboard | null` 承载上面的判定逻辑，vitest 单测覆盖四种场景。
- 序列号 Rust 命令属平台代码（winapi），不下沉单测；`sanitize_file_list` 既有测试不动。

---

## Feature 3：手风琴 — 多开 + 顶节点常驻（flex 布局重构）

### 现状

`src/lib/components/Explorer.svelte` 标记结构：`.explorer`（`use:overlayScroll` 整树单一滚动容器）→ 每工作区 `.explorer-workspace`（头 `sticky top-0 z-20`）→ 每 cwd `.explorer-section`（头 `sticky top-8 z-10`）→ body。非活动工作区 body 有 `max-h-[32vh] overflow-y-auto` 上限；活动工作区 body 不设上限、自由增高。问题：活动工作区里 cwd 文件多时，整段很高，滚动会把其它 cwd / 工作区的顶层头推出视口。

### 决策（用户已选：多开 + 顶节点常驻）

改为 **flex 列布局**，使所有顶层节点头永不滚走，文件列表在各自 body 内部滚动：

- `.explorer` → `display:flex; flex-direction:column; height:100%; min-height:0`。
- 工作区头、cwd 头 → `flex:0 0 auto`（固定，永远在流中、不随文件列表滚动）。
- 每个**展开**的 cwd body → `flex:1 1 0; min-height:0; overflow-y:auto`（内部滚动，`rg-scroll` 样式）。多个展开 body 平分剩余纵向空间。
- 折叠的 cwd → 无 body。工作区折叠 → 隐藏其下所有 cwd 段。
- 移除"活动 vs 非活动工作区"的 `max-h-[32vh]` 特例，统一由 flex 分配。
- 滚动模型迁移：从"整树一个 OverlayScrollbars"改为"每个 body 原生 `overflow-y:auto` + `rg-scroll`"，避免 N 个 OverlayScrollbars 实例。

### 溢出兜底

顶层头很多（多工作区 × 多 cwd）时，头本身可能超出视口高度：

- 每个展开 body 给一个 `min-height`（如 `6rem`），保证不被压成 0。
- 当 `头总高 + body 最小高之和 > 容器`：`.explorer` 外层回退为可滚动（保留 sticky 作为二级保障，让工作区头/当前 cwd 头仍钉住）。
- 即 flex 为常态最优解；sticky + 外层滚动为极端数量下的降级，不冲突。

### 改动点

- `Explorer.svelte`：`.explorer` 容器（去 `use:overlayScroll` 或改其职责）、`.explorer-workspace` / `.explorer-section` / body 的布局类与 `<style>`；删除 `max-h-[32vh]` 三元；body 加 `overflow-y:auto rg-scroll min-h-*` 与 flex 类。
- 慢加载进度条、pane 标签条、SidebarPluginRegion 等子元素位置不变，只随 flex 重排。

### 权衡 / 风险

- 多个 cwd 同时展开时，每个 body 高度 = 剩余空间 / 展开数，单个可能偏矮——属"多开 + 全部可见"的固有取舍；min-height + 外层滚动兜底缓解。
- 等高分配（`flex:1 1 0`）实现最简；按子节点数加权为可选增强，本轮不做（YAGNI）。

### 可测试性

- 纯布局/CSS 改动，无纯逻辑可单测；依赖手动验证（用户偏好：plan 不写手测清单）。

---

## 跨切面

- **提交策略**（用户偏好）：3 个独立 commit，一功能一 commit：
  1. `feat(terminal): 文件拖入终端改走 bracketed paste（OS 拖放 + 文件树拖拽统一）`
  2. `fix(explorer): 剪贴板序列号判过期，修系统复制被内部剪贴板遮住 + 右键粘贴`
  3. `feat(explorer): 手风琴改 flex 布局，顶层节点（工作区/cwd）常驻可见`
- **i18n**：仅 Feature 2 新增 `explorer.ctxPaste`，全语言补齐。
- **平台**：剪贴板 CF_HDROP / 序列号均 Windows 实现；非 Windows 退化（读空 / seq=0），与既有 `clipboard_files.rs` 约定一致。

## 不在本轮范围

- mac/Linux 文件列表剪贴板互通。
- 拖入终端的多图"逐图分次粘贴"优化（本轮按空格合并一次粘贴）。
- 手风琴 body 高度按子节点数加权分配。
- 文件树 HTML5 DnD → 指针事件的遗留迁移（与本轮无关）。
