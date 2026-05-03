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

### 4.1 `WebGpuBackend` 骨架 ⏳

- **文件**：新增 `src/render/webgpu.rs`，实现 `RenderBackend` trait（`backend.rs` 已经定义好接口）
- **关键 API**：
  - `configure(font, size, dpr) -> (cellW, cellH)`
  - `render(rows: &[RowDraw], cursor: CursorDraw, frame: FrameMetrics)`
  - `apply_theme(theme)`
- **注意**：**`RenderHandle` 当前硬编码 `Canvas2dBackend`**（`src/lib.rs`），需要改成运行时选择或 wasm-bindgen 从 JS 传入选择标志。

### 4.2 `GlyphAtlas` 数据结构 ⏳

- **文件**：新增 `src/render/glyph_atlas.rs`
- **设计要点**：
  - key = `(font_family_hash, font_size, glyph_id)`
  - 值 = `(texture_layer, uv_rect, advance, ascent_offset)`
  - LRU 淘汰 + 容量上限（避免 4K 字符 × 多字号 OOM）
  - 字形栅格化用 `cosmic-text` 或 `fontdue`（前者支持 fallback chain）
- **解耦**：atlas 与 `WebGpuBackend` 解耦，后续 Canvas2D 也可以读取（虽然现 Canvas2D 用浏览器原生 fillText，不走 atlas）

### 4.3 共享 surface（OVERVIEW D1）⏳

- **背景**：当前 round 2.4 是每 pane 一个 `<canvas>`。OVERVIEW 设计是全局一个 canvas + scissor rect。
- **依赖**：4.1 落地后才有意义（Canvas2D 不便于 scissor 划分）。
- **影响范围**：`manager.ts` 的 attach/detach 逻辑要重写——所有 PaneEntry 共享同一个 canvas，render 循环改为按可见 viewport 分别 scissor。
- **预期收益**：10 pane 时 1 个 GL ctx + 1 份 atlas，比现在 10 个 Canvas2D context 节省 ~80% GPU 内存。

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
  - [ ] prompt 显示
  - [ ] 输入命令 + 回车有输出
  - [ ] Ctrl+C 终止 sleep
  - [ ] `ls --color` 看到颜色
  - [ ] 拖 splitpanes 边界跟随
  - [ ] `seq 200` 滚轮看历史
  - [ ] Shift+PageUp / Shift+PageDown 翻页
  - [ ] 选段 → 右键复制
- **附加验证项**：
  - [ ] vim/less 退出后主屏内容恢复（alt screen ?1049）
  - [ ] 输入中文（IME 候选窗位置正确）
  - [ ] Ctrl+F 搜索 + n/N 切匹配
  - [ ] Ctrl+click OSC 8 链接打开

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
- 2026-05-03 — §0 进度快照「round 4 部分提前」row 措辞收尾：原文「与 INTEGRATION_R2_4.md 中"已知不工作"清单背离——实际代码已完成」（描述早期 doc 与代码的不一致状态）。现在 INTEGRATION_R2_4.md 顶部 banner 已对齐该清单，row 改为「✅ 2026-05-02 | INTEGRATION_R2_4.md 顶部已加 2026-05-03 状态横幅同步该清单」——日期标注 + 指向 banner，去掉"背离"措辞（不再是事实）。本会话两次 gate sanity check：`cargo check --manifest-path src-tauri/Cargo.toml` 0 错 0 警告；`pnpm check` 0 错 0 警告。`cargo test --lib` 73 passed（修复 §1.13 后稳定）。

### 1.13 [LOW] `cargo test --lib` 中 `commands/project.rs` + `git.rs` async 测试预存编译错误 ✅ 2026-05-03

- **背景**：本会话审计 dead code 时，`cargo test --manifest-path src-tauri/Cargo.toml --lib` 报 23 个错误。`commands/project.rs::{delete_path, copy_path, move_path}` 9 条测试 + `commands/git.rs::find_git_repos_below` 5 条测试（共 14 条 async 函数测试）走 `#[test]` + 同步 `.unwrap()` 语义，但函数已是 `pub async fn`。
- **修法（已实施）**：每条测试 `#[test]` → `#[tokio::test]`、`fn name(` → `async fn name(`、production-call 后插 `.await`。tokio 已配 `macros + rt-multi-thread`（Cargo.toml 验证），无需改 dep。
- **结果**：`cargo test --manifest-path src-tauri/Cargo.toml --lib` **73 passed; 0 failed; 0 ignored**，含 9 条 project + 5 条 git 修复后的测试全部通过。`cargo check --lib`（生产 profile）维持 0 警告。
- 2026-05-02 — 一系列协议补全 patch：ECH/ICH/DCH/REP/DECSCUSR/DSR/DA/?2026/?1004/OSC0/1/2/7/8、鼠标拖选（含 word/line/shift-click）、Ctrl+F 搜索、IME v2 cursor-tracking、Ctrl+click OSC 8 链接 — 详见 git log

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
