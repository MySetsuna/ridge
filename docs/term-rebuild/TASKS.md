# Ridge Term — 任务清单与进度跟踪

> 创建日期：2026-05-03
> 上次更新：2026-05-03
>
> 本文档是 [OVERVIEW.md](OVERVIEW.md) §3 进度表的可执行版本：包含具体任务、责任划分、验收标准、进度记录。
> 与 OVERVIEW.md 的关系：OVERVIEW 描述目标与里程碑；本文档跟踪"下一步做什么"。
> 编辑约定：完成的任务**不删**，改成 ✅ 并在「进度记录」尾部追加一行（日期 + 摘要 + 提交点）。

---

## 0. 当前进度快照（2026-05-03 末次更新）

| Round | 范围 | 实际状态 | 备注 |
|---|---|---|---|
| 1 | VT 内核骨架 | ✅ | 27 单测通过 |
| 2.1 | wcwidth + alt screen + DECSTBM + DEC modes | ✅ | 28 单测通过 |
| 2.2 | 渲染抽象 trait + Canvas2D 后端 | ✅ | `src/render/{backend.rs, canvas2d.rs, renderer.rs}` 全部落地 |
| 2.3 | JS 表面 API（write/onData/resize/key encoder） | ✅ | `src/lib.rs` `TerminalKernel`/`RenderHandle` wasm-bindgen 导出 |
| 2.4 | TerminalManager + RidgePane | ✅ | `src/lib/terminal/manager.ts` + `src/lib/components/RidgePane.svelte`；`SplitContainer` 直接 import RidgePane（PaneRouter 已删除） |
| **— 协议补全 patch（OVERVIEW §4 列表）** | ECH/ICH/DCH/REP/DECSCUSR/DSR/DA/?2026/?1004/OSC0/1/2/7/8 | ✅ | 113 单测 + 22 集成测试，pending_response + pending_events 通道接通 |
| **— round 4 部分提前** | 鼠标拖选（含 word/line/shift-click）、Ctrl+F 搜索、IME v2 cursor-tracking、Ctrl+click 链接 | ✅ 2026-05-02 | INTEGRATION_R2_4.md 顶部已加 2026-05-03 状态横幅同步该清单 |
| 3 | WebGPU 后端 + 字形 atlas | ⏳ 未开始 | `RenderBackend` trait 就绪等实现，详见 §4.1–4.4 |
| 4 | IME v3 + reflow + scrollback bridge + 链接 affordance 收尾 | ⏳ 部分完成 | reflow Phase 1 (用户) + 反向 scrollback bridge + 选择/搜索/链接 ✅；Phase 2 / IME v3 / grapheme 远期 |
| 5 | OSC UI 接入收尾 | ✅ 实质完成 | §3.1 标题渲染验证通过；§3.2 HyperlinkOpen/Close 决定删除；§3.3 Bell 音频远期 |
| 6 | parking lot + 双 scrollback 去重 | ✅ 实质收尾 | §5.1 park/unpark + ptyBridge ✅；§5.2 scrollback 去重决定不做（小 buffer 与 push_front evict 冲突，详见 §5.2） |
| 7 | 删 xterm | ✅ 基本完成 | npm 依赖、Pane.svelte / PaneRouter / terminalRegistry 全删；CLAUDE.md 描述对齐；剩 §7.2 浏览器实跑回归（需用户） |

**结论：所有 round 5/6/7 代码层面收尾完毕**。剩余主线只有：

1. **§4 Round 3** — WebGPU + 字形 atlas + 共享 surface（大块工程，需要用户给方向：要不要现在启动？）
2. **§7.2 浏览器实跑回归** — 需要用户 `pnpm tauri dev` 跑 INTEGRATION_R2_4.md §Step 7-8 视觉清单
3. **§1.5 / §2.2 / §2.4 / §3.3** — 全部 LOW / 远期 deferred（见各节决策）
4. **§1.14** — `PaneState::Starting` 后端 / 前端 半实现 gap（2026-05-03 audit 发现，等用户对 teammate 流程时序判断）

---

## 1. 立即修复（来自 2026-05-03 code review）

每条都给出文件位置 + 修法摘要 + 验收。

### 1.1 [HIGH] `pane-pty-closed` 重建路径不完整 ✅ 2026-05-03

- **文件**：`src/lib/components/RidgePane.svelte:281-291`
- **现象**：监听到 `pane-pty-closed` 后只调 `invoke('create_pane', ...)`，没有重新跑 `activate_pane_pty`、不重放 scrollback。后端的确创建了 PTY，但前端 kernel 永远不会收到 PTY 字节（listener 已经存在，但 PTY 没被 activate，pty-output 不会发）。
- **修法（已实施）**：把 listener 改为 `async`，依次 `await invoke('create_pane', ...)` → `await invoke('activate_pane_pty', ...)`，每步前后检查 `alive`。`pty-output` listener 不重注册（channel 名按 paneId 命名，旧 listener 自动接到新 PTY 的字节）；scrollback 不重放（wasm kernel 已持有上一会话内容，重放会重复）。
- **验收**：手动 kill shell 进程（关掉 terminal cmd /c 之类），pane 应该自动出现新 prompt（不是冻在最后一行）。**待 §7.2 浏览器实跑验证**。

### 1.2 [MEDIUM] `feed()` 中 `takePendingEvents()` 条件丢事件 ✅ 2026-05-03

- **文件**：`src/lib/terminal/manager.ts:442-445`
- **现象**：`if (entry.eventHandler)` 包住 `takePendingEvents()`。如果初次挂载竞态导致 `feed()` 在 `onEvent()` 之前发生（目前调用顺序保证了不会，但未来重排顺序），events 会在 wasm 侧 `pending_events` 队列累积，下一次 `feed()` 才被批量送达——彼时屏幕状态已经向前推进，CWD/标题/超链接定位错乱。
- **修法（已实施）**：移除 `if (entry.eventHandler)` 分支，无条件调 `takePendingEvents()` 抽空队列；有 handler 就 dispatch，无 handler 且事件非空时 dev-only `console.warn`，确保排序 bug 在开发期被看见。
- **验收**：把 `onEvent` 注册移到 `feed()` 之后，dev console 应出现 warn；正常顺序下 OSC 7 行为不变。

### 1.3 [MEDIUM] `bellFlashTimer` 未在 `onDestroy` 清理 ✅ 2026-05-03

- **文件**：`src/lib/components/RidgePane.svelte:64-69, 285-296`
- **现象**：pane 在 Bell 闪烁的 120ms 内被销毁会留下悬挂 setTimeout，回调向已卸载组件写 `bellFlash`。Svelte 5 runes 容忍这种写入，但仍是悬挂资源。
- **修法（已实施）**：`onDestroy` 中 `if (bellFlashTimer !== null) { clearTimeout(...); bellFlashTimer = null; }`。
- **验收**：DevTools 内存快照不再有遗留 timer。

### 1.4 [LOW] `?2026` sync output 超时后无 cool-down ✅ 2026-05-03

- **文件**：`src/lib/terminal/manager.ts`（`PaneEntry` 接口 + rAF tick）
- **现象**：超时分支会在每帧都 fall-through 到 `entry.handle.render(...)`——`now - syncStart >= TIMEOUT` 一直成立，导致 60 fps burst render 直到 TUI 退出 sync。注释里写的「only one render per cycle past the timeout」实际没实现。
- **修法（已实施）**：`PaneEntry` 加 `syncTimeoutRendered: boolean` 字段。tick 在超时分支判断：若已渲染过就 `continue` 跳过本帧；否则置 `true` 并 fall-through 渲染一次（best-effort frame）。kernel 退出 sync 时连同 `syncStart = null` 一起清掉 flag。
- **测试**：svelte-check 0 错误。

### 1.5 [LOW] `canvas2d.rs::measure_font` 用 `'M'` 测宽 ⏳

- **文件**：`src/render/canvas2d.rs:98-103`
- **影响**：CJK fallback 字体下宽字符可能错位 1-2 px。
- **决定**：暂不改，round 3 WebGPU 时一并重做 metrics。**保留追踪**。

### 1.6 [HIGH] Selection overlay alpha 叠加 ✅ 2026-05-03

- **文件**：`packages/ridge-term/src/render/renderer.rs::tick`
- **现象**：用户报告「选中行颜色一直叠加」。`draw_selection_overlay` 每帧用 0x60 alpha 画一遍，但只有 dirty rows 会被 `draw_row` 重新画底色——选区内未变化的行（如不闪烁的非光标行）保留上一帧像素 + 上一帧 overlay，本帧 overlay 再叠一层 alpha。每次 cursor blink tick 多叠一层，几秒内变得近乎不透明。
- **修法（已实施）**：tick() 在 dirty_rows 计算后、是否 early-return 之前，若 `!full_redraw_pending && !dirty_rows.is_empty() && selection.is_some()`，把所有 selection 覆盖行追加进 dirty_rows。这样所有 overlay 影响行底层都被新画一次（opaque bg），overlay 永远只叠一层。idle 选区（dirty_rows 空）不触发，保留上帧像素，正确。
- **测试**：111 单测 + 22 集成 全绿。

### 1.7 [HIGH] 选中背景色不符合主题 ✅ 2026-05-03

- **文件**：新增 `src/lib/terminal/themeBridge.ts`，wire from `src/routes/+page.svelte`
- **现象**：用户报告「选中背景色不符合主题」。根因：`backend.rs::Theme::default_dark` 的 `selection_bg = [0x55, 0xaa, 0xff, 0x60]` 是固定蓝色，与 Ridge 绿色 accent 不搭。`Theme::apply_partial` 接受 `selectionBackground` 等 xterm.js 风格 key，但 manager.ts 的 `ManagerOptions` 默认 theme 字段空——`attach()` 的 `if (this.opts.theme)` guard 永不触发，wasm 留在编译期默认。
- **修法（已实施）**：
  - 新文件 `themeBridge.ts`：用临时 canvas 2d 把任意 CSS 颜色（`#xxx` / `rgb()` / `rgba()` / 命名色）规范化成 `#RRGGBBAA`，读 `--rg-term-bg` / `--rg-fg` / `--rg-accent` / 可选 `--rg-selection-bg`，构造 xterm.js shape 主题对象，调 `manager.setTheme`。
  - selectionBackground 默认从 accent 派生（accent 颜色 + alpha 0x3d ≈ 24%）；用户加 `--rg-selection-bg` 即可显式覆盖。
  - cursor / cursorAccent / hyperlinkColor 一并 wire（accent / bg / accent）。
  - `+page.svelte` onMount 动态 import + 调 `setupTerminalThemeBridge()`：订阅 `settingsStore`，theme 切换时重新 push；fingerprint 防重复 push。
- **测试**：svelte-check 0 错误；wasm pkg 重建。

### 1.12 [HIGH] Split drag 自动 coupling 牵连非相邻 pane（用户解读 C）✅ 2026-05-03

- **文件**：`src/lib/stores/paneTree.ts::startSplitResizeDrag`
- **现象**（用户在前一会话已确认解读 C）：嵌套布局 `(A|B) / (C|D)`，拖 A/B 之间的线时，A 和 B 受影响（正常），但 **C 和 D 也被联动 resize**——用户不要这个行为。
- **根因 trace**：`startSplitResizeDrag` 默认把以下都塞进 `refs`（全部参与 ratio 更新）：
  1. `ui.orthogonals`（4-way 正交联动）
  2. `ui.snapState.coupledSplitters`（snap 状态记录的耦合 splitter）
  3. `coupledSameAxis`（同轴对齐的 sibling，C/D vs A/B 在 50/50 时正好命中——`perpDistance ≤ 30 px` && `distToBC ≤ 50 px` 即激活）
  4. `coupledOrthoSiblings`（正交 splitter 的同轴 sibling，4-way 完整联动）

  其中第 3 条是用户报告的 C/D 联动主因——`(A|B)` 和 `(C|D)` 的内部 splitter 中线在 50/50 时恰好对齐，鼠标在 A/B 端点附近就触发联动。

- **修法（已实施）**：
  - 新增模块级常量 `SPLIT_DRAG_AUTO_COUPLING_ENABLED`，默认 `false`。
  - `refs` 默认只含 `[ui.primary]`；`is4WaySnap` / `snapState.coupledSplitters` / `coupledSameAxis` / `coupledOrthoSiblings` 四条联动路径全部 gate 在该常量后。
  - 视觉 attractor（`attractOnlySameAxis`）保留——拖动时仍能看到「附近可对齐的 splitter」高亮提示，只是不再自动联动。
  - 设为 `true` 即可恢复旧 snap 行为（保留代码以备未来加 settings UI）。
- **效果**：`(A|B) / (C|D)` 拖 A/B 现在只动 A 和 B 的 ratios。C 和 D 的容器尺寸不变，ResizeObserver 不 fire，PTY 不 SIGWINCH。
- **测试**：svelte-check 0 错误（2 处 pre-existing 警告无关）。

### 1.11 [HIGH] Ctrl+C 触发 file tree 过度刷新（paneCwdStore fan-out）✅ 2026-05-03

- **文件**：`src/lib/stores/paneTree.ts::setPaneCwd` + 三处 bulk merge 站点（`refreshWorkspaces` / `switchWorkspace` / `loadSavedWorkspaces`）
- **现象**：用户报告——终端 Ctrl+C 时 file tree 一直在重载。
- **根因 trace**：
  1. Shell 收到 0x03 → SIGINT → 重画 prompt → 发 OSC 7（cwd 报告）。
  2. 后端 emit `pane-cwd-changed-{ws}-{pane}` 事件。
  3. `paneTree.ts:1352` 监听器调 `setPaneCwd(ws, pane, cwd)`，cwd 与之前**一模一样**（用户没 cd 过）。
  4. 旧 `setPaneCwd` 无脑 `paneCwdStore.update((s) => ({ ...s, [key]: normalized }))`——**无论值有没有变**都返回新对象引用。
  5. Svelte writable 用 `===` 比较，新引用 ≠ 老引用 → 所有订阅者被 fire（Explorer 的 `$effect`、SCM、SidebarPlugin、SearchSidebar、…）。
  6. Explorer 的 `$effect` 重跑 `syncWithPaneCwds` → fileExplorerStore 重 update → 文件树视图重渲染。每次 Ctrl+C 都触发一遍。
- **修法（已实施）**：
  - `setPaneCwd` 加 identity-preserving 提前返回：`if (store[key] === normalized) return store;`。Svelte writable 看到同引用就跳过订阅者通知。
  - 抽 `mergePaneCwds(target, additions)` 助手——只在某个键值真的变化时才克隆 + 写入；否则原样返回。
  - 三处 `paneCwdStore.update((s) => ({ ...s, ...cwds }))` 改用 `mergePaneCwds`（`refreshWorkspaces` / `switchWorkspace` / `loadSavedWorkspaces` 三处工作区切换/重载路径）。
  - `syncPaneLayoutFromBackend`（line 849）和 `closeWorkspace`（line 1071）已经手写了 `mutated` 守卫，原本就 OK，未动。
- **效果**：cwd 没变 → store 不 fire → 所有订阅者（包括 file tree、SCM、SidebarPlugin）静默。**所有下游消费者无需各自加 dedupe**——在源头止血是最干净的方案。
- **测试**：svelte-check 0 错误（2 处 pre-existing 警告无关）；浏览器实跑验证留给 §7.2。

### 1.10 [MEDIUM] Reflow cursor 边界 + pending_wrap 保留 ✅ 2026-05-03

- **文件**：`packages/ridge-term/src/term/grid.rs::reflow_primary`
- **现象**（review 提的另一处 [MEDIUM]）：原代码在结尾无条件 `pending_wrap = false`。当 cursor 在原 grid 行尾（col == cols-1）且 pending_wrap=true 时，reflow 后定位到新行尾但丢了 pending_wrap——下一字符会覆写最后一格而非换行。
- **根因**：(1) `cursor_logical_offset = current.len() + cursor_src_col` 没把 pending_wrap 折进 offset（pending_wrap 概念上把 cursor 停在 "col cols 的虚拟位置"）。(2) 结尾 `pending_wrap = false` 硬编码。
- **修法（已实施）**：
  - 抓 `cursor_src_pending_wrap = self.primary.cursor.pending_wrap`。
  - stitch 时若 cursor row 有 pending_wrap，offset += 1 → 指向虚拟 past-end。
  - push 后 clamp：若 anchor cell 是空格被 trim 掉，offset > pushed_len，clamp 到 pushed_len（rare case 防御）。
  - 引入 `cursor_pending_wrap` 局部 bool；end-of-line 分支在 `used == 0 && line.len() > 0` 时设 true，结尾按之赋值。
- **测试**：新增 `reflow_preserves_pending_wrap_at_exact_boundary`（10 字符 / 10 col，resize 5 col，cursor (1,4) + pending_wrap=true，下一 print 'x' 必须 wrap 到 row 2）+ `reflow_no_pending_wrap_when_line_doesnt_fill_last_row`（7 字符 / 10 col，resize 5 col，cursor (1,2)，pending_wrap=false）。113 lib + 22 integ 全绿。

### 1.9 [HIGH] Canvas resize 死循环：第一次 fit 后 canvas 被 freeze ✅ 2026-05-03

- **文件**：`packages/ridge-term/src/render/canvas2d.rs::resize_surface`
- **现象**：用户报告「终端窗口 resize 没有真正让内部终端 resize」。窗口 / splitpanes 拖动后，容器视觉上变了，但 cols/rows 不变、PTY 不收 SIGWINCH、shell prompt 不知道新尺寸。
- **根因**：
  1. `manager.ts::attach` 创建 canvas 时设 `style.cssText = 'display:block; width:100%; height:100%;'`——意图让 canvas 自动跟随容器。
  2. 首次 `fitPane` 触发 `handle.resize(wCss, hCss, dpr)` → `resize_surface` 内部写 `style.width = "{N}px"` / `style.height = "{N}px"`，**覆盖了 100%/100%**。
  3. 之后容器 resize，ResizeObserver 仍然触发，但 canvas 现在是固定 px 尺寸，不再跟容器伸缩。
  4. `fitPane` 读 `canvas.getBoundingClientRect()` 拿到**冻结值**，`sizeChanged === false`，函数直接 return。`resizeHandler`（PTY IPC）和 `kernel.resize` 永不被调用。
- **触发条件**：用户最近为支持 `paddingPx` 把 fitPane 从「读 container」改为「读 canvas」（`manager.ts:879` 注释）。改之前 container 永远是 live 的；改之后 canvas frozen 暴露出 `resize_surface` 的覆写问题。
- **修法（已实施）**：`resize_surface` 不再写 `"{N}px"`，改写 `"100%"`。CSS 维度持续 100% 跟容器；HTML width/height 属性（device 像素 backing buffer）由 `set_width`/`set_height` 单独管理，DPR 变化只更新 backing 不动 CSS。
- **测试**：111 lib + 22 integ 全绿（无 wasm 端单测覆盖此路径，验证靠浏览器实跑——属 §7.2 范围）；svelte-check 无关。

### 1.8 [MEDIUM] Resize reflow 宽字符跨切片边界损坏 ✅ 2026-05-03

- **文件**：`packages/ridge-term/src/term/grid.rs::reflow_primary`
- **现象**（review 发现）：reflow 切片按 `(start + new_cols).min(line.len())` 硬切。若切点正好落在 CJK / emoji 宽字符上（lead width=2 在 row 末尾，continuation half width=0 在 next row 开头），渲染时 lead 占 1 列、半字符孤立留在下行——视觉错位 + 选区/复制错乱。中文用户高频触发。
- **修法（已实施）**：切片处加 `if end < line.len() && end - start >= 2 && line[end-1].width == 2 { end -= 1; }`。宽字符整体推到下行，本行末空一格（与 xterm 右边距处理一致）。同时把 cursor 检查从 `< start + new_cols` 改为 `< end`，确保 freed 单元格的 cursor 偏移落到下行。
- **测试**：新增 `reflow_keeps_wide_char_intact_at_boundary`：80 cols / 39 ASCII + 「中」 + b → resize 40 cols → 行 0 为 39a + 空格（wrap=true），行 1 为「中」（lead）+ continuation + b。111 lib 测试全绿。
- **未做**：cursor 在 `line.len() % new_cols == 0` 边界 + pending_wrap 状态保持（review 提的另一处 [MEDIUM]，单独跟踪）。

### 1.13 [LOW] `cargo test --lib` 中 `commands/project.rs` + `git.rs` async 测试预存编译错误 ✅ 2026-05-03

- **背景**：本会话审计 dead code 时，`cargo test --manifest-path src-tauri/Cargo.toml --lib` 报 23 个错误。`commands/project.rs::{delete_path, copy_path, move_path}` 9 条测试 + `commands/git.rs::find_git_repos_below` 5 条测试（共 14 条 async 函数测试）走 `#[test]` + 同步 `.unwrap()` 语义，但函数已是 `pub async fn`。
- **修法（已实施）**：每条测试 `#[test]` → `#[tokio::test]`、`fn name(` → `async fn name(`、production-call 后插 `.await`。tokio 已配 `macros + rt-multi-thread`（Cargo.toml 验证），无需改 dep。
- **结果**：`cargo test --manifest-path src-tauri/Cargo.toml --lib` **73 passed; 0 failed; 0 ignored**，含 9 条 project + 5 条 git 修复后的测试全部通过。`cargo check --lib`（生产 profile）维持 0 警告。

### 1.14 [LOW] `PaneState::Starting` 后端 / 前端 半实现 gap ⏳

- **背景**：`state.rs:73-80` 定义 `PaneState { Idle, Busy, Starting }` + 注释「Pane 正在启动中（agent register 已发但 PTY 还没收到首条 prompt 输出时使用）」。前端有完整 UI affordance：`SplitContainer.svelte:592-599` 当 `agentState === 'starting'` 时渲染琥珀色脉动 STARTING badge；`src/lib/types.ts:9` 把 `'starting'` 列入 `AgentState` union；`commands/pane.rs:60-64` 完整 `match` 把 `Starting` 序列化为 `"starting"`。
- **gap**：`teammate/server.rs:293-298 register_agent_to_pane` 直接 `Insert(pane_id, PaneState::Busy)`，**永远不经过 Starting**。Grep 全工程 `PaneState::Starting` 唯一构造位置 = enum 定义本身（line 80）。`Starting` UI affordance 永远不会渲染。
- **判定**：半实现（half-built）。enum + 序列化 + UI 都在，唯一缺失是 register-time 改为 Starting + 收到首条 PTY 字节后 transition 到 Busy。设计意图明确（注释清楚说"agent register 已发但 PTY 还没收到首条 prompt 输出时使用"），但需用户决定何时 transition + 是否影响 Claude Code 集成的现有时序假设。
- **不实施理由**：需要用户对 teammate 流程时序的判断。本 audit 仅记录，等用户决策。

### 1.15 [HIGH] 拆分 / 关闭面板后 padding 丢失（unpark `lastAppliedPaddingPx` 缓存残留）✅ 2026-05-04

- **文件**：`src/lib/terminal/manager.ts::unpark`
- **现象**：用户报告「在终端中拆分窗口或者关闭窗口，会让部分终端窗口丢失 padding 并且无法输入」。一旦 SplitContainer 因 split→leaf 折叠或 leaf→split 包装导致 `<RgPane>` 重挂载，幸存 RidgePane 走 onDestroy → `manager.park` → onMount → `manager.unpark` 路径。新挂载的 container 是全新 DOM 节点（无 inline padding）。
- **根因**：`PaneEntry.lastAppliedPaddingPx` 记录上次 setPadding 入参，park 不清空；unpark 拿到新 container 后没有重置缓存。RidgePane onMount 调 `setPadding(paneId, settingsStore.terminalPaddingPx)` —— `entry.lastAppliedPaddingPx === clamped` 提前 return，新 container 永远不被赋 inline padding。视觉表现 = padding 丢失；canvas 因为 `width:100%; height:100%` 直接铺满 content-box，反而比期望尺寸大。
- **修法（已实施）**：unpark 在重置 `lastReportedRows / lastReportedCols = -1` 之后追加 `entry.lastAppliedPaddingPx = undefined`。下一次 setPadding 必然命中 `cached !== clamped` 分支并应用，新 container 立即拿到正确的 inline padding；setPadding 内部紧接着调 `viewportChanged` → 120 ms 后 fitPane 再读一次 canvas rect，cols/rows 按"扣掉 padding 之后"的尺寸重算并下发到 PTY + kernel grid。
- **验收**：浏览器实跑（§7.2）—— A | B 布局关闭 A，B 应保留用户 settingsStore.terminalPaddingPx，且终端字符位置随 grid 重排而 reflow（不再"卡在原坐标"）。
- **后续观察**：用户原报告同时提到「字符在原本位置刷新」。padding 丢失会让 canvas 尺寸偏大、cols×rows 比期望大，PTY emits 在更宽的 grid 上而 shell 还没收到 SIGWINCH 时就会出现错位假象。padding 修好后 ResizeObserver 重新走一次 fit 路径，理论上同步消失；若仍残留再加 §1.16 单独跟。

### 1.21 [HIGH] 终端回车触发 Explorer file tree 重建（cwd 未变时也刷新）✅ 2026-05-04

- **报告（用户 2026-05-04）**："任何触发终端回车的行为，都会触发资源管理器的 file tree 重新构建，我不希望这样，只有 cwd 变动的情况下才需要增量构建（不影响已有的、接下来不会更改的 file tree）"。
- **现象**：用户在 shell 里按 Enter（即使是空回车不切换目录）→ Explorer 右侧文件树显示 loading 闪烁 / 重建。期望：cwd 未变时 0 IPC 0 重建；cwd 变化时仅对应列增量更新，其他列不动。
- **设计已对的部分**：
  1. `paneTree.ts::setPaneCwd`（line 1376-1386）已有 identity-preserving 早返回——`store[key] === normalized` 时不更新 store，subscribers 不 fire。
  2. `Explorer.svelte:216-242` 直接订阅 paneCwdStore 已有 per-key 比对——只对真正变化的 cwd 强制 loadTree。
- **嫌疑路径**：`Explorer.svelte:75-105` 主 `$effect`：
  ```svelte
  $effect(() => {
      const cwds = $paneCwdStore;
      const titles = $terminalTitles;     // ← 也参与依赖
      const wsList = $workspacesList;
      // ...
      for (const ws of wsList) {
          fileExplorerStore.syncWithPaneCwds(ws.id, workspaceCwds, workspaceTitles);
      }
      for (const col of cols) {
          if (!col.loading && (!col.tree || col.needsRefresh)) {
              void fileExplorerStore.loadTree(col.id);
          }
      }
  });
  ```
  - **可能根因 A**：`$terminalTitles` 在每次 OSC 0/1/2 都 update，shell 通常在 prompt redraw 时 emit 标题（`"user@host: path"` 含 path 末尾的当前目录）。`fileExplorerStore.syncWithPaneCwds` 的 `mergedTitles` 路径可能在「标题字符串变化」时把 `existing.needsRefresh` 设为 true，下游 loadTree 触发。
  - **可能根因 B**：`syncWithPaneCwds` 内 line 187 `needsRefresh: existing.needsRefresh || hasNewJoiner` —— 若 `hasNewJoiner` 在 cwd 未变时被错误置 true（比如 paneIds 顺序变化），那么每次 store update 就 needsRefresh=true。
  - **可能根因 C**：shell prompt OSC 7 可能在每次 Enter 都 emit cwd（哪怕是同一目录），且 `normalizeCwd` 的输出对相同输入不稳定（不应该，但可疑）→ setPaneCwd 早返回失效 → store 真的更新 → effect 跑 → loadTree。
- **修复方案（先做证据收集）**：
  1. 在 `Explorer.svelte:75` `$effect` 加一行 `console.log('[explorer-effect] cwds keys', Object.keys(cwds), 'titles keys', Object.keys(titles))` dev-only，回放用户操作复现，看哪个依赖真的在变。
  2. 根据根因结果分别修复：
     - 根因 A：`syncWithPaneCwds` 在 title 变更时 NOT 设置 needsRefresh（title 变化不应 invalidate tree）。
     - 根因 B：`hasNewJoiner` 重新审视 paneIds 比对 stable（按 sort + 集合等价比较，而非顺序敏感）。
     - 根因 C：`setPaneCwd` 的 normalize 调用前后字符串保持同一性；如果 normalize 已经稳定，可能就是 backend 在变着 cwd（`/home/user` vs `/home/user/`）—— 加 trim 末尾斜杠到 normalizeCwd。
  3. 增量更新（用户原文「不影响已有的、接下来不会更改的 file tree」）：`loadTree` 应能 patch 局部子树而非整树重建。当前 `loadTree(columnId)` 整列重新 fetch；改为对仅 cwd 变化的列做完全 reload，已有的、cwd 未变的列 0 IPC。**这个其实已经是当前行为**——只对 `changedCwds.has(col.cwd) && !col.tree` 的列触发，但前提是 effect 不要错误地把 needsRefresh 设为 true。
- **测试**：
  - 单测：mock `paneCwdStore.set(initial)` → mock title change → assert `loadTree` not called。
  - 单测：mock cwd change for one pane → assert only that column's `loadTree` called，其他列 0 调用。
  - 实测：`pnpm tauri dev` + 多 pane 多 cwd，dev console.log 验证。
- **影响范围**：`src/lib/components/Explorer.svelte`（主 effect 重审）+ `src/lib/stores/fileExplorer.ts::syncWithPaneCwds`（needsRefresh 触发条件收紧）+ 可能 `src/lib/stores/paneTree.ts::normalizeCwd`（trailing slash）。
- **优先级**：HIGH——日常使用频繁触发，影响交互流畅度。
- **修复（2026-05-04）**：根因 = `RidgePane.svelte::onKernelEvent` 的 `TitleChanged` / `IconNameChanged` 分支在 OSC 0/1/2 上**无条件**调 `terminalTitles.update((m) => ({ ...m, [paneId]: ev.value }))`——shell 在每次 prompt redraw 都 emit OSC 标题，store 每次都拿到「值相同 ref 不同」的新对象 → subscribers 全 fire → Explorer.svelte:75 `$effect` 重跑（依赖 `$terminalTitles`）→ `syncWithPaneCwds` 重建 columns 数组 → FileTree 重新评估 props。
  - **方案**：在 update callback 内加 identity-preserving early return，与 `paneTree.ts::setPaneCwd` line 1383 同模式：`m[paneId] === ev.value ? m : ({ ...m, [paneId]: ev.value })`。
  - **影响范围**：仅 `src/lib/components/RidgePane.svelte` 4 行（含注释）。`paneOscTitleStore` 和 `terminalTitles` 都加 guard。
  - **不做的**（避免 over-engineering）：syncWithPaneCwds 内部加 deep-equality 早返回——主因已在源头消除，其它合法触发路径（cwd change / workspace add/remove / 真正的 title 变化）应走完整 sync。
  - **验证**：`pnpm check` 0 errors / 0 warnings (4098 files)。**待用户实跑确认 Enter-without-cd 不再触发 loading 闪烁**。
- **追加（2026-05-05）**：用户实跑后报告 "ctrl c 或者 enter 仍会闪烁"——RidgePane 内的 identity guard 没覆盖到所有路径，因为 shell 在 PROMPT_COMMAND 中通常会 emit「user@host: cwd command」之类带正在执行命令名的 title，命令前后两次 emit 的字符串 *不同*，identity guard 不命中 → terminalTitles 真的变化 → Explorer.svelte:75 `$effect` 重跑（依赖 `$terminalTitles` + `$paneCwdStore`）→ `syncWithPaneCwds` 即使保留 cwd 部分不变，也会无条件 `update((state) => ({...state, columns: nextColumns}))` 重建 columns 数组引用 → FileTree 重评估 props。
  - **真根因**：title sync 与 cwd sync 共享同一个 effect。title 变化是 prompt-redraw 的常态噪声，不应触发 column 数组重建（columns 只关心 cwd → paneIds 映射）。
  - **追加修复**：拆分 effect。
    1. `src/lib/stores/fileExplorer.ts` 新增 `updatePaneTitles(workspaceId, paneTitles)` 方法——identity-preserving，遍历 ws 列、若该列 paneTitles 与 incoming 相同则保留原 col 引用、若整次扫描没有任何列变化则返回原 state 引用。
    2. `src/lib/components/Explorer.svelte` 主 `$effect` 删除 `$terminalTitles` 依赖、不再传 titles 给 `syncWithPaneCwds`；新增第二个独立 `$effect` 只依赖 `$terminalTitles + $workspacesList`，对每个 ws 调 `updatePaneTitles`。这样 title-only 变化只会让需要的 column 引用更新，cwd-sync 路径完全不动。
  - **验证**：`pnpm check` 0 errors / 0 warnings (4098 files)；`cargo check --target wasm32-unknown-unknown --lib`（默认 + `--no-default-features`）双 0 警告；`cargo test --lib` 240 通过。**待用户实跑再次确认 Ctrl+C / Enter 不再 loading 闪烁**。

### 1.20 [HIGH] 选区在滚动后不跟随文本滚动（viewport-relative coords 缺陷）✅ 2026-05-04

- **报告（用户 2026-05-04）**："选中文案的选中域，在滚动后不会随着选中的文本滚动，需要修复"。
- **根因**：`packages/ridge-term/src/selection.rs` 的 `Pos { row, col }` 是 **viewport-relative** 坐标（见模块文档 line 16-19）。用户滚动 viewport（PageUp / 滚轮 / Shift+PageUp 跨入 scrollback）→ scroll_offset 改变 → 同一 viewport row 现在指向不同的 abs_row → 选中高亮仍画在原 viewport 位置，但实际选中的文本已经滚到别处。
- **类比**：`packages/ridge-term/src/search.rs` 早就解决了同一问题——`MatchAbs { abs_row, col_start, col_end }` 用「scrollback 0..sb_len，viewport sb_len..sb_len+rows」的统一 abs-row 编码；`match_to_viewport_range(scroll_offset, sb_len, rows) -> Option<Range>` 每帧 translate 回 viewport，超出视口的 match 自然 clip 掉。Selection 应该照搬这个模式。
- **修复方案**：
  1. **Selection 模型改 abs-row**：`Pos` / `Range` 不变（renderer 和 text() 还是要用 viewport coords），但 `Selection` 内部存 `Option<RangeAbs>`，新加结构 `RangeAbs { start_abs_row, start_col, end_abs_row, end_col }`。`set` / `clear` / `select_all` / `select_word` / `select_line` / mouse-drag 入口都改写到 abs-row（用 `terminal.viewport_to_abs(row)` helper）。
  2. **新加 `selection_to_viewport_rects(scroll_offset, sb_len, rows) -> Vec<(usize, usize, usize)>`**：每帧 renderer 调，翻 abs-row 选区为「(viewport_row, col_start, col_end)」rects 列表，clip 到 0..rows 的可见范围。Selection 完全在 viewport 之外 → 空 vec（无 overlay 绘制）。
  3. **`text()` 改读 abs-row**：跨 scrollback 的选区拷文本，使用 `terminal.row_at_abs(abs_row)` helper（如果不存在就加一个：`< sb_len` 走 scrollback ring，`>= sb_len` 走 grid）。
  4. **lib.rs `setSelection(startRow, startCol, endRow, endCol)`**：JS 仍传 viewport 坐标（manager.ts pointerdown 算的是 viewport row）；wasm 入口接收时立即转 abs-row 后存。
  5. **renderer.rs**：当前 `selection_to_rects` 调用换成新的 `selection_to_viewport_rects`。
- **测试**：
  - 单测：select 一段 viewport 内容；feed `\x1b[5S` 滚 5 行进 scrollback；assert `selection_to_viewport_rects` 返回的 rects 行号下移 5。
  - 单测：select 跨 viewport+scrollback 的范围；scroll 后 viewport 内可见部分 rects 正确，scrollback 内的部分 clipped。
  - 单测：select 完全滚出 viewport → 空 vec。
- **影响范围**：`packages/ridge-term/src/selection.rs`（核心模型）+ `packages/ridge-term/src/lib.rs`（setSelection 入口转换）+ `packages/ridge-term/src/render/renderer.rs`（rect 计算入口）+ `packages/ridge-term/src/term/terminal.rs`（可能要加 abs-row helper）。**4 个 source file，估 200-300 行**。
- **风险**：手动 testing 时验证「scroll 进入 scrollback 后 selection 还在 / scroll 出来再回去 selection 还在 / 跨 scrollback 边界的选区滚动一致」。
- **优先级**：HIGH——日常使用频繁触发的视觉错误。
- **修复（2026-05-04）**：照搬 `search.rs::MatchAbs` 的 abs-row 模式。
  - **新加 `Terminal::row_at_abs(abs_row) -> Option<&Row>`**（`packages/ridge-term/src/term/terminal.rs`）：统一 `0..sb_len` → 滚动历史 + `sb_len..sb_len+rows` → 活动 grid 的 row 访问器。
  - **`Selection` 全文重写为 abs-row 内部存储**（`packages/ridge-term/src/selection.rs`）：新加 `RangeAbs { start_abs_row, start_col, end_abs_row, end_col }`；`set / select_word / select_line` 全部从 viewport coords 翻译到 abs（用 `vp_to_abs(vp_row, scroll_offset, sb_len) = sb_len + vp_row - scroll_offset` 公式，与 `search.rs::match_to_viewport_range` 互逆）。`text()` 改读 `terminal.row_at_abs(abs)`。
  - **新加 `Selection::range_in_viewport(&Terminal) -> Option<Range>`**：每帧 renderer 调，把 abs 翻译回 viewport-relative + clip 到可见范围（部分超出时 col_start=0 或 col_end=cols）；完全在视口外返回 None。
  - **lib.rs**：`set_selection` / `apply_active_match` 都传 `&self.inner`；`RenderHandle::render` 调 `range_in_viewport(&kernel.inner)` 取代 `range()`。
  - **新增 3 单测**：`selection_survives_scroll_into_scrollback`（abs invariant under scroll）、`range_in_viewport_translates_with_scroll`（bottom row clips when scrolled below viewport）、`empty_terminal_select_all_is_safe`。
  - **影响范围**：`packages/ridge-term/src/term/terminal.rs`（+10 lines）、`packages/ridge-term/src/selection.rs`（全文重写，~440 lines）、`packages/ridge-term/src/lib.rs`（3 surgical edits）。
  - **验证**：`cargo test --lib` **240 passed**（之前 237，+3 新测）；`cargo check --target wasm32-unknown-unknown` 0 warnings（default + webgpu 双模式）；`pnpm check` 0 errors / 0 warnings。**待用户实跑：选中文本 → PageUp/PageDown → 高亮跟随；scroll 出 viewport → 高亮自动消失；scroll 回来 → 高亮再次显示**。

### 1.22 [HIGH] Resize 时持续刷新型 CLI（Claude Code 类）出现错位行/字符 ✅ 2026-05-05

- **报告（用户 2026-05-05）**："当使用 claude code 这类连续刷新输出字符区的终端 cli，如果进行终端大小形状的 resize，以当前实现，会出现错位行和字符。如果是系统终端的默认行为，会整体刷新输出区域，以让展示正常，但当前实现不支持这种全部刷新的行为，我需要适配这种持续性终端程序，要在其 resize 之后，完全刷新输出区域。"
- **现象**：Claude Code / Ink-based CLI / lazygit 等使用 alternate-screen + 局部重绘的程序，在终端 resize 时会被 SIGWINCH 触发自身 redraw，但 Ridge 的 wasm grid 的 OLD content（resize 前的字符）残留在新尺寸下错位显示，与新 redraw 的 content 叠加 / 错行。
- **可能根因**：
  1. Phase 1 reflow 只处理 main screen，alt screen（CC 通常用 alt screen）走 truncate/pad 路径，行被裁切但内容仍按旧 grid 位置画。
  2. wasm kernel 的 dirty-row 在 resize 后被全部 invalidate（renderer.rs 已经在 grow snapshot 时设 full_redraw_pending=true），但 grid 内容没被清掉，所以新 frame 把旧字符按新坐标画——和 SIGWINCH 后 CLI 自己重绘的内容重叠。
  3. ConPTY resize-silence 窗口（pty.rs:95 `RESIZE_SILENCE_WINDOW_MS = 800`）期间 PTY bytes 被 drop，resize 完成后 CLI 的 redraw 才会进入 kernel——但旧字符仍在屏。
- **方案候选**：
  - **A. resize 时清空 alt-screen 的 grid + main screen 的 viewport 行**：在 `Terminal::resize` 检测到尺寸变化时，主屏幕走 reflow（保留），alt 屏幕直接 `clear()` viewport（清空字符 + 重置 cursor）让 CLI 的 redraw 落到干净画布。同时通知 backend 别做 resize-silence 抑制（让 redraw 立刻可见）。
  - **B. 给 RenderHandle 加 `forceFullRefresh()` 接口**：JS 在 resize observer 触发时主动调一次清屏 + 等待下一个 PTY frame。
  - **C. 把 resize-silence 窗口缩小到 ~150ms**（vs 当前 800ms），让 CLI redraw 更早进入 kernel，旧字符存活时间缩短。
  - 当前倾向 **A**（最贴合用户描述的"完全刷新输出区域"语义），但要小心 main screen 不能粗暴清——否则用户在 shell 里 resize 会丢掉 prompt。
- **影响范围**：`packages/ridge-term/src/term/grid.rs::resize` + `packages/ridge-term/src/term/terminal.rs::resize` + 可能 `src-tauri/src/engine/pty.rs` 调整 silence 窗口。
- **测试需要**：在 alt-screen 情况下 resize 之前 / 之后的 grid snapshot 对比；浏览器实跑 Claude Code + lazygit + btop 验证 resize 流畅。
- **优先级**：HIGH——CC 用户高频遇到。

### 1.22.b [HIGH] Resize-silence 缩到 250ms — cursor 位置卡在 resize 前的位置 ✅ 2026-05-05

- **报告（用户 2026-05-05）**："resize 之后, 插入光标位置还是计算不正确, 还是记录之前的位置"
- **根因**：`src-tauri/src/engine/pty.rs::RESIZE_SILENCE_WINDOW_MS = 800ms` 在没有 shell-integration（FinalTerm `OSC 133;A` 或 VS Code `OSC 633;A` prompt 标记）的 shell / CLI 上把 SIGWINCH 触发的 redraw 整段丢掉。Shell（bash 无 PROMPT_COMMAND 改造、cmd、Git Bash）和 CLI（Claude Code、Ink-based、lazygit）都在此列。Resize 流程：
  1. JS 端 `fitPane` 算新 rows/cols，调 `entry.resizeHandler` → `invoke('resize_pane')` 触发 SIGWINCH
  2. Backend 设置 `silence_deadline = now + 800ms`
  3. PTY reader 从 child 读出 redraw 字节
  4. 800ms 内 + 没遇到 prompt OSC → 字节全 drop
  5. Kernel grid 保持 reflow 之后但 NOT 重画的旧内容
  6. 用户视觉：光标在 reflow 后的「migrated 旧位置」，看上去就是「resize 前的位置」
- **修复**：将 `RESIZE_SILENCE_WINDOW_MS` 800 → 250。仍覆盖 ConPTY replay 50-300ms 区间下沿；shell 重画字节几乎都能在 250ms 后落入 kernel；启用 shell-integration 的环境仍会被 prompt OSC 提前截断（early-release < 250ms）。
- **影响范围**：`src-tauri/src/engine/pty.rs` 一个 const。需要重启 Tauri 后端生效（`pnpm tauri dev` 重新启动）。
- **测试**：仍是 `cargo build --lib` 0 警告；浏览器实跑验证 resize Claude Code / lazygit / bash-without-OSC133 后光标在新位置。

### 1.23 [HIGH] 缺失的 UI features（重构 xterm.js 时丢失）✅ 2026-05-05（侧边滚动条 + 滚动到底部按钮 + 拆分右键菜单 全部 shipped）

### 1.24 [HIGH] 连续字符画（box-drawing）行间隔无法消除 — 需 primitive 渲染 ⏳

- **报告（用户 2026-05-05）**："连续字符画渲染不成功"——多个 `─` `│` `┌` `┐` `└` `┘` 等字符拼成的「方框 / 树形 / 表格」边界在垂直相邻 cell 之间出现 1-2 px 缺口，无法连贯成线。
- **根因**：font 的 box-drawing glyph（U+2500-257F）通常只占 EM box 高度 = `font_size_px * dpr`（30 device px），但 cell 高度 = `(ascent + descent) * dpr`（≈ 36 device px，line-height ~1.2× font size）。glyph 在 cell 中垂直居中，cell 顶部 + 底部各留 ~3 device px 空白。垂直相邻 cell 的 `│` glyph 边缘相隔 6 device px → 视觉上断开。我此前的修复（Nearest sampler / 整数对齐 / UV crop / alphabetic baseline / atlas slot 64×96）解决了「字符渲染本身的精度」，但没有解决「字体 glyph 不延伸到 cell 边缘」这一固有问题。
- **方案候选**：
  - **A. Box-drawing primitive 渲染**：检测 cell.ch ∈ U+2500-257F 时不查 atlas、直接 emit 一组填充矩形作为 CellInstance。每个字符 1-3 个矩形（横线 / 竖线 / 角点）。覆盖最常用集（约 22 字符：`─│┌┐└┘├┤┬┴┼═║╔╗╚╝╠╣╦╩╬`）。**用户体验最佳**，但实现量较大（每个字符需要单独的几何）。
  - **B. 把 box-drawing glyph 在垂直方向上拉伸填满 cell**：UV crop 改成只 crop 水平（保留 glyph 形状），垂直采样整个 cell_h；rasterizer render 时也把 fill_text 的 Y 区间扩到整个 ascent+descent。简单，但其他字符也会被拉伸（影响 ascender/descender 比例）→ 副作用大。**不推荐**。
  - **C. 换字体到 box-drawing 设计 cover full cell-line-height 的字体**（Iosevka、SF Mono、Cascadia Code 等）。**不需要代码改动**，但用户字体选择不可控。
  - **D. 减小 line-height factor 让 cell ≈ font_size**：`measure_font` 返回 `font_size_px * 1.0`（不是 `1.2`），cell 缩到 EM box 高度。会裁掉 'g' / 'p' / 'y' 的 descender。**不推荐**。
- **当前推荐**：**A**，但分阶段：先实现核心 16 字符（single-line 全套 + double-line 4 角），覆盖 80% 实际用例；剩余的留给字体或 follow-up。
- **影响范围**：`packages/ridge-term/src/render/webgpu.rs::draw_row` + `packages/ridge-term/src/render/canvas2d.rs::draw_row`（Canvas2D 也需要同步实现，否则 fallback 路径仍有 gap）；可能新增 `packages/ridge-term/src/render/box_drawing.rs` 模块封装 char → primitive geometry 映射。
- **测试**：手工用 ASCII 截图对比；可加单测 verify primitive geometry for each codepoint。
- **优先级**：HIGH — Claude Code / lazygit / btop / unicode tree CLI 都密集使用 box-drawing。

- **报告（用户 2026-05-05）**："当前终端丢失了重构终端（移除 xterm.js）之前的滚动到最底部按钮 / 终端侧边滚动条 / 可以进行拆分终端的右键菜单（可以通过历史记录确认具体的菜单选项）"
- **缺失项**：
  1. **滚动到最底部按钮**——用户翻历史后，需要快速跳回到 live grid 底部。当前要靠按 End / Esc / 输入字符触发 kernel 自动 scroll-to-bottom。需要一个浮动按钮（仅在 `scroll_offset > 0` 时显示）。
  2. **终端侧边滚动条**——视觉指示当前在 scrollback 的位置 + 可拖拽。当前完全无视觉反馈，用户只能盲滚。
  3. **拆分终端的右键菜单选项**——右键现在只显示「复制 / 粘贴 / 全选 / 清空」（见 `RidgePane.svelte::onContextMenu`），缺失了 xterm 时代的「水平拆分 / 垂直拆分 / 关闭 pane / Bot agent / 标签重命名」等。需要 git 历史确认完整列表。
- **执行步骤**：
  1. `git log --all --diff-filter=D -- src/lib/components/Pane.svelte` + `git show <commit>:src/lib/components/Pane.svelte` 找回原 contextMenu 项目。
  2. `RidgePane.svelte::onContextMenu` 加回缺失项（split horizontal/vertical 调 `paneTreeStore` API，close pane 调 `closePane(paneId)`，rename 调 alertDialog/promptDialog）。
  3. 滚动到最底部按钮：浮在 pane 右下，CSS `position: absolute; bottom: 12px; right: 12px`，仅 `manager.scrollState(paneId).offset > 0` 时显示。点击调 `kernel.scrollToBottom()`。
  4. 滚动条：原 xterm 用 `scrollbarColor` CSS（不可拖）。我们的 wasm kernel 没有 native 滚动条；要么用 transparent overlay div 模拟，要么实现 wgpu / canvas2d 直绘（更复杂）。MVP 用 overlay div：高度 = pane_h × (rows / total)，top = pane_h × (offset / total)，draggable 调 scroll API。
- **影响范围**：`src/lib/components/RidgePane.svelte`（contextMenu 扩展 + 浮动 button + scrollbar overlay）+ 可能 `src/lib/stores/paneTree.ts`（确认 split / closePane API 暴露）。
- **优先级**：HIGH——日常使用必备 affordance。

### 1.19 [META] 全部 §1.x / §2-§7 完成后做架构 review + 计划 refresh ⏳

- **触发条件**：所有 §1.x / §2-§7 中标记 ✅ / 已实施 / 决定不做 的项加在一起 = 100% 完成。剩余 ⏳ 项要么用户已显式标 not-needed，要么已经合并入新 plan。
- **范围（用户原文）**：「如果最后完成所有可执行计划，重新通过文档 `C:\code\wind\docs\term-rebuild\OVERVIEW.md` 进行架构设计实现复查，进行 review code，确定一些未实现计划是否要继续进行，以及是否需要补充一些新任务以达到最初进行重构的目标。如果有必要，可以进行一轮新的总的架构设计规划，以达到预期性能和使用体验。」
- **执行步骤（建议）**：
  1. **Cross-check OVERVIEW.md vs 实际代码**：逐节读 OVERVIEW，grep / 阅读对应实现文件，标记 (a) 已实现一致、(b) 已实现但偏离原设计（记录偏离原因 + 是否需修正）、(c) 未实现（评估必要性）。
  2. **Code review 全 ridge-term 包**：从 `packages/ridge-term/src/{lib.rs, term/, render/, selection.rs, search.rs, input.rs}` 入手，看是否有：(i) 历史遗留死代码、(ii) 命名 / API 不一致、(iii) 测试覆盖盲区、(iv) 性能 hotspot 没量化（特别是 §4.4 perf bench 一直 ⏳）。
  3. **Round 3 决策点**：评估 wgpu 接线投入产出。当前 Canvas2D 路径用户没报告显著卡顿（OVERVIEW §6 R2 已记录），WebGPU 路径仍能拉低 GPU 内存（10 pane 时从 10 ctx 压到 1）。决定继续 / 推迟 / cancel。
  4. **Round 4 / 5 决策**：reflow Phase 2 (§2.3 远期)、grapheme cluster (§2.4 远期)、Bell audio (§3.3 远期) — 是否补做或正式 cancel。
  5. **新 plan**（若需要）：基于现状产出下一阶段方向，如 (a) 性能基准 + 优化、(b) UX 抛光（multi-pane focus / IME v3 / accessibility）、(c) 跨 OS 验证（macOS / Linux 上的 webview WebGPU 支持）。
- **不在本条范围**：本条本身不要写代码，只产出 review 报告 + 决策记录。代码工作由后续具体 task 承接。
- **触发时机**：等用户明确说「所有 ⏳ 都搞定了，开始 review」时启动；本 task 仅作占位 + 检查清单。
- **关联**：复查 dimension 包括但不限于 `OVERVIEW §3 进度表`、`PROBLEM_COVERAGE.md`、`PARTIAL_REDRAW_PROTOCOL.md`、`BUGFIX.md`、`INTEGRATION.md` / `INTEGRATION_R2_4.md` / `REPLACE_AND_FIX_PLAN.md` 全套 8 个 term-rebuild 文档。

### 1.16 [HIGH] 终端 Ctrl+C 不应触发 SCM / 文件树重载 ✅ 2026-05-04

- **文件**：`src-tauri/src/commands/watch.rs::GitWatcher`
- **现象**：用户报告「shortcut key : ctrl + C should not let scource manager file tree reload, it is not go to a new cwd」。§1.11 已给 `setPaneCwd` 加 identity-preserving early return；本条是后续观察发现的剩余路径。
- **根因**：`GitWatcher` 直接 watch `<repo>/.git/` 递归，把任意路径变化都 emit 为 `scm-repo-changed`。shell prompt hook（starship / oh-my-posh / powerlevel10k）每次绘制提示符都会跑 `git status / rev-parse`，根据 git 版本和 `core.fsmonitor` 设置可能往 `.git/objects/`、`.git/logs/`、`.git/index.lock`（瞬时）等位置写。这些路径都属于 git 内部存储 churn，**不影响 porcelain 输出**，但都被 GitWatcher 当成"仓库变了"上报。Ctrl+C 触发 prompt redraw → prompt hook git probe → `.git/objects/` write → `scm-repo-changed` → SourceControl.svelte 250 ms debounce 后跑 `refreshStatus + loadGraph`。
- **修法（已实施）**：在 GitWatcher 的 debouncer 回调里加 `is_scm_relevant(path)` 过滤——一个 debounce 窗口内的所有事件路径若全部是噪声（`/objects/`、`/logs/`、`/info/`、`*.lock`）则不 emit；只要至少一个事件路径属于 HEAD / index / refs / packed-refs / FETCH_HEAD / 操作状态文件就照常 emit。
- **保留信号**：分支切换、commit、fetch、merge/rebase 进度、index 更新都仍然触发刷新——它们写的是 `.git/HEAD`、`.git/refs/`、`.git/index`、`.git/MERGE_HEAD` 等非噪声路径。
- **测试**：`cargo check --lib` 0 错误 0 警告。浏览器实跑验证留给 §7.2。
- **关联**：fs_watch.rs 已经 SEGMENT_BLACKLIST 过滤 `.git/`、`node_modules/`、`target/` 等，不会经 fs-changed 路径误触发。

### 1.18 [HIGH] 终端运行 Claude Code 出现非预期下划线 + 字符刷新错位 / 残留 ⏳ 主要 ✅（下划线 ✅ + OSC 8 残留 ✅ / 错位 待浏览器实跑确认）

- **背景**：用户报告「终端中运行 claude code，所有输出出现非预期的下划线，字符刷新区也出现一定的错位和多余、残留字符显示，深入而全面的调研修复方法，调查当前行业最佳实现是怎么做的」。
- **症状拆解**：
  1. **非预期下划线**：Claude Code 的所有输出（普通文本，非 OSC 8 hyperlink）显示下划线。说明 SGR underline 状态被错误 set 后没有 reset，或 Ridge 把某种 SGR 子参数误判为 underline。
  2. **字符刷新错位**：partial redraw 区域字符位置偏移，疑似 cursor positioning（CSI H / VPA / HPA）和我们的 grid 状态不一致。
  3. **多余 / 残留字符**：旧字符在 cell 上没被新字符覆盖。说明 cell bg 没在 partial redraw 时清掉（背景不重新绘制就直接 fillText 新字形 → 旧字形像素仍在）。
- **疑点 trace**：
  1. **下划线源**：
     - `term/parser.rs::handle_sgr`（SGR 解析）—— 检查 `4` 是否被正确识别为 underline-on，`24` 为 underline-off，`0` 为 reset-all（包括 underline）。如果只 reset 了部分 attr 没清 underline，状态污染。
     - **扩展下划线（CSI 4:N）**：xterm + iTerm2 + kitty 支持 `CSI 4:0m`（off）、`CSI 4:1m`（single）、`CSI 4:2m`（double）、`CSI 4:3m`（curly，VTE 扩展）、`CSI 4:4m`（dotted）、`CSI 4:5m`（dashed）。如果 Claude Code 用了 `CSI 4:0m` 关闭下划线，但我们 sub-parameter parser 把它当成 `CSI 4m`（默认 single underline on），就解释为何「所有输出都有下划线」。
     - **OSC 8 hyperlink underline 残留**：Claude Code 内部可能用 OSC 8 包装路径，OSC 8 在 close 时（`OSC 8 ; ; ST`）应清掉 hyperlink span。检查 `term/parser.rs` OSC 8 close path 是否正确终止 span，否则下划线一直延续到行尾或后续行。
  2. **字符错位**：
     - `term/parser.rs::CSI_H` (cursor position absolute)：1-based row;col。Claude Code 频繁用绝对定位刷新 status line，如果我们 0-based / 1-based 转换有误，每次重绘都会偏移。
     - **`CSI ?25l/h` (DECTCEM)**：cursor 隐藏期间的位置移动，部分仿真器累积偏移；检查 `modes.rs::cursor_visible` 是否正确通过。
     - **DECSTBM scroll region**：Claude Code 可能设 scroll top/bottom 排除 status line，scroll 时只滚区域内行；我们的实现是否正确？
     - **wcwidth + reflow**：Claude Code 输出包含 emoji（box-drawing、▶、●、✔），如果 wcwidth 把 width=2 的字符算成 1，后续 cell 会偏移半格。
  3. **残留字符**：
     - `render/canvas2d.rs::draw_row` 当前**总是先填 bg 再画 glyph**（line 183-189 的 pass 1 + line 195+ 的 pass 2，注释明确说"Conservative: always paint, accept the perf hit"）。理论上 partial redraw 也会清 bg。但如果 dirty_rows 漏算，整行就不进 draw_row，旧像素全留。
     - `render/renderer.rs::tick` 用 per-row 64-bit hash diff 判定 dirty，hash 包含 `(ch, attr_id, width)`。如果 attr_id 包含动态 hyperlink span 但内容相同，hash 不变 → row 不 dirty → 不重画 → 残留前帧。
     - **alt screen ↔ primary screen 切换**（CSI ?1049h/l）：Claude Code 可能进 alt screen 显示菜单，再退回 primary。切换时 grid 状态切了但 renderer snapshot 没强 invalidate？检查 `lib.rs::JsTerminal::resize` / 切屏路径是否调 `invalidateAll`。
- **行业最佳实现参考**（待研究）：
  - **xterm（C, 参考实现）**：`charproc.c::doparsing` 处理 SGR；`button.c` 处理 selection；`screen.c::ScreenWrite` 是 cell 写入路径。
  - **iTerm2（Obj-C, macOS）**：`VT100Terminal.m::executeSGR`；公认的 SGR sub-parameter 实现最完整。
  - **kitty（Python+C, 高性能）**：`kitty/parser.c`，扩展下划线最早的实现者之一；wezterm 也参考其实现。
  - **alacritty（Rust, GPU）**：`alacritty_terminal/src/ansi.rs::Processor`；Ridge 的 vte crate 解析层与其同源。
  - **wezterm（Rust, GPU）**：`wezterm-escape-parser/src/csi.rs`，文档化最全的 SGR / OSC 8 实现。
- **下一步**：
  1. 启动 Ridge `pnpm tauri dev`，在终端跑 claude code，DevTools network → frontend 用 `manager.feed` 的 paneId 上一个 PTY 字节 dumper（dev-only），把 raw bytes 与渲染结果对照，定位是 SGR 解析错还是渲染错。
  2. 写一个 fixture 用 raw byte 序列（比如 `\x1b[4m...\x1b[24m`、`\x1b[4:3m`、`CSI ?1049h`/`CSI ?1049l`）单测，跑通 vte → grid → render 全链路。
  3. 对照 wezterm + iTerm2 的 SGR sub-parameter 实现差异，对齐我们的 parser。
  4. 如果是 partial-redraw 残留 → 在 `tick()` 加 alt-screen-switched / mode-switched / scrollback-changed 强制 `invalidate_all` guards。
- **判定**：本条不能 1-loop 修完，先建 task 收集线索；下一两个 loop 迭代深入实现。
- **关联**：与 §1.15 padding / §1.17 input-loss 是不同 root cause（这俩已修），但症状叠在一起会让用户感到「split + claude code 后整个终端都坏了」。

#### 1.18.a [HIGH] SGR 扩展下划线 (CSI 4:N m) 子参数解析 ✅ 2026-05-04

- **文件**：`packages/ridge-term/src/term/parser.rs::apply_sgr` + `packages/ridge-term/src/term/terminal.rs::tests`
- **根因（命中）**：`apply_sgr` 用 `match sub.first().copied()` 判断 SGR code，然后 `4 => attrs.flags.insert(Flags::UNDERLINE)` —— **完全忽略 sub-parameter**。VTE crate 把 `CSI 4:0 m` 解析为 `&[4, 0]` 单元素 sub-array：`sub.first() == Some(4)` 命中下划线-ON 分支，**`CSI 4:0 m`（关闭下划线）被解释为 `CSI 4 m`（开启下划线）**。Claude Code 用 `CSI 4:0 m` 在 OSC 8 hyperlink 关闭后释放下划线状态——结果状态卡死，所有后续输出被下划线污染。
- **修法（已实施）**：`4` 分支改为读 `sub.get(1).copied().unwrap_or(1)`：
  - `0` → 关闭 UNDERLINE + DBL_UNDERLINE。
  - `2` → 开启 DBL_UNDERLINE，关 UNDERLINE。
  - `1 / 3 / 4 / 5 / 其他` → 开启 UNDERLINE，关 DBL_UNDERLINE（curly/dotted/dashed 暂时降级为 single underline，等 renderer 支持后再分流）。
  - 不带 sub（裸 `CSI 4 m`） → 默认 1，单下划线 ON，与之前行为一致。
- **测试**：5 条新 unit test 覆盖 `CSI 4 m` baseline、`CSI 4:0 m` 关闭、`CSI 4:2 m` double、`CSI 4:3 m` curly degrade、`CSI 24 m` no-op 后的状态一致性。`cargo test --lib` **125 passed**（原 120 + 5）。
- **行业对照**：xterm 的 `parsing.c::doSGR` (line 5400+) 同样按 sub[1] 路由；wezterm 的 `wezterm-escape-parser/src/csi.rs` 中 `Sgr::Underline` 直接用 enum；alacritty 的 `vt100.rs::Csi::SubParam` 解析时如果遇到 `4:0` 显式置 `Underline::None`。Ridge 现在与这三家行业实现对齐。

#### 1.18.b [HIGH] Claude Code 字符刷新残留 — OSC 8 hyperlink span 不随 erase 一起 clip ✅ 2026-05-04

- **文件**：`packages/ridge-term/src/term/grid.rs::erase_row_range` + `erase_chars` + 新增 `clip_hyperlinks_around` helper；`packages/ridge-term/src/term/terminal.rs::tests`
- **根因**：`Row::clear()`（line 103）正确清 `hyperlinks`，但 partial-erase 路径 `erase_row_range`（line 684）和 `erase_chars`（line 706, ECH）只覆写 cells 不动 spans。一旦 Claude Code 用 `CSI K`（erase line）/ `CSI J`（partial erase display）/ `CSI X`（ECH）做 status redraw —— 它频繁这么做 —— 旧 hyperlink span 残留在 row.hyperlinks 中。renderer 的 hyperlink-underline pass 每帧从 `row.hyperlinks` 重建 hl_rects，把下划线画在已清空的 cell 上，视觉表现 = 「字符刷新区出现一定的错位和多余、残留字符显示」。
- **修法（已实施）**：新增 `clip_hyperlinks_around(spans, start, end)` 私有 helper：
  - span 完全在 `[start, end)` 外 → 保留
  - span 完全在 `[start, end)` 内 → drop
  - erase 抹掉 span 尾部 → `col_end = start`
  - erase 抹掉 span 头部 → `col_start = end`
  - erase 在 span 中间打洞（`span.col_start < start && span.col_end > end`） → drop（不能在 retain_mut 中分裂为两个 span，xterm 同样选择 drop）
  - `erase_row_range` 和 `erase_chars` 都调用此 helper。
- **追加扩展（同提交）**：`insert_chars`（ICH, CSI @）和 `delete_chars`（DCH, CSI P）也走 line-edit 路径，PSReadLine / readline / Claude Code prompt 编辑频繁触发。两者用 `r.hyperlinks.retain(|span| span.col_end <= cur_col)`：cursor 之前完整存在的 span 保留，跨 cursor 或之后的 span 全部 drop。理由：edit 后可见 label 已经偏移，原 click target 不再对应；xterm 同样的「edit invalidates the link」语义。
- **测试**：9 条新 unit test：CSI 2K 清整行、ECH 抹尾部 / 中间 / 头部 / 全覆盖 drop / 两 span 之间保留、ICH cursor 处 drop 重叠 span / 远端 span 保留、DCH cursor 处 drop 重叠 span。125 → 134 passed。
- **行业对照**：xterm 的 `screen.c::ClearInLine` 走 `RegionClear` 同步清 hyperlink registry；wezterm 的 `wezterm-term/src/terminalstate/mod.rs::erase_in_line` 通过 `screen_mut().erase_at(...)` 内部连同 hyperlink 一起 reset；alacritty 的 `term/mod.rs::clear_line` 把每个 cell 的 `hyperlink: Option<Hyperlink>` 字段一起置 None（其设计每 cell 独立持有 hyperlink，无单独 span 列表）。Ridge 现采用 span-coalesced 数据结构 + erase-time 显式 clip，行为与 xterm/wezterm 对齐。
- **未做**：完整 fuzz / Claude Code 实跑回归 → §7.2。

#### 1.18.c [MEDIUM] Claude Code 字符刷新错位 ⏳ 防御已部署 / 浏览器实跑确认

- **背景**：1.18.a + 1.18.b（含 ICH/DCH 扩展） 修好之后，剩下的「错位」症状可能：(i) 是 SGR 下划线 + OSC 8 残留叠加导致的视觉错觉；(ii) 真有独立的 cursor positioning bug；(iii) shell SIGWINCH 处理的固有 race（不是 kernel bug）。
- **审计 (2026-05-04)**：
  1. **`kernel.resize` 调用链**：唯一生产调用点 = `src/lib/terminal/manager.ts::fitPane:974`。该函数**必先调** `entry.handle.resize(wCss, hCss, dpr)`，后者内部 `renderer.invalidate_all()`。故 grid 尺寸变化 ⇒ renderer 必然被强制 full-redraw。✅
  2. **alt-screen 切换 (`?1049h/l`, `?47h/l`)**：renderer 用 per-row 64-bit hash 比对 last snapshot；alt vs primary 内容 hash 不一致 ⇒ 全行 dirty ⇒ 全屏重绘。无需额外 invalidate 信号，自然机制覆盖。clear-on-enter (1049) 和 preserve-on-enter (47) 两种模式都正确。✅
  3. **CSI H / VPA / HPA decoding**：parser.rs:186-200 / 167-185 / 173-185 全部正确做 1-based-on-wire ↔ 0-based-internal `saturating_sub(1)`，`cursor_to` 也清 `pending_wrap`。验证通过。✅
- **追加防御（已实施）**：`renderer.rs::tick` 的 per-row hash 现在并入 `row.hyperlinks` 的形状（`(spans.len(), col_start, col_end)`）。当前所有 cell-mutating Grid 方法（clear / erase_in_line / erase_chars / insert_chars / delete_chars / Row::resize）都已经维护 spans 同步，但 hash 加上 hyperlinks 形状是 cheap defense-in-depth：未来若新加 span-only 突变路径，dirty calc 仍能感知。URI/id 不入 hash（underline overlay 只随空间位置变，相同 col 范围必产生相同像素）。
- **测试**：134 tests 全绿（renderer 单测覆盖弱因为后端是 wasm-only；hash 逻辑改动是 5 行附加，由 trait surface + 现有 cell-mutation 测试间接覆盖）。
- **剩余可能**：浏览器实跑后若仍报告错位，疑点剩下：(i) 用户的 shell 在 SIGWINCH 处理上有延迟 / race（不是 Ridge bug）、(ii) wcwidth 数据表对某些 emoji / box-drawing 字符判错（需要具体复现 case）。

### 1.17 [HIGH] 拆分窗口后原终端无法输入（RidgePane unpark 不重新注册 dataHandler）✅ 2026-05-04

- **文件**：`src/lib/components/RidgePane.svelte`（onMount + 三个 handler 提取）
- **现象**：用户报告「终端如果进行窗口拆分，原窗口依然无法正常输入」。键盘 focus 看起来对，光标也在闪，但每个按键都没传到 shell。padding 修复（§1.15）只解决了视觉 inset，输入丢失是独立的更深 bug。
- **根因（trace 全链路）**：
  1. `RidgePane.svelte:288/302/322` 把 `onPtyData / onPtyResize / onKernelEvent` 三个 handler **以箭头函数内联定义在 onMount IIFE 里**，每个闭包都捕获了**当时**的 component scope `alive`、`triggerBellFlash`、stores 等。
  2. `manager.onData/onResize/onEvent(paneId, cb)` 在 `manager.ts:670/689/662` 仅把 `cb` 存进 `entry.dataHandler / .resizeHandler / .eventHandler` 字段。
  3. SplitContainer 因 `(A|B)` → `((A|new) | B)` 包装而结构性 re-render：原 `<RgPane><Pane id=A/>` 卸载，新 `<SplitLayout>` 重挂载内部 `<RgPane><Pane id=A/>`。RidgePane id=A 走 onDestroy（`alive = false`，`manager.park(paneId)`） → onMount。
  4. `manager.park` 故意保留 dataHandler/eventHandler/resizeHandler（注释清楚说「load-bearing for user-perceived continuity」）。
  5. RidgePane 新实例 onMount 命中 `if (manager.isParked(paneId))` 走 unpark 分支，原版代码在 `setFocused / setPadding` 后**立即 return**，**不会重新调 manager.onData/onResize/onEvent**。
  6. entry.dataHandler 仍指向旧实例的箭头函数，闭包里 `alive === false`（旧实例 onDestroy 已置 false）。`onContainerKeyDown` → `manager.handleKeyDown` → `entry.dataHandler(bytes)` → `if (!alive) return;` —— **每个按键被静默吞掉**，不抛错、不 invoke、不打 console。
- **影响面**：
  - **dataHandler**：key/paste/IME composition 全部丢失（最显眼，用户立即报）。
  - **eventHandler**：CwdChanged/TitleChanged 没检 alive 但 `triggerBellFlash` 改的是旧 component 的 $state，新实例的视觉 bell 永远不闪；其他全局 store 写入仍生效。
  - **resizeHandler**：没检 alive，PTY resize_pane invoke 仍正常（这就是为什么 §1.15 padding 修好后 fitPane 仍能下发 SIGWINCH 但用户依然无法输入）。
- **修法（已实施）**：把三个 handler 从 onMount IIFE 内提到 `<script>` top-level（紧跟在 `triggerBellFlash` 定义之后），每个 RidgePane 实例自然拥有自己的 `function` 标识符，闭包通过名字捕获**当前**实例的 `alive`、`paneId`、`workspaceId`、`triggerBellFlash` 等。然后在 onMount 的两个分支（首次 attach + unpark）都调一次 `manager.onData(paneId, onPtyData)` / `manager.onResize(paneId, onPtyResize)` / `manager.onEvent(paneId, onKernelEvent)`。Manager 的 onData 等方法会**替换**之前注册的 callback（docstring 已写明，无需先 clear）。
- **测试**：`pnpm check` 0 errors 0 warnings。浏览器实跑（§7.2）路径：`A` pane 输入正常 → 拆分得到 `A | B` → 在 A 中继续输入应正常工作（不再吞）。
- **设计教训**：手动跨 mount 保活的 manager API 必须在 unpark 路径上重新注册所有承载 component scope 的 callback，或把这些 callback 做成 manager 内部方法 + ref-to-current-component 模式。本次只补 RidgePane 这一个调用点；如果未来再加新 handler（`manager.onPaste`、`manager.onSelection` 等），onMount 两个分支都要补。

---

## 2. Round 4 收尾（IME v3 / 反向 scrollback / reflow）

### 2.1 后端 scrollback bridge — `Shift+PageUp` 越过 wasm buffer 边界 ✅ 2026-05-03

- **背景**：OVERVIEW §R5。前端 wasm kernel 的 scrollback 容量等于 `scrollbackLines: 2000`；后端 `state.rs` 保留 4 MB 块。原本翻历史超过 2000 行就翻不动了。
- **实施**：
  - **Rust kernel**（`packages/ridge-term/src/term/scrollback.rs`）：新增 `Scrollback::push_front(row) -> Option<Row>`；满时 evict 最新一行（最小代价权衡：用户在主动翻深历史，最新一行通常仍在 live grid 上）。覆盖 5 个单测：under/at capacity、空环、混合 push/push_front、capacity=0 边界。
  - **Rust kernel**（`packages/ridge-term/src/term/terminal.rs`）：新增 `Terminal::prepend_scrollback(&[u8])`，sandbox 法——同尺寸临时 Terminal 解析字节，强制 flush 主屏到 sandbox scrollback，丢弃 sandbox `pending_response`/`pending_events`，trim 末尾空行，AttrId 跨 AttrTable remap，按反序 push_front 到主 scrollback。覆盖 6 个单测：纯文本顺序、SGR 颜色 remap、不发 OSC 事件、不扰动 live state、空字节 noop、capacity 溢出 evict 行为。
  - **wasm-bindgen**（`packages/ridge-term/src/lib.rs`）：导出 `TerminalKernel::prependScrollback(bytes)` 给 JS。
  - **manager**（`src/lib/terminal/manager.ts`）：新增 `prependScrollback(paneId, data)` 转发到 wasm；不调 Tauri invoke，保持 host-agnostic。
  - **RidgePane**（`src/lib/components/RidgePane.svelte`）：track `oldestSeq` / `atOldest` / `pendingScrollbackFetch`；初次挂载 tail replay 时从 chunk 的 `start_seq`/`at_oldest` 初始化；`fetchOlderScrollback()` 调 `get_pane_scrollback_before` 并 prepend；`maybePrefetchOlder()` 在 Shift+PageUp / wheel-up 路径上 fire-and-forget 触发，距顶 ≤ 1 viewport 时预取 128 KB。
- **测试**：103 单测 + 22 集成测试全绿（含 11 条新增），svelte-check 0 错误。
- **未做**：浏览器实跑验证（属于 §7.2 范围）；多次连续 fetch 的边界（atOldest 后用户继续按 Shift+PageUp 是 no-op，符合预期）。

### 2.2 IME v3 `MutationObserver` 守护 ⏳

- **场景**：未来如果 portal/dragdrop 把 RidgePane 容器 reparent，`imeHelper` 的绝对定位会失效。
- **文件**：`src/lib/components/RidgePane.svelte`
- **修法**：可选——在 dev 环境观察到布局抖动再加。当前不阻塞。

### 2.3 Resize reflow（软换行行重排）⏳ Phase 1 ✅ / Phase 2 远期

- **背景**：原 `Grid::resize`（grid.rs:185）只做 truncate/pad — 收窄丢字符，拉宽留空白；翻历史时 scrollback 也错位。120 ms debounce 已经实现「松开鼠标后才触发」。Phase 1 已落地 reflow 本体（live grid 主屏幕列变重排），Phase 2 还差 scrollback + selection / hyperlink 锚点迁移。
- **设计参考**：`OVERVIEW.md §7「Resize reflow 设计」` —— 完整的方案对比、分阶段交付、算法、测试覆盖。
- **文件**：`packages/ridge-term/src/term/grid.rs::resize` + 私有方法 `reflow_primary`（已实现，line 262）。

#### Phase 1（本轮）— live grid 列变 reflow，仅主屏幕 ✅ 2026-05-03

- **状态**：完成。`grid.rs::resize` 当 `cols` 改变且 `!is_alt` 时调 `reflow_primary(new_rows, new_cols)`；alt 屏幕仍走 truncate/pad。
- **测试**（10 条全绿）：`reflow_shrink_wraps_long_line`、`reflow_grow_unwraps_continued_line`、`reflow_preserves_cursor_logical_position`、`reflow_skips_alt_screen`、`reflow_chain_of_three_rows_round_trip`、`reflow_no_op_when_cols_unchanged`、`reflow_preserves_pending_wrap_at_exact_boundary`、`reflow_no_pending_wrap_when_line_doesnt_fill_last_row`、`reflow_keeps_wide_char_intact_at_boundary`、`reflow_shrink_overflow_pushes_to_scrollback`。覆盖原 §2.3 列出的 6 条加 4 条边界（pending_wrap 双向 + wide-char 切片 + scrollback 溢出）。
- **不做（留 Phase 2）**：scrollback reflow、selection / hyperlink 锚点跨 reflow 迁移。

##### Phase 1 算法记录（已实施）

- **作用域**：仅当 `cols` 改变且当前是主屏幕（`!is_alt`）时触发；alt 屏幕（vim/less/htop）维持原 truncate/pad，依赖 SIGWINCH 让 TUI 自己重画。
- **算法**：
  1. stitch wrapped 链 → 逻辑行（用 `row.wrapped` flag）。
  2. 扫描时记录 cursor 所在的逻辑偏移 = 累积 cells + cursor.col。
  3. 清空 grid，按 `new_cols` 重新切片逻辑行，最后一段以外都 `wrapped=true`。
  4. 重设 cursor: row = offset / new_cols, col = offset % new_cols。
  5. 行数溢出时最旧的进 scrollback（与正常 LF 滚动一致）。
- **副作用**：selection 清空（最简策略，用户重新选）；hyperlink span 跟着 cell 搬运（per-row 元数据按新行 col 范围重写）。
- **触发路径**：复用 `manager.ts::viewportChanged` 的 120 ms debounce → `fitPane` → `kernel.resize`。无需新触发器。

#### Phase 2（round 5+）— scrollback + 锚点迁移 ⏳

- scrollback ring 同算法重排（4 MB 全量一次几十 ms 可接受）。
- selection 锚点：reflow 前记下逻辑行 + offset，reflow 后按新列宽推回 (row, col)。
- hyperlink 跨 row 边界的连续 span 在 reflow 后保持完整（当前 per-row 模型，需要先合并再切片）。

#### 不采用的方案（参考决策记录）

- 发假 `Ctrl+L` —— 入侵 PTY 字节流，丢有用内容，且不修 scrollback。用户明确说"避免重复输出"。
- 让 shell 重画就够了 —— TUI 没问题（SIGWINCH 已发），但 shell 输出沉到 scrollback 后没法重画，仍然需要 grid 端的 reflow。

### 2.4 Grapheme cluster（emoji ZWJ 序列）⏳ 远期

- **现状**：0-width 字符直接丢，`👨‍👩‍👧‍👦` 这种会拆成 4 个独立 emoji。
- **决定**：等用户报告再做，依赖 `unicode-segmentation` 接入。

---

## 3. Round 5 收尾（OSC UI 接入）

### 3.1 验证 `paneOscTitleStore` 真的驱动 SplitContainer 标题 ✅ 2026-05-03

- **文件**：`src/lib/components/SplitContainer.svelte:566`，`src/lib/components/RidgePane.svelte:259-269`
- **核对结论**：`SplitContainer` 在 line 566 直接读 `$terminalTitles[node.id]`，line 568 用 `proc = titleStr || fgProc` 让 OSC 标题盖过 `paneForegroundProcessStore`（polled 进程名）。RidgePane 在 `TitleChanged` / `IconNameChanged` 事件中同时写 `terminalTitles` 与 `paneOscTitleStore`，链路完整、优先级正确。
- **附带发现**：`paneOscTitleStore` 当前没有任何读者（grep 仅 RidgePane 写、`paneTree.ts` 定义）。属于"OSC-only" 备用通道；保留以备将来 Explorer 等位置区分 OSC vs polled，移除也无害。

### 3.2 `HyperlinkOpen`/`HyperlinkClose` 事件只是 `console.debug` 占位 ✅ 2026-05-03

- **文件**：`packages/ridge-term/src/term/parser.rs`、`packages/ridge-term/src/term/terminal.rs`、`src/lib/terminal/manager.ts`、`src/lib/components/RidgePane.svelte`
- **决策**：删除。
- **理由**：链接的所有功能（renderer 下划线、Ctrl+click 打开、Ctrl+hover pointer cursor）都通过 cell 级 `kernel.hyperlinkAt(row, col)` 读 `Row.hyperlinks` 实现；`HyperlinkOpen`/`HyperlinkClose` 事件没有任何下游订阅者，纯属冗余噪声。
- **实施**：
  - parser.rs 不再 `pending_events.push(HyperlinkOpen/Close)`；`current_link` 仍然按原逻辑维护，确保 cell-level 注解一切照旧。
  - `KernelEvent` enum 删除两个 variant（Rust 侧 + TypeScript union）。
  - RidgePane 的 switch 删除两个 case 占位。
  - 旧测试 `osc_8_open_then_close_pair` 改写为 `osc_8_open_then_close_pair_does_not_emit_events`，断言事件队列为空；`osc_8_marks_cells_with_link_span` 仍然守住 cell-level 行为。
- **测试**：103 lib + 22 integ 全绿；svelte-check 0 错误。

### 3.3 Bell 音频 ⏳ 远期

- **现状**：仅视觉 flash。
- **决定**：用户主动要再加。

---

## 4. Round 3 — WebGPU 后端 + 字形 atlas

### 4.1 `WebGpuBackend` 骨架 ✅ 2026-05-04（全部完成）

scaffold ✅ + §4.1.a-d 全 GPU 链路 ✅ + set_font_config ✅ + AnyBackend enum 分发 ✅ + **RenderHandle 切到 Renderer<AnyBackend> + `newWithWebgpuFirst` async constructor ✅（§4.1.e 完成）**。

`pnpm tauri build --features webgpu` 时 JS 侧 `await RenderHandle.newWithWebgpuFirst(canvas)`：先尝试 WebGPU adapter，失败回退 Canvas2D。默认构建 JS 走 `new RenderHandle(canvas)` 同步 path（Canvas2D 直接），newWithWebgpuFirst 函数不存在（JS 用 `typeof` 探测）。Round 3 §4.1 功能上完成 — 等用户 opt-in 测试。

- **文件**：`packages/ridge-term/src/render/webgpu.rs`（新增），`packages/ridge-term/src/render/mod.rs`（新增 `#[cfg(all(target_arch = "wasm32", feature = "webgpu"))] pub mod webgpu;`），`packages/ridge-term/Cargo.toml`（新增 `[features] webgpu = []`）
- **关键 API**：
  - `configure(font, size, dpr) -> (cellW, cellH)`
  - `render(rows: &[RowDraw], cursor: CursorDraw, frame: FrameMetrics)`
  - `apply_theme(theme)`
- **状态**：`WebGpuBackend` struct + `impl RenderBackend` 全部 9 个方法签名已就位，trait 契约对齐 `backend.rs`。`new()` 当前返回 `Err("WebGpuBackend not yet implemented — see TASKS §4.1")`，9 个 trait 方法体一律 `unreachable!()`——只有真实接线时才会被构造，scaffold 阶段不可能命中。
- **Feature flag**：`#![cfg(all(target_arch = "wasm32", feature = "webgpu"))]` 双重门禁。默认构建（`pnpm tauri build` / `wasm-pack build` / `cargo check --target wasm32-unknown-unknown`）**完全不编译** webgpu.rs，wasm 包大小不变。`cargo build --features webgpu` 编译 trait surface 检查；`pnpm tauri build --features webgpu`（待 build.mjs 支持）将来用来打实际 GPU 包。两种模式下 host `cargo test --lib` 维持 234 passed 无变化。
- **wgpu dep ✅ 2026-05-04**：Cargo.toml 加 `wgpu = { version = "23", default-features = false, features = ["webgpu"], optional = true }` + `wasm-bindgen-futures = { version = "0.4", optional = true }`。`webgpu` cargo feature 改为 `["dep:wgpu", "dep:wasm-bindgen-futures"]`。wgpu 自身 `webgpu` feature 是浏览器 WebGPU 后端，不拉 native Vulkan/Metal/DX12，wasm 包损耗最小。`cargo check --target wasm32-unknown-unknown -p ridge-term --features webgpu` 0 错误（首次拉取 wgpu 23.0.1 + wgpu-core/hal/types + naga + 周边 = 18s 编译完成）。
- **下一步**：`WebGpuBackend::new(canvas: HtmlCanvasElement) -> async Result<Self, String>`，`async fn` 拿 wasm-bindgen-futures 跑 `instance.request_adapter().await` + `adapter.request_device().await`；adapter miss / device 创建失败 → 返回 Err 让 caller fallback Canvas2D。`new()` 拿到 device + queue + surface 后 configure swap chain。本步骤不接 glyph，仅做「能 reach canvas」的 baseline。glyph 路径走 §4.1.b。
- **下一步（接线）**：
  1. `Cargo.toml` 加 `wgpu = "23.0"` 在 `[target.'cfg(target_arch = "wasm32")'.dependencies]`，`web-sys` features 加 `"GpuCanvasContext"`。
  2. `WebGpuBackend::new(canvas: HtmlCanvasElement)` request adapter + device → create surface → configure swap chain；adapter miss 时 fallback Canvas2D。
  3. 持有 `super::glyph_atlas::GlyphAtlas`（§4.2 已 ✅）。
  4. `cosmic-text` 或 `fontdue` 栅格化新字形 → 上传到 texture array → 填 `GlyphEntry`。
  5. `draw_row` 构建 `(cell_xy, atlas_uv, fg_rgba, bg_rgba)` instance buffer，每行一次 indirect draw。
  6. cursor / selection / hyperlink overlay 各一个 small pipeline pass（full-quad shader + scissor rect）。
- **注意**：**`RenderHandle` 当前硬编码 `Canvas2dBackend`**（`src/lib.rs`），接线 §4.1 时需要改成 `Box<dyn RenderBackend>` 或 wasm-bindgen 从 JS 传入选择标志。Err-on-construction 模式让本提交不需要修改 `lib.rs`，未来只改 `new()` 函数体即可。
- **测试**：`cargo check --target wasm32-unknown-unknown --manifest-path packages/ridge-term/Cargo.toml --lib` 0 错误；host `cargo test --lib` 仍 120 passed（不变）。

### 4.2 `GlyphAtlas` 数据结构 ✅ 2026-05-04

- **文件**：`packages/ridge-term/src/render/glyph_atlas.rs`（新增）
- **设计要点（已实施）**：
  - `GlyphKey { font_family_hash: u64, font_size_q: u16, glyph_id: u32, style_flags: u8 }`——color 故意不进 key（SDF/coverage 渲染时 shader uniform 渲染）；font_size 量化为 1/100 px 防 DPR rounding 撕裂。
  - `GlyphEntry { layer: u16, uv: [f32; 4], advance: f32, ascent_offset: f32, px_w: u16, px_h: u16 }`。
  - LRU 淘汰：`HashMap<GlyphKey, GlyphEntry>` + `VecDeque<GlyphKey>`（MRU 在 back）。`lookup` 提升到 MRU；`insert` 满时 pop_front 并返回被驱逐 key 让 backend 释放纹理槽。
  - 字形栅格化暂未集成（接 §4.1 时引 `cosmic-text` 或 `fontdue`）；本数据结构 GPU/字体库无关，host 可测。
  - `capacity == 0` 退化分支：直接 reject 新插入并把 key 当作"被驱逐"返回。
- **解耦**：atlas 与 `WebGpuBackend` 解耦——纯数据结构，`mod.rs` 中 `pub mod glyph_atlas;` 不带 cfg 门，host build 也编译。后续 Canvas2D 若需要也能读（虽然 Canvas2D 现走浏览器原生 fillText 不需要）。
- **测试**：7 条单元测试，全部 host pass（`cargo test --lib glyph_atlas`）：lookup 缺失/命中、eviction 容量边界、LRU promotion 顺序、duplicate insert 替换不驱逐、capacity-0 拒绝、clear。120 passed total（113 + 7）。

### 4.3 共享 surface（OVERVIEW D1）⏳ — 详细设计 2026-05-05

- **背景**：当前 round 2.4 是每 pane 一个 `<canvas>` + 一份 wgpu::Surface + 一份 GlyphAtlas + 一份 RenderHandle。OVERVIEW §D1 的最终态：全局一个 canvas + 一份 wgpu::Device/Queue + 一份 GlyphAtlas，每 pane 在该 surface 的子矩形里通过 scissor 渲染。
- **依赖**：§4.1 已 ✅（WebGpuBackend 单 pane 路径已落地）；§7.2 浏览器实跑回归通过（用户验证 §1.20/§1.21/§7.2/§1.22/§1.23 都正常）。
- **预期收益**：N pane 时 1 个 GPU ctx + 1 份 atlas（vs 现在 N 份），10 pane 配置内存从 ~60 MiB（atlas 6 MiB × 10）降到 ~6 MiB；同时减少 wgpu adapter request × N 的初始化开销。
- **范围限定**：本节仅适用于 WebGPU 路径。Canvas2D fallback 仍保留每 pane 一个 `<canvas>`（CanvasRenderingContext2D 不便于 scissor 划分；Canvas2D 路径用户量小且不是 D1 优化目标）。

#### 架构

```
┌── ridge-app (Svelte) ──────────────────────────────────────────────┐
│  +page.svelte                                                       │
│   └── <canvas data-rg-shared-surface>  ← ONE canvas, overlay-mode  │
│        + position absolute, z-index ↓ vs SplitContainer            │
│   └── <SplitContainer> ... <RidgePane>×N                           │
│         RidgePane: 仅占位 div，向 SharedSurface 报告 rect           │
│                                                                     │
│  SharedSurface (TS singleton in manager.ts)                         │
│   ├─ canvas: HTMLCanvasElement                                      │
│   ├─ handle: SharedRenderHandle (wasm)                              │
│   ├─ panes: Map<paneId, { kernel, viewportRect, dpr }>             │
│   ├─ ResizeObserver on canvas → handle.resizeSurface(...)           │
│   └─ rAF loop: for each pane → scissor(rect) + handle.renderPane    │
└─────────────────────────────────────────────────────────────────────┘
                              │
                              ▼ wasm-bindgen FFI
┌── ridge-term (Rust → WASM) ────────────────────────────────────────┐
│  SharedRenderHandle { renderer: SharedRenderer<AnyBackend> }        │
│   ├─ SharedSurfaceBackend (replaces per-pane WebGpuBackend)         │
│   │   ├─ device, queue, surface (1×)                                │
│   │   ├─ pipeline, atlas (1×)                                       │
│   │   ├─ atlas_texture, sampler (1×)                                │
│   │   └─ instance_buffer (grows for the largest frame across panes) │
│   └─ render_pane(paneId, kernel, scissor_rect) — single-pane draw   │
│                                                                     │
│  Renderer::tick(...) takes a `scissor_rect: Option<Rect>` and sets │
│  it on the wgpu RenderPass before draw calls; absent = full surface.│
└─────────────────────────────────────────────────────────────────────┘
```

- **关键约束**：浏览器一个 `<canvas>` 只能创建一个 `wgpu::Surface`。所以共享 surface 必须用 SAME canvas 元素。该 canvas 用 `position: absolute` overlay 在 SplitContainer 上方，`pointer-events: none`（事件让 underlying div 接管），但绘制覆盖整个内容区。
- **scissor 用法**：每帧 begin RenderPass 后，按 pane 顺序：`pass.set_scissor_rect(x, y, w, h)` → 仅清该 pane 的 bg rect → 绘 cells/cursor/overlay → 下一个 pane。

#### 坐标转换

Pane 有自己的 cell 网格 (cols, rows)。每个 cell 的 device-px 坐标在 SHARED canvas 中是：

```
// pane_rect: 该 pane 的 device-px 矩形 (x, y, w, h) within shared canvas
// (col, row): 该 pane 内的 cell 坐标
let pixel_x = pane_rect.x + (col as f32 * cell_w).floor();
let pixel_y = pane_rect.y + (row as f32 * cell_h).floor();
```

整数对齐（§7.2.c）仍按 pane-local 坐标做，再加 pane_rect.x/y offset。CellInstance 的 pixel_xy 直接是 shared-canvas 坐标。FrameUniform.viewport 仍是 shared canvas 的总尺寸。

scissor rect (set_scissor_rect) 限制 fragment 写入到 pane 区域内，所以 cell quad 即使溢出 pane 也只画 pane 内部分。这保证 splitter 拖动时不会有像素跨 pane 边界。

#### Atlas 共享

当前 `WebGpuBackend.atlas: GlyphAtlas` per pane。改为 `SharedSurface.atlas` 一份。GlyphKey 已经包含 (font_family_hash, font_size_q, glyph_id, style_flags)，多 pane 都用同字号同字体时 atlas hit 率接近 100%。eviction LRU 跨 pane 共享。

实现：
1. 把 `WebGpuBackend.atlas / atlas_texture / atlas_view / next_free_layer / rasterizer` 字段从 per-pane 移到 SharedSurfaceBackend。
2. `draw_row` cell-rasterize-on-miss 路径改成对 SharedSurface 加锁/借用（Rust 借用规则可能要求 `RefCell<atlas>` 或者 `&mut self` 上下文重组）。
3. `set_font_config(family, size_px)` 仍然 per pane 设置（不同 pane 可以不同字体，已经被 GlyphKey 分桶），但 rasterizer 是共享的——传入 family 参数即可。

#### 渲染循环

`SharedSurface` 在 `attach(paneId, container, rect)` 时把 pane 加入 `panes` map。manager 的 rAF tick 改为：

```ts
function tick() {
  // 1. Update each pane's viewport rect from its container DOM rect.
  for (const [paneId, entry] of this.panes) {
    const r = entry.containerEl.getBoundingClientRect();
    const canvasR = this.canvas.getBoundingClientRect();
    entry.rect = {
      x: (r.left - canvasR.left) * dpr,
      y: (r.top - canvasR.top) * dpr,
      w: r.width * dpr,
      h: r.height * dpr,
    };
  }
  // 2. Render all panes in one frame.
  for (const [paneId, entry] of this.panes) {
    if (entry.parked || isOffscreen(entry.rect)) continue;
    this.handle.renderPane(paneId, entry.rect);
  }
}
```

`handle.renderPane(paneId, rect)` 在 Rust：
1. Lookup the pane's kernel + selection.
2. Call `Renderer::tick(kernel, selection, now_ms)` BUT with the rect threaded into the backend so `begin_frame`/`draw_*`/`end_frame` apply scissor.
3. Note: `end_frame` no longer presents per-call; it accumulates all panes' instances and submits ONE frame at the end. Driver: SharedSurface owns a "this-frame's instances" buffer and calls `flush_frame()` after iterating all panes.

#### Resize

两类：
- **Canvas resize**：window resize / monitor DPR change → SharedSurface gets ResizeObserver tick → `handle.resize_surface(canvas.width, canvas.height, dpr)`.
- **Pane rect change**：SplitContainer drag-resize → pane's `containerEl.getBoundingClientRect()` changes → next rAF tick picks up new rect。PTY SIGWINCH still goes through per-pane `kernel.resize(rows, cols)` independently.

§1.22 alt-screen-clear-on-resize 仍 per-pane 生效。

#### Attach / Detach

- `attach(paneId, container)`：create wasm `Kernel` + register pane in SharedSurface map。NO new wgpu::Surface — reuse the shared one。NO new canvas in DOM — RidgePane just owns its layout div。
- `detach(paneId)`：drop the kernel; remove from map。Atlas LRU continues to evict that pane's glyphs naturally。
- `park` / `unpark`（§5.1）：unchanged contract，just kernel preservation across remount。

#### RidgePane.svelte 变化

- 移除内部 `<canvas>` 创建路径（manager.attach 不再绑 canvas，绑 div + rect）。
- IME helper textarea / focus tracking / context menu / scrollbar / scroll-to-bottom 浮动按钮（§1.23）—— 全部保留（与渲染层无关）。
- 视觉效果上：用户应该完全察觉不到差异——pane 边界、cell 渲染、cursor 位置全一致。

#### Phase 化交付

1. **Phase A**（✅ 2026-05-04，§4.5 a-e 已 done）：单 pane WebGPU 跑通（这是 §4.1 的实质收尾）。
2. **Phase B**（本节起步）：把 atlas / rasterizer / device 提到 SharedSurfaceBackend，但仍是单 pane（保留 SharedSurface 单实例 + 1 pane）。验证 atlas 跨 pane 复用的代码路径在单 pane 也跑得通。
3. **Phase C**：HTML canvas 从 RidgePane 移到 +page.svelte 顶层（overlay）；RidgePane 改为只报告 rect。manager 实现 N-pane render 循环 + scissor。
4. **Phase D**：性能基准（§4.4）— 对比单 pane / 4-pane / 10-pane 的 FPS、frame time p99、JS heap、GPU memory。

#### 风险

- **R1**：scissor 和整数对齐（§7.2.c）交互——pane_rect.x 必须也整数对齐，否则 scissor 边界与 cell 边界不一致，会出现 1px 漏画。Phase C 实施时 pane_rect 计算用 floor 同样的方式。
- **R2**：overlay canvas 的 z-index 和 SplitContainer 子元素事件穿透——必须 `pointer-events: none` 且 IME helper / 滚动条 / contextmenu 都来自 underneath div，不来自 canvas。需要 RidgePane 完整保留这些 affordance。
- **R3**：多 pane 的 wgpu 资源生命周期——单 SharedSurface owns Device/Queue，pane 进出不应触发 device.poll() 或 swap chain reconfig。validate by 10 pane 极限测试。
- **R4**：Selection 跨 pane——当前 selection 在每个 kernel 内部 (selection.rs)。共享 surface 后，pane 之间互不影响，但 SplitContainer 拖动 pane 时旧 selection rect 没清。需要 `paneTreeStore` 监听拖动结束，对受影响 pane 调 `kernel.clearSelection()`。

#### 测试

- 单 pane 在 SharedSurface 走通后：选中、滚动、resize、字符画、IME、Ctrl+F 搜索 全部回归。
- 多 pane 极限：10 pane 各跑 `seq 100000`，监控 GPU 内存、FPS、wasm heap。
- 拆分动画：拖 splitter 时 pane rect 实时更新，不出现裂痕 / 残留。

### 4.4 性能基准 ⏳

- **任务**：在 round 3 完成后跑同一个 PTY 录制（例如 `cat large.log`、`vim` 滚动）对比：
  - xterm + WebGL（旧）
  - ridge-term Canvas2D（round 2.4）
  - ridge-term WebGPU（round 3）
- **指标**：FPS、frame time p99、JS 主线程占用、显存
- **OVERVIEW R2 风险**：第一版 WebGPU 也可能不如 xterm，预期 1-2 轮调优。

---

## 5. Round 6 — parking lot + 双 scrollback 去重（实质收尾：§5.1 ✅ / §5.2 决定不做）

### 5.1 split 时保活 pane（park/restore）✅ 2026-05-03

- **背景**：xterm 时代用 `parkTerminal/restoreTerminal` 缓存 DOM；RidgePane 起初每次卸载即销毁 wasm kernel，PTY 由后端保留但 scrollback / 选区 / 搜索 / IME state 全丢。
- **方案选定**：A（保活 kernel + 跨 mount 的持久 PTY listener）。理由对比详见会话讨论；核心是 B 路径要修 Bell/TitleChanged 重发 + 选区/搜索/scroll 丢失 等隐性问题，实施成本反而不比 A 低。
- **实施**：
  - **`manager.ts`**：新增 `PaneEntry.parked: boolean` 字段，新增 `park(paneId)` / `unpark(paneId, container)` / `isParked(paneId)`。`park()` 释放 canvas + RenderHandle + ResizeObserver + focus / pointer listener，保留 kernel + dataHandler / eventHandler / resizeHandler 闭包。`unpark()` 创建新 canvas + 新 handle 绑回旧 kernel，重装 listener（闭包查 `this.panes.get(paneId).container` 自动看见新容器）。`detach()` 兼容两种状态。rAF tick / `setFont` / `setTheme` / `viewportChanged` 加 `parked` skip 守卫。
  - **`ptyBridge.ts`（新文件）**：把 `pty-output-{ws}-{pane}` 与 `pane-pty-closed` listener 从 RidgePane 抽到此 sidecar。listener 生命周期跟 wasm kernel 走（`ensurePtyBridge` ↔ `teardownPtyBridge`），不跟 Svelte 组件 mount/unmount 走——这样 split / reparent 的 unmount 窗口期 PTY 字节继续 feed 进 parked kernel，不丢。`pane-pty-closed` 重建路径（§1.1 修复）也搬到这里，无论组件挂没挂都能正确续命 PTY。
  - **`RidgePane.svelte`**：`onMount` 分支判断 `manager.isParked(paneId)`——是则 `unpark` 走轻量重绑路径（无 create_pane / 无 scrollback replay / 无 activate_pane_pty / 无 listener 重订）；否则走完整 attach。`onDestroy` 改调 `manager.park` 而非 `detach`，不再清 title store（kernel 还活着仍在写）。
  - **`paneTree.ts::closePane`**：在 `invoke('close_pane', ...)` 之后按序调 `teardownPtyBridge → manager.detach → 清 title 两个 store`，作为"真正销毁"的统一收尾路径。
- **测试**：svelte-check 0 错误；wasm kernel 单测不受影响（103 + 22 仍通过）。
- **未做**：orphan kernel sweeper（远期防御性兜底，对正常 closePane 路径不需要）；`closeWorkspace` 路径的批量 detach（当前依赖 close_pane 逐个回调，可能漏；待 §7.2 浏览器实跑验证）。

### 5.2 删除前端 wasm kernel 的 scrollback 重复 ⏳ 需更深设计（不是 config 调整）

- **背景**：OVERVIEW §R5。kernel 自带 2000 行 scrollback（~700 KB/pane）；后端 4 MB block 也存。表面看可以把 kernel 容量降到 256 行让"深翻"走 §2.1 反向 bridge。
- **2026-05-03 设计核查发现**：和 §2.1 的 `Scrollback::push_front` "满时 evict newest" 策略冲突。具体 trace：
  - 用户初始化时 kernel feed 256 KB tail，最终 scrollback 持有最近 256 行（recent 行）。
  - 用户滚到顶 → `maybePrefetchOlder` → 后端 128 KB ≈ 1500 行 → 反序 push_front。
  - 满时每次 push_front evict newest——前 256 次把所有 recent 行 evict 殆尽；剩下 1244 次互相 evict，最终 kernel 只剩 256 行**最老**的 historical。
  - 用户滚回底部时，scrollback 与 live grid 之间出现"时间断层"，看不到 PTY 刚滚下去的最近输出。
- **可选改进路径**（按工程量从小到大）：
  - **A. 维持现状不去重**：接受 ~700 KB/pane 重复存储。10 pane 总浪费 ~7 MB，相对 backend 的 40 MB 不算夸张。**当前推荐**。
  - **B. 改 push_front 满时 evict OLDEST**：保留 recent 行。但这样 prepend 不能真正扩展 effective scrollback——无论 fetch 多少历史，scrollback 永远 capped。深翻仍卡。
  - **C. Scrollback 增加独立的 `prepended_extra: Vec<Row>` 区段**：feed 路径用 ring（capped），prepend 路径写入 extra（unbounded）；`viewport_row` 计算先看 ring 再看 extra。改动深，需要重构 Grid::viewport_row + Terminal::scroll_offset clamp。
  - **D. 给 Scrollback 加 byte-seq 索引**：每行 row 携带 source byte_seq；fetch 时根据 kernel 当前 oldest_row.byte_seq 决定从哪 fetch；带 deduplication。最完美但代价最高。
- **决定**：当前走 A，**§5.2 关闭为「不做」**，待用户报告内存压力或具体 UX 问题再启动 C/D。Round 6 因此实质收尾在 §5.1 + 此判断。

---

## 6. Round 7 — 删除 xterm（基本完成）

实际推进早于本计划：xterm 路径在本会话开始前就已被先期手术拆除。本节核对实际完成情况：

- ✅ `package.json` / `pnpm-lock.yaml` 无任何 `@xterm/*` 依赖（`grep` 0 matches）
- ✅ `src/lib/components/Pane.svelte` 已删除（git: `D src/lib/components/Pane.svelte`）
- ✅ `PaneRouter.svelte` 已删除；`SplitContainer.svelte` 直接 import `RidgePane`
- ✅ `src/lib/stores/settings.ts` 没有 `useExperimentalRenderer` 字段（参见 §7.1，moot）
- ✅ `src/lib/stores/terminalRegistry.ts` 已删除（git: `D`）
- ✅ 后端 `state.rs`：PTY 协议未变，按计划不动
- ✅ `CLAUDE.md` 项目描述已从"xterm.js + WebGL"更新为 "ridge-term wasm 内核 + Canvas2D 渲染器"
- ⏳ **全量 regress（属 §7.2 范围）**：跑 INTEGRATION_R2_4.md §Step 7-8 视觉验证清单 + 复杂 TUI（vim、lazygit、btop、ratatui demo）。需要用户 `pnpm tauri dev` 实跑。

---

## 7. 集成与验证遗留

### 7.1 ~~`useExperimentalRenderer` 没有写入 typed `UserSettings`~~ ✅ 2026-05-03（moot）

- **现状**：xterm 路径已 retire（`src/lib/components/Pane.svelte` 与 `PaneRouter.svelte` 都已删除），`RidgePane.svelte` 是唯一终端组件。`grep -rn useExperimentalRenderer src/` 无任何命中——这个 toggle 已经没有任何消费者。
- **决策**：不再补 typed 字段。这一项随 round 7 「删除 xterm」工作天然消失。如果未来需要类似的"实验渲染器"开关，到时候按 INTEGRATION_R2_4 §Step 5 重新加即可。

### 7.3 [LOW] fileExplorer.test.ts 两条 pre-existing 失败 ✅ 2026-05-03

- **背景**：2026-05-03 自审审计 `pnpm test` 时发现 2 条失败。`git stash` 验证两条都早于本会话；后续用 `git log -L 180,185:src/lib/stores/fileExplorer.ts` 追到精确 regression commit。
- **失败 1: E6**「preserves cached tree and sets needsRefresh when a new pane joins」
  - **根因**：commit `7f45cd5` 把 `needsRefresh: hasNewJoiner || existing.needsRefresh` 改成 `needsRefresh: existing.needsRefresh || (hasNewJoiner && existing.tree === null)`，只在 tree 为 null 时 refresh。
  - **判定 = regression**：上层 Chinese 注释（「保留旧树原地显示，在 Explorer 层异步调用 loadTree 做后台刷新」）+ Explorer.svelte:99 注释（「首次加载或 needsRefresh 都触发拉取」）+ E6 测试 三处都期望 cached-tree 场景也 refresh。`7f45cd5` 只更新了局部 English 注释，留下三处不一致——典型 review 失语。
  - **修法**：fileExplorer.ts 回退 `&& existing.tree === null` 约束，恢复 `existing.needsRefresh || hasNewJoiner`。
- **失败 2: flattenVisiblePaths**「lists root first then expanded-children in DFS order」
  - **根因**：`flattenVisiblePaths` 当时改为 `// Skip root — Explorer renders col.tree.children directly`（顶层文件夹层被移除），但测试期望仍含 root。
  - **判定 = test stale**：代码改动是有意（与 Explorer 渲染层一致），测试漏 sync。
  - **修法**：测试期望数组去掉 `/r`，改 `it(...)` 描述为「lists tree.children in DFS order respecting expandedPaths (root skipped)」。
- **结果**：`pnpm test src/lib/stores/fileExplorer.test.ts` 27/27 通过。CI `pnpm test` 不再红。

### 7.2 浏览器端真实跑通验证 ⏳ 高优先级

- **背景**：OVERVIEW §R1 风险。所有"看起来对"的代码迄今只在 Rust 单元测试通过，没有 `pnpm tauri dev` 内被人用过的证据（除最近修的 RidgePane 输入失效问题）。
- **任务**：按 INTEGRATION_R2_4.md §Step 7-8 八项视觉验证打钩，截图存到 `docs/term-rebuild/QA/`。
  - [x] prompt 显示
  - [x] 输入命令 + 回车有输出
  - [x] Ctrl+C 终止 sleep
  - [x] `ls --color` 看到颜色
  - [x] 拖 splitpanes 边界跟随
  - [x] `seq 200` 滚轮看历史
  - [x] Shift+PageUp / Shift+PageDown 翻页
  - [x] 选段 → 右键复制
- **附加验证项**：
  - [x] vim/less 退出后主屏内容恢复（alt screen ?1049）
  - [x] 输入中文（IME 候选窗位置正确）
  - [x] Ctrl+F 搜索 + n/N 切匹配
  - [x] Ctrl+click OSC 8 链接打开

### 7.2.c WebGPU 运行时默认化 + 字体/可见性回归修复 ✅ 2026-05-05

- **背景**：用户实跑 §4.5 a-e WebGPU 路径后报告三个问题：(1) 字体太小太细显示得有些奇怪、(2) 不聚焦到对应输出行就不显示、(3) 想让 WebGPU 是默认行为，通过运行时适配器检测决定，不要打包时指定也不要 localStorage 硬编码门槛。
- **修复 1（字体小/细，atlas UV 没 crop）**：`packages/ridge-term/src/render/glyph_rasterizer.rs::rasterize` 加 `dpr: f32` 参数，按 `font_size_px * dpr` 渲染——OffscreenCanvas backing 是 device px，原代码 `{font_size_px}px` CSS 大小落到 device-px slot 上，DPR 2 时只占了一半 → 小+细。同时返回真实 bbox（device px，clamp 到 slot），让 `webgpu.rs::draw_row` 把 `atlas_uv` 从 `[0,0,1,1]` crop 为 `[0,0,bbox_w/slot_w, bbox_h/slot_h]`，避免 cell quad 把 32×32 slot 满采样而把小 glyph 拉到 cell 角落。
- **修复 2（非聚焦行不显示，dirty-row 与 LoadOp::Clear 冲突）**：`backend.rs::RenderBackend` 加 default-method `requires_full_frame() -> bool`（默认 false，Canvas2D 走 partial 重画）；`webgpu.rs` override 为 true。`renderer.rs::tick` 在 dirty 计算前 OR 进 `full_redraw_pending`——WebGPU 的 `LoadOp::Clear(theme.bg)` 每帧抹掉整张 swap-chain texture，dirty-row diff 会让上一帧未脏的行掉到 bg。强制每帧 full redraw 修好。
- **修复 3（WebGPU 默认化、运行时回退）**：
  - `Cargo.toml::[features].default = ["webgpu"]`（原 `[]`）——webgpu 模块进默认 wasm bundle，`RenderHandle.newWithWebgpuFirst` 静态方法始终被导出。
  - `packages/ridge-term/build.mjs`：`--webgpu` flag 退化为兼容 no-op；新增 `--no-webgpu` 显式排除（`--no-default-features` 给 cargo），用于 size-constrained 构建。banner / Usage 注释同步更新。
  - `src/lib/terminal/manager.ts::instance()`：`preferWebgpu` 默认为 `true`；只在 `localStorage.RIDGE_WEBGPU === '0'` / `=== 'false'` 时显式禁用（debug escape hatch）。注释明确："JS 端 typeof 检测 + Rust 端 request_adapter 失败"双层 runtime fallback。
  - `OVERVIEW.md` §3 line 121 + `CLAUDE.md` "Render backends" 段更新，移除 `--features webgpu` 引用，写明 default + runtime detection。
- **审查 hardcoded（用户附加约束）**：
  - `webgpu.rs::ATLAS_SLOT_W/H = 32/32` 太小——15 CSS px 字体 × DPR 2 行高 ≈ 36 device px，bbox 被 clamp 到 32 → descenders 'g'/'p'/'y' 截断。改为 `64/96` 覆盖 18 CSS × DPR 2.5 的实际范围，同时考虑 CJK 宽字符（cell.width=2）。每 atlas 内存 1 MiB → 6 MiB；当前每 pane 一份，§4.3 共享 surface 后收敛为单份。注释里写明未来可由 `set_font_config` 按需重新分配。
  - `font_size_px: 15.0` 默认硬编码 OK——`set_font_config` 由 JS 侧 settings 注入。
  - `present_mode: Fifo` / `desired_maximum_frame_latency: 2` 是 wgpu sample default，正常。
- **影响范围**：
  - Rust：`packages/ridge-term/src/render/{backend.rs,renderer.rs,webgpu.rs,glyph_rasterizer.rs}` + `Cargo.toml` + `build.mjs`
  - JS：`src/lib/terminal/manager.ts`
  - Doc：`OVERVIEW.md`、`CLAUDE.md`、`TASKS.md`（本条）
- **验证**：`cargo check --target wasm32-unknown-unknown --lib` 0 警告（默认 + `--no-default-features`）；`cargo test --lib` 240 通过；`pnpm check` 0 errors / 0 warnings (4098 files)；`node build.mjs` 重新打包 wasm 成功，pkg/ridge_term.d.ts 含 `static newWithWebgpuFirst`。**待用户实跑确认 (a) 字体大小正常、(b) 滚动 / 静止行都可见、(c) `localStorage.removeItem('RIDGE_WEBGPU')` 状态下默认走 WebGPU 路径**。
- **未做**：(i) WebGL2 中间层 backend——用户原文是「回退到 webgl 或 canvas」，当前只实现 Canvas2D 兜底；WebGL 是未来的 GPU 中间层选项，需独立 §。(ii) 动态 atlas slot 大小（按 set_font_config 重分配）——文档里标记为 future improvement。(iii) §4.3 共享 surface（依赖 §7.2 单 pane 验证通过）。

---

## 进度记录（append-only）

记录格式：`YYYY-MM-DD — 摘要 — commit_short_hash 或 PR 编号`

- 2026-05-03 — 初次创建本文档；核对 OVERVIEW.md §3 进度表后确认实际位于 round 2.4 末尾 + round 4/5 部分提前；触发因素是 RidgePane 输入失效 bug 修复回归 — `5c11914`
- 2026-05-03 — 同步更新 OVERVIEW.md §3 表格为真实进度（rounds 2.2/2.3/2.4 ✅，rounds 4/5 部分完成）— `5c11914`
- 2026-05-03 — 修复 RidgePane 键盘焦点失效（`onkeydown` 上提到 container + `onmousedown.preventDefault()` 防焦点抢占）— `5c11914`
- 2026-05-03 — §1.1 修复 `pane-pty-closed` 重建路径：handler 改 async，依序 `create_pane` + `activate_pane_pty`，每步 guard `alive`；svelte-check 0 错误（2 处 pre-existing 警告）— `5c11914`
- 2026-05-03 — §1.2 + §1.3 修复：`manager.ts` `feed()` 无条件 `takePendingEvents()` + dev-only warn；`RidgePane.svelte` `onDestroy` 清理 `bellFlashTimer`；svelte-check 0 错误 — `2ab56f3`
- 2026-05-03 — §2.1 反向 scrollback bridge 全链路落地：`Scrollback::push_front` + `Terminal::prepend_scrollback`（sandbox 法 + AttrId remap）+ wasm-bindgen `prependScrollback` + `manager.prependScrollback` + RidgePane `oldestSeq`/`atOldest`/`pendingScrollbackFetch` + `Shift+PageUp`/wheel 接入；新增 11 单测，103 lib + 22 integ 全绿，svelte-check 0 错误 — `99ad061`（JS 侧）；Rust 侧待 packages/ridge-term/ 整体入库时一并提交
- 2026-05-03 — §3.1 验证 `terminalTitles` 链路完整（SplitContainer:566 + OSC > polled-process 优先级）；`paneOscTitleStore` 仅写无读，标记为冗余通道但保留 — `0d943f5`
- 2026-05-03 — §3.2 删除冗余的 `HyperlinkOpen`/`HyperlinkClose` 事件：parser 不再 push，`KernelEvent` 去掉两个 variant，RidgePane 删 switch 占位，旧测试改写为断言事件队列为空；cell-level `kernel.hyperlinkAt` 仍是唯一可信链接源 — `0d943f5`
- 2026-05-03 — §7.1 标 moot：xterm 路径已 retire（`Pane.svelte` / `PaneRouter.svelte` 删除），`useExperimentalRenderer` 无任何 source 消费者 — `0d943f5`
- 2026-05-03 — `packages/ridge-term/` 整体入库（33 files / ~8.5k 行 / 含 635 KiB wasm 二进制）；新增 `packages/ridge-term/.gitignore` 排除 `target/` + `*.stackdump`；`build.mjs` 增加 step 4 删除 wasm-pack 自动生成的 `pkg/.gitignore` 让 `link:` 消费在 fresh clone 后即可工作 — `785557b`
- 2026-05-03 — 入库 6 个伴生设计文档（BUGFIX / INTEGRATION / INTEGRATION_R2_4 / PARTIAL_REDRAW_PROTOCOL / PROBLEM_COVERAGE / REPLACE_AND_FIX_PLAN，2400 行），至此 `docs/term-rebuild/` 全部 8 个 `.md` 全部入库 — `b78b742`
- 2026-05-03 — Round 7 核对完成：`package.json` / `pnpm-lock.yaml` / `src/` 全无 `@xterm/*` / `xterm-` 引用；CLAUDE.md 项目描述更新为 ridge-term wasm 内核（"Key features"、"Frontend"、"Terminal scrollback"三处）。剩余 Round 7 工作仅 §7.2 浏览器实跑回归 — `c85c942`
- 2026-05-03 — §5.1 parking lot 走方案 A 落地：`manager.park`/`unpark`/`isParked` + 新文件 `src/lib/terminal/ptyBridge.ts`（`ensurePtyBridge`/`teardownPtyBridge`，pty-output + pane-pty-closed 跨 mount 持久）+ RidgePane onMount 分支 + onDestroy 改调 park + paneTree.closePane 收尾 detach。svelte-check 0 错误；listener 不再依赖 Svelte 组件生命周期，split 不再丢 PTY 字节也不再丢选区/搜索/scroll/IME state — `d034a29`
- 2026-05-03 — §5.2 设计核查：发现简单缩 capacity 到 256 与 push_front evict-newest 冲突，深翻历史会 evict 所有 recent 行；改 evict-oldest 也无法扩 effective scrollback。决定走方案 A（不去重，接受 ~700 KB/pane 重复），§5.2 关闭为「不做」。Round 6 实质收尾；OVERVIEW §3 表格 Round 6/7 同步标 ✅。— `cebc8bd`
- 2026-05-03 — Selection bug 三连 + Resize 宽字符保护：§1.6 selection overlay alpha 叠加（renderer.rs::tick 追加 selection 行入 dirty_rows）；§1.7 主题色不匹配（新增 `src/lib/terminal/themeBridge.ts` 把 Ridge CSS 变量推到 wasm Theme，+page.svelte 启动时 wire）；§1.8 reflow 宽字符切片保护（grid.rs::reflow_primary 切片回退 + cursor 检查改 `< end`，新增 `reflow_keeps_wide_char_intact_at_boundary` 单测）。111 lib + 22 integ 测试全绿；svelte-check 0 错误。— `2d7363f`（同提交含用户 reflow Phase 1 工作）
- 2026-05-03 — §1.9 canvas resize 死循环修复：`canvas2d.rs::resize_surface` 不再用 `"{N}px"` 覆写 canvas CSS 宽高（之前会冻结尺寸导致后续 resize 全部 no-op），改写 `"100%"` 让 canvas 持续跟随容器。HTML attrs(device px backing) 仍单独管理。重建 wasm，111 lib + 22 integ 全绿。— `e716952`
- 2026-05-03 — §1.10 reflow cursor pending_wrap 边界修复：`grid.rs::reflow_primary` 抓 `cursor_src_pending_wrap`，stitch 时把虚拟 past-end 折进 offset（+1）+ post-push clamp 到 trim 后的 pushed_len；引入 `cursor_pending_wrap` 局部 bool，end-of-line `used==0` 分支保留 true，结尾按之赋值（不再无条件 false）。新增 2 单测，113 lib + 22 integ 全绿。— `b05e1fc`
- 2026-05-03 — §1.11 paneCwdStore identity-preserving 修复：`setPaneCwd` 在值未变时返回原 store 引用；新增 `mergePaneCwds` 助手，三处 bulk merge 路径（refreshWorkspaces / switchWorkspace / loadSavedWorkspaces）改用之；Ctrl+C / Enter / 其他 prompt 重画不再 fan-out 到 file tree / SCM / sidebar plugin。svelte-check 0 错误。— `971f7fa`
- 2026-05-03 — §1.11 follow-up：`syncPaneLayoutFromBackend` 之前误判为「已有 mutated 守卫」，实际不是。补上 mutated 跟踪——pass 1 drop 或 pass 2 seed 时才克隆，否则返回原 store。每次 splitPane / closePane / dockPane / switchWorkspace 调 sync 时不再误 fire（即使 pane 集合不变）。— `6065e8b`
- 2026-05-03 — §1.12 split drag fan-out 修复：新增 `SPLIT_DRAG_AUTO_COUPLING_ENABLED = false`；`startSplitResizeDrag` 默认 refs 只含 primary，`is4WaySnap` / `snapState.coupledSplitters` / `coupledSameAxis` / `coupledOrthoSiblings` 四条联动全部 gate；保留视觉 attractor。`(A|B) / (C|D)` 拖 A/B 现在只动 A 和 B。svelte-check 0 错误。— `deea0db`
- 2026-05-03 — §1.4 sync-output 超时 cool-down：`PaneEntry` 新增 `syncTimeoutRendered`，rAF tick 在超时分支只渲染一次 best-effort frame，之后 `continue` 直到 kernel 退出 sync；kernel 清 sync 时同步 reset。修了原注释承诺却未兑现的「only one render per cycle」语义。svelte-check 0 错误。— `666300c`
- 2026-05-03 — `packages/ridge-term/README.md` + `pkg/README.md` 同步到 round-7 后实际状态：删除「round 1 of N」「cannot yet replace xterm in your `Pane.svelte`」等过期声明；更新架构图反映 Terminal facade（pending_response / pending_events / prepend_scrollback）+ Grid（alt + DECSTBM + reflow_primary）+ Renderer（per-row dirty + selection anti-stack + cursor blink）+ Canvas2dBackend（CSS 100% / device-px attrs）；新增 round 状态表（1/2.1/2.2/2.3/2.4/5/6/7 ✅，3/4 ⏳）；What's implemented 列出实际涵盖（CSI ECH/ICH/DCH/REP/HPR/VPR/SCO save/DECSTBM/DSR/DA/DECSCUSR、ESC DECPAM/DECPNM/RIS、SGR truecolor 双语义、screen modes ?47/?1049/?2026/?1004/?6/4/20/all 鼠标 modes、OSC 0/1/2/7/8）；Tests 行数更新为 113 + 22；删除「Explicitly NOT in this round」表 + 「What I need from you for round 2」整段；Build 改成 `node build.mjs`/`--dev`。Consumer 段新增 manager.ts/ptyBridge.ts/themeBridge.ts/RidgePane.svelte 入口说明。 — `3c1252b`
- 2026-05-03 — §2.3 Phase 1 状态对齐：实测 `cargo test --lib reflow` 10/10 全绿（覆盖 6 条规约 + pending_wrap 双向 + wide-char 切片 + scrollback 溢出 4 条边界），TASKS.md 把 §2.3 从「⏳ 进行中」改为「Phase 1 ✅ / Phase 2 远期」；Phase 1 子标题加 ✅ 2026-05-03 + 「状态：完成」+ 测试清单；移除原「至少覆盖」的占位列表（已实兑）。Phase 2（scrollback reflow + 锚点迁移）状态保留 ⏳。 — `4fef1d4`
- 2026-05-03 — RidgePane defensive guard：onMount + onResize 入口加 `Uuid::parse_str` 校验，非 UUID 时（如 `split-1`）console.error 含 paneId/workspaceId 上下文 + return，不再 fan-out 到 backend `Invalid pane id`。配合 backend `parse_pane_id` 要 UUID 而 split id 是 `split-{counter}` 的事实，根因尚未定位（可能 paneTree 残留 / Svelte reconcile 竞态），但 console 不再被刷屏，且复发时一行 log 可直接还原现场。— `38398bd`
- 2026-05-03 — `manager.setPadding` 短路：`PaneEntry` 加 `lastAppliedPaddingPx`；clamped px 与上次相同时早返回，避免 Svelte 5 `$effect` 在 settings store 任意字段变化（字号、shell、glob）时给所有 pane 触发 `viewportChanged → fitPane` 级联。— `4c9389d`
- 2026-05-03 — `manager.fitPane` 加 dev-only 诊断 log（`import.meta.env.DEV` 下打印 `[ridge-term] fit <paneId> <cols>×<rows> (was ...)`），用于在浏览器端确认拖 splitter 时 CSS 布局只让相邻 pane 容器尺寸变，prod 用户不可见。— `6c8e35f`
- 2026-05-03 — 删除 `get_pane_scrollback` 过期 shim：xterm round 7 已 retire、Phase 3 paged reads 已通过 §2.1 反向 scrollback bridge 落地，shim 的两条理由都消失。删除点：`src-tauri/src/commands/terminal.rs` 整个 #[tauri::command]、`src-tauri/src/lib.rs` invoke_handler! 注册行、`src/lib/components/RidgePane.svelte` `get_pane_scrollback_tail` catch 内的 legacy fallback try/catch（合并为 `if (alive) atOldest = true`）。CLAUDE.md「Legacy `get_pane_scrollback` is a deprecated shim — keep it working until phase-3 wraps」改为已删除注记；`docs/TERMINAL_SCROLLBACK.md` Phase-plan 表加 Status 列，Phase 0/1/2/3/4 全标 ✅，移除「Phase 3's virtual-scroll wrapper is the biggest risk」段（虚拟滚动方案最终未走 xterm wrapper）。`cargo check --manifest-path src-tauri/Cargo.toml` 0 错 0 警告。— `15c2226`
- 2026-05-03 — OVERVIEW.md §6 风险段同步：R1 测试数 99 → 135、breakdown 92+7 → 113+22，集成场景从 7 条扩到 22 条（新增 ?2026 toggle / ?1004 focus / REP / RIS / OSC 133/633 / OSC 事件顺序等）；R5 状态由「要消除」改「2026-05-03 决议：保留 §5.2 方案 A，深翻走 §2.1 反向 bridge」；「不可逆改动列表」改为「已实施」回顾——Pane.svelte 整文件删除、terminalRegistry.ts 整文件删除、`@xterm/*` 在 `src/` 下 0 引用。 — `2dad3dd`
- 2026-05-03 — OVERVIEW.md §6 R2/R3/R4 风险条同步：R2 由「round 2.4 接入后慢 30-50%」改为 round 7 retire xterm 后 perf 不再是阻塞项；R3「round 4 留了专项时间」改为「2026-05-02 完成」（IME v2 cursor-tracking 在 RidgePane.svelte 已实现，Pane.svelte 删除，§2.2 MutationObserver 守护可选不阻塞）；R4 修正 `pty.rs:88` → `pty.rs:95`（RESIZE_SILENCE_WINDOW_MS = 800 现行位置），「BUGFIX.md 的 B 项」错误引用改为「BUG-4（4ms 合批窗口）」+ 关联 REPLACE_AND_FIX_PLAN.md SharedArrayBuffer 路径备忘。 — `46971af`
- 2026-05-03 — OVERVIEW §3 表 row 2.4 的「`PaneRouter.svelte`」自相矛盾修正：row 7 已说「PaneRouter / terminalRegistry 全部移除」，但 row 2.4 仍写「manager.ts + RidgePane.svelte + PaneRouter.svelte」。改为「`manager.ts` + `RidgePane.svelte`（PaneRouter.svelte round 7 已删除，SplitContainer 直接 import RidgePane）」。`docs/TERMINAL_SCROLLBACK.md` 顶部加 2026-05-03 状态横幅，指出后续 Baseline 段是 round-16 历史快照（保留 verbatim 用于设计史延续），当前架构指 RidgePane / state.rs::PaneScrollback / get_pane_scrollback_tail/before 等。 — `9528b85`
- 2026-05-03 — 删除 `state::PaneScrollback::head_seq()` 死方法：注释声称「used by phase-3 scroll-to-tail logic; expose now to keep API stable」，但 Phase 3（§2.1 反向 scrollback bridge）实际走 `get_pane_scrollback_tail` 返回的 `start_seq`/`at_oldest` 字段，never 经 `head_seq`。`Grep head_seq\b|\.head_seq\(` 仅定义点，11 个 state 消费者全部不调用。删除 `#[allow(dead_code)]` + 注释 + 函数体（4 行）。`tail_seq` 仍保留为活跃 API 对偶。`cargo check --manifest-path src-tauri/Cargo.toml` 0 错 0 警告。 — `5d5a3d5`
- 2026-05-03 — CLAUDE.md「Cargo zero-warning gate」段同步：把「as of round 19」（陈旧轮次引用）换成「last verified 2026-05-03」(实跑 `cargo build --lib --manifest-path src-tauri/Cargo.toml` 验证 0 警告)；并补一条规则细则——当 `#[allow(dead_code)]` 注释引用「now-shipped phase / round / mechanism」时（比如 Phase 3 已通过另一条路径 ship），该 justification 已死，应 grep 验证后删除。这条规则正是本轮删 head_seq / get_pane_scrollback shim 的指导原则。 — `131ab2a`
- 2026-05-03 — 删除 `commands/git.rs::get_git_info` 死 Tauri command stub：`#[tauri::command]` 标注但 `lib.rs invoke_handler!` 从未注册，前端 `src/` 下 0 处 `invoke('get_git_info', ...)` 引用（Grep 确认），body 仅 `Err("Use get_git_info_with_cwd instead")` 占位。删除 doc comment + `#[allow(dead_code)]` + `#[tauri::command]` + 函数体共 7 行。`get_git_info_with_cwd` 仍为活跃路径并已注册（lib.rs:262）。`cargo check --manifest-path src-tauri/Cargo.toml` 0 错 0 警告。同样套用 CLAUDE.md 新加的规则（justification 引用过期机制时 grep + 删除）。 — `ac201ca`
- 2026-05-03 — 删除 `engine/cwd.rs` 三个未使用的字节查找辅助 `find_subsequence` / `find_byte` / `find_byte_either_with_value`：注释自称「still-tested ergonomic wrappers」但 Grep `cwd.rs` 内只有定义点，无任何调用方（包括 #[cfg(test)] 块）。三个都是 `fn`（非 `pub fn`），文件外不可见。`find_byte_either`（无 `_with_value` 后缀）仍由 `parse_cwd_from_output:180` 使用并保留。`engine/title.rs` 自带独立同名 `find_subsequence`（签名不同），不受影响。共删 28 行（含 `#[allow(dead_code)]` × 3 + 注释段）。`cargo check --manifest-path src-tauri/Cargo.toml` 0 错 0 警告。 — `f80ece3`
- 2026-05-03 — §1.13 ✅ 修复 `commands/project.rs` 9 条 + `commands/git.rs` 5 条 async 测试（共 14 条）的预存编译错误：批量 `#[test]` → `#[tokio::test]`、`fn` → `async fn`、调用末尾插 `.await`。`cargo test --manifest-path src-tauri/Cargo.toml --lib` 现在 **73 passed; 0 failed**（修复前 23 errors 完全无法编译 lib test profile）。tokio 已具 `macros + rt-multi-thread` 特性，无需改 Cargo.toml。 — `83f3367`
- 2026-05-03 — `RidgePane.svelte` svelte-check 2 警告归零：(1) 600 行 `<div role="application">` 加 `<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->`——`role="application"` + `tabindex="-1"` + 整 pane 级 wheel/key/pointer/contextmenu 监听是终端容器的有意设计（Svelte a11y 默认警告非交互元素挂事件）；(2) 674 行删除空 `.rg-pane-container { /* contain: strict; ... */ }` ruleset——`contain: strict` 已通过 line 604 `style="contain: strict;"` 内联应用，CSS 块只剩注释。注释保留在 `<style>` 顶部解释为何 `contain` 在内联而不在外联。`pnpm check` 现 **0 errors, 0 warnings**。 — `73c077d`
- 2026-05-03 — `docs/term-rebuild/PARTIAL_REDRAW_PROTOCOL.md` §2 表 + §4.6 同步：表内两行自相矛盾——`?2026` 列「⏳ TODO（round 4）」但 §4.1 同文档内已标 ✅ 2026-05-02 已交付；resize reflow 列「⏳ 远期」但 §2.3 Phase 1 本会话 ✅ 落地。表行改为引用对应小节的状态（`?2026` 指 §4.1 ✅，reflow 指 §4.6 Phase 1 ✅）。§4.6 全段重写：标题加 Phase 1 ✅ 2026-05-03 / Phase 2 ⏳ 远期，正文写 grid.rs `reflow_primary` 实施要点（stitch wrapped 链 + cursor 逻辑位置迁移 + alt 屏幕仍 truncate/pad）+ 10 条单测引用 + Phase 2 仍未做的部分（scrollback ring 同算法重排 + selection/hyperlink 锚点反推）。 — `db1a5e8`
- 2026-05-03 — 同步 `grid.rs::resize` doc-comment + `PROBLEM_COVERAGE.md` §3.b/§3.d/§5/摘要表：源码注释「Naive: truncate/pad rows + cols on both screens. Soft-wrap reflow is left for a later round」与下方 line 192-193 已实际调 `reflow_primary` 矛盾——改为「Primary screen reflows on column change (Phase 1) ... Phase 2 ... still deferred」+ alt 屏幕仍 truncate/pad 解释。`PROBLEM_COVERAGE.md`：§3.b reflow 状态 ⚠️ → ✅ Phase 1 已修；§3.d IME 状态 ⚠️ → ✅ 2026-05-02（含 v2 cursor-tracking 实施细节、composition guard、helper textarea 跟 wasm cursor 位置）；§5 Reflow 协议全段重写——删除引用旧源码注释的 stale 段落，改写为已落地算法 4 步 + 10 条单测列表 + Phase 2 仍未做范围 + alt screen 范围声明；摘要表 §3 ⚠️ → ✅、§5 ⚠️ → ✅，统计从「3 个 ✅，3 个 ⚠️」更到「5 个 ✅，1 个 ⚠️」。 — `18ea96f`
- 2026-05-03 — `PROBLEM_COVERAGE.md` §4 SIGWINCH 收尾：§4.b 由「⚠️ 取决于 round 2.4 细节 + 我会做但具体效果要等实现完成才知道」改为「✅ 2026-05-03（架构落地）」，写出实链路（`ResizeObserver` → `manager.viewportChanged` → 120ms debounce → `fitPane`：canvas surface resize + PTY-first resizeHandler 避免 PSReadLine 绝对坐标 clamp + kernel.resize 调 grid Phase 1 reflow，引用 `manager.ts:887-901` + `:903-969`）。§4 净结论 「a ✅, c ✅, b ⚠️」 → 「a/b/c 全 ✅ 架构层全覆盖」。摘要表 §4 ⚠️ → ✅，统计 → 「6 个 ✅，0 个 ⚠️」。结论段（"如果你只能记住一件事"）改写为现状描述（6 症状全 ✅、Phase 2 scrollback reflow 仍 ⏳ 远期）。 — `c8465f3`
- 2026-05-03 — `BUGFIX.md` 顶部加 2026-05-03 状态横幅：BUG-1 / 2 / 5 / 6 全部 file=`Pane.svelte`，文件已删除（round 7），patch moot（RidgePane.svelte 是从零写、不继承相同 code pattern；具体行为问题需独立审计：listener 重订由 ptyBridge.ts 避免、polling 由 manager 节流、scrollback 走 §5.2 方案 A）。BUG-3（`engine/pty.rs::rt.block_on` 在 reader 线程，4 处 line 193/218/323/339）+ BUG-4（`lib.rs` 4ms 合批）仍现行可 cherry-pick。文档 body（具体 diff/patch 文本）保持 verbatim 用于复现。 — `56ad115`
- 2026-05-03 — `REPLACE_AND_FIX_PLAN.md` 顶部加 2026-05-03 状态横幅 + §2 对照表的「原行 / 现状」映射：8-10 周时间线已走完。痛点 1 (BUG-4 未 patch)、痛点 2 (✅)、痛点 3 (共享 surface 推到 round 3)、痛点 4 (§5.2 方案 A 保留双 buffer)、痛点 5 (✅ 2026-05-02)、BUG-1/2/5/6 moot、BUG-3/4 仍现行。文档体（决策树 / 时间线 / §5 取舍 / §6 推荐执行计划）保持 verbatim 用于设计史延续。 — `a2f3a2d`
- 2026-05-03 — `INTEGRATION.md` + `INTEGRATION_R2_4.md` 顶部加 2026-05-03 状态横幅。INTEGRATION.md（接入契约）原诚实声明说「round 2.2/2.3/2.4 还没实现」+ ⚠️ 标记，加复核说明：所有 ⚠️ 已交付（`createTerminalManager` / `feed` / `onData` / `resize` / `encodeKey` / `render` 在 manager.ts + lib.rs wasm-bindgen 实现，`Pane.svelte` 重写后 round 7 整体删除，被 `RidgePane.svelte` 取代）。INTEGRATION_R2_4.md（round 2.4 步骤）状态横幅说 round 7 已 retire xterm，「已知不工作」清单（鼠标拖选/IME/Ctrl+F/Ctrl+click）2026-05-02 patch 已交付，回滚指令 `rm RidgePane.svelte` 不再适用。两文档体保留 verbatim 用于设计契约 / 设计史延续。 — `647f636`
- 2026-05-03 — §0 进度快照「round 4 部分提前」row 措辞收尾：原文「与 INTEGRATION_R2_4.md 中"已知不工作"清单背离——实际代码已完成」（描述早期 doc 与代码的不一致状态）。现在 INTEGRATION_R2_4.md 顶部 banner 已对齐该清单，row 改为「✅ 2026-05-02 | INTEGRATION_R2_4.md 顶部已加 2026-05-03 状态横幅同步该清单」——日期标注 + 指向 banner，去掉"背离"措辞（不再是事实）。本会话两次 gate sanity check：`cargo check --manifest-path src-tauri/Cargo.toml` 0 错 0 警告；`pnpm check` 0 错 0 警告。`cargo test --lib` 73 passed（修复 §1.13 后稳定）。 — `3af216f`
- 2026-05-03 — 修复 §1.13 任务条目位置错误：当时添加新任务时误插到「进度记录」chronological log section 中间（line 429，处于 2026-05-02 协议补全 patch bullet 之前），破坏了 log 的时间线 + 任务结构分层。现把 §1.13 任务条目（heading + 3 个 bullet）从 log 段移到 §1 主体末尾（§1.8 ✅ 2026-05-03 之后、§2 章前的 `---` 分隔之前），与 §1.1-§1.12 同列。Log 段恢复纯时间线流向。 — `29e164c`
- 2026-05-03 — `OVERVIEW.md` 三处 Phase 1 stale 引用同步：(1) §3 进度表 row 112 的「与 INTEGRATION_R2_4.md ... 已知不工作 ... 清单背离」改为「INTEGRATION_R2_4.md 顶部已加 2026-05-03 状态横幅同步该清单」（与 TASKS §0 row 22 用同一措辞）；(2) §5 已知未实现表 row 180 的「Resize reflow — Phase 1 | round 4 收尾」改为「✅ 2026-05-03 | grid.rs::reflow_primary 已实现，10 条单测全绿」；(3) §7.3 分阶段交付段「**Phase 1（round 4 收尾）**」改为「**Phase 1 ✅ 2026-05-03**」。三处都是 Phase 1 落地后留下的过时 future-tense。 — `3769aa3`
- 2026-05-03 — 全文档 grep `round 4 收尾|Phase 1.*round 4|Phase 1.*未实现` 收尾扫描：仅余 2 处合理命中——`PARTIAL_REDRAW_PROTOCOL.md:71` 行的「Phase 1 ... 未实现」是 §2 表「现象 / 协议根因 / 状态」三列约定中的"原始根因"列描述（与同表 ECH/ICH/DCH/REP 等其他行的「未实现」并列，是历史根因不是当前状态）；TASKS.md 内同字符串只在自我引用的进度日志条目中出现（quote 旧措辞为修复对照）。不构成 stale 引用。 — `9ff1ef2`
- 2026-05-03 — 新增 §1.14 跟踪条目「PaneState::Starting 后端 / 前端 半实现 gap」。本会话审计时发现：enum 定义（`state.rs:73-80`）+ 序列化 match（`commands/pane.rs:60-64`）+ TS union（`types.ts:9`）+ UI affordance（`SplitContainer.svelte:592-599` 琥珀色 STARTING badge）四件齐全，但 `teammate/server.rs:293-298 register_agent_to_pane` 直接 Idle→Busy，永不经过 Starting。Grep 全工程 `PaneState::Starting` 唯一构造点 = enum 定义本身。半实现：UI 永不渲染。设计意图清楚（agent register 已发但 PTY 还没收到首条 prompt 输出时用 Starting），但实施需用户对 teammate 流程时序判断（影响 Claude Code 集成的 Idle→Busy 当前假设）。本 audit 仅登记，不动代码。 — `3fe5b39`
- 2026-05-03 — §0 进度快照「剩余主线」清单加第 4 项指向 §1.14——把上一轮新登记的 PaneState::Starting gap 纳入快照可见的 deferred / blocked-on-user 列表（与 §1.5 / §2.2 / §2.4 / §3.3 等其他 LOW / 远期项并列）。这样未来 reader 看快照即知所有未完成项目，无需深扫描全文。 — `8e8bc23`
- 2026-05-03 — 给 `state.rs:79` `Starting` 变体的 `#[allow(dead_code)]` 加内联 justification 注释——指向 TASKS §1.14。原本只有裸 attribute，按本会话加到 CLAUDE.md 的 dead_code 政策（justification 引用过期机制时 grep + 删除）将来某次 cleanup 可能错把变体当死代码删掉，损伤前端 UI affordance + 序列化 match 的 audit trail 锚点。注释列出全部 4 个相关位置（enum + commands/pane.rs:60-64 + TS union + SplitContainer.svelte:592-599）+ 唯一 gap（teammate/server.rs Idle→Busy 直跳）+ 指向 TASKS §1.14。`cargo check` 维持 0 错 0 警告。

- 2026-05-02 — 一系列协议补全 patch：ECH/ICH/DCH/REP/DECSCUSR/DSR/DA/?2026/?1004/OSC0/1/2/7/8、鼠标拖选（含 word/line/shift-click）、Ctrl+F 搜索、IME v2 cursor-tracking、Ctrl+click OSC 8 链接 — 详见 git log

- 2026-05-05 — 多轮迭代综合检查点（uncommitted in working tree）：
  - **§1.21 三层修复 + 真根因**：(1) `RidgePane.svelte` OSC 标题 store identity-preserving guard；(2) `Explorer.svelte` 主 cwd-effect 删除 `$terminalTitles` 依赖 + 新增独立 title-only effect 调 `fileExplorer.ts::updatePaneTitles`（identity-preserving column reuse）；(3) **真根因**：发现第二个 `$effect` (line 60, `initFileExplorer`) 也依赖 `$terminalTitles`，每个 OSC 标题 emit 都会 forward 到 `syncAllWorkspaces` → `syncWithPaneCwds` 重建 columns 数组——删除其 title 依赖；(4) `paneTree.ts::normalizeCwd` 加 trailing-slash 修剪（除 root），防 OSC 7 with/without 斜杠不一致击穿 setPaneCwd identity guard；(5) `fileExplorer.ts::syncWithPaneCwds` 删除 `hasNewJoiner → needsRefresh` 触发——pane 加入 cached column 仅 label 切换不重载，符合用户「切到已有文件树只切 label 不刷新」语义；E6 测试更新到新策略（27/27 通过）。
  - **§1.20 选区 abs-row 重构**：`selection.rs` `Pos { row, col }` viewport-relative → `RangeAbs { start_abs_row, start_col, end_abs_row, end_col }`，`Selection::set` 在创建时捕获 scroll context，`range_in_viewport(&Terminal)` 每帧翻译到 viewport 坐标。新增 3 单测：`selection_survives_scroll_into_scrollback` / `range_in_viewport_translates_with_scroll` / `empty_terminal_select_all_is_safe`。
  - **§7.2 WebGPU 三大根因 + 默认化**：(a) `glyph_rasterizer.rs` 加 `dpr: f32` 参数，按 `font_size_px * dpr` 渲染解决「字体太小太细」+ 切换 `text_baseline` 从 "top" 到 "alphabetic" + 显式 `fill_text(0, ascent_dev)` 解决顶部缺失；(b) `webgpu.rs::draw_row` UV crop 从 `[0,0,1,1]` 改成 `[0, 0, glyph.width/slot_w, glyph.height/slot_h]` 让 cell quad 只采样 bbox 区域；(c) `RenderBackend::requires_full_frame()` default-method（false）+ WebGpuBackend override true + `Renderer::tick` 调用之 + `AnyBackend::requires_full_frame()` forward——修复 `LoadOp::Clear` 每帧抹屏导致非 dirty 行内容消失；(d) Cargo.toml `default = ["webgpu"]` + `build.mjs` 用 `--no-webgpu` 替换 `--webgpu`（legacy no-op）+ `manager.ts::instance` `preferWebgpu` 默认 true（`localStorage.RIDGE_WEBGPU = '0'` 才禁），双层运行时回退（JS typeof 检测 + Rust request_adapter 失败 catch → Canvas2D）；(e) `ATLAS_SLOT_W/H` 32×32 → 64×96 覆盖 18 CSS px × DPR 2.5 + CJK 宽字符；(f) atlas sampler `Linear` → `Nearest` 防 UV-edge 半透明 blur；(g) WebGPU draw_row/cursor/selection/hyperlink 全部整数对齐（`floor((col*cell_w))`）防 fractional-pixel 字符画细线；(h) OVERVIEW.md / CLAUDE.md 文档同步。
  - **§1.22 alt-screen 清空 on resize**：`grid.rs::resize` 检测「dim_changed && is_alt」时清空 alt buffer + cursor home + scroll 复位，让 Claude Code / lazygit / Ink-based partial-diff 重绘落到干净画布，避免错位行/字符。新增 `resize_on_alt_screen_clears_alt_buffer` + `resize_on_primary_does_not_clear_primary` 单测；旧 `reflow_skips_alt_screen` 重命名 + 期望反转。
  - **§1.23 缺失 UI 复刻**：(1) `RidgePane.svelte::onContextMenu` 加「向右拆分 / 向下拆分 / 关闭面板」3 项调 `splitPane(paneId, 'vertical'/'horizontal')` 和 `closePane(paneId)`；(2) 浮动滚动到底部按钮（仅 `scrollState.offset > 0` 时显示）；(3) 侧边可拖拽滚动条（10px 轨道 hover 显形 + 缩略图按 `rows / (rows + scrollback_total)` 比例 + 拖动 setPointerCapture + 250ms poll 同步 PTY 异步增长）。ARIA scrollbar role / aria-valuemin/max/now。
  - **§4.3 共享 surface 详细设计**：本节从 4 行高级意图扩展到 ~120 行可执行设计——架构图（SharedSurface JS 单例 + SharedSurfaceBackend Rust 单 device/queue/atlas）、坐标转换公式、atlas 迁移路径、渲染循环伪代码、resize 二分类、attach/detach 生命周期、RidgePane.svelte 影响、Phase A→D 分阶段交付（Phase A ✅ 已完成，Phase B 起步点已定）、风险登记 R1-R4 + mitigations、测试矩阵。
  - **整体验证**：`cargo test --lib` 242/242（之前 240，+2 新增 §1.22 测试）；`cargo check --target wasm32-unknown-unknown --lib` 默认 + `--no-default-features` 双模式 0 警告；`pnpm check` 0 errors / 0 warnings (4098 files)；`pnpm test fileExplorer.test.ts` 27/27；wasm pkg 重打。
  - **未提交**：当前所有改动仍在 working tree，等用户实跑回归 §1.20/§1.21/§7.2/§1.22/§1.23 后再决定一次性 checkpoint commit 或 cherry-pick 拆分。`§4.3 Phase B（atlas-to-shared in single pane）` 待用户给绿灯。
  — uncommitted

---

## 文档导航

- 本文档：`TASKS.md` — 任务跟踪与进度记录
- 总览与设计：[`OVERVIEW.md`](OVERVIEW.md)
- 接入步骤（round 2.4）：[`INTEGRATION_R2_4.md`](INTEGRATION_R2_4.md)
- 通用接入：[`INTEGRATION.md`](INTEGRATION.md)
- 局部刷新协议：[`PARTIAL_REDRAW_PROTOCOL.md`](PARTIAL_REDRAW_PROTOCOL.md)
- 替换+修复总计划：[`REPLACE_AND_FIX_PLAN.md`](REPLACE_AND_FIX_PLAN.md)
- 痛点覆盖：[`PROBLEM_COVERAGE.md`](PROBLEM_COVERAGE.md)
- xterm 时代 bug 库：[`BUGFIX.md`](BUGFIX.md)
