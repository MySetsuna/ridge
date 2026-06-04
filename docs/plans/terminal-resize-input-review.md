# 终端 自适应全屏 (Resize/Fit) 与 鼠标/键盘控制 (Input) 审查报告

> 范围: FIT/RESIZE 接线 + INPUT 转发。**不触碰** wasm-kernel / worker-renderer 内部 (P4 worktree `p4-ipc-render-decouple` 正在改动渲染器内部)。
> 审查文件: `src/lib/terminal/manager.ts`, `src/lib/components/RidgePane.svelte`, `src/routes/+page.svelte`, `src-tauri/src/commands/terminal.rs`。
> 日期: 2026-06-04。

---

## 一、Resize/Fit 接线 — 结论: 接线基本正确

### 完整链路 (window resize 与 split 两条路径)

1. **每个 pane 容器** 在 `attach()`/`unpark()` 装 `ResizeObserver` →
   `manager.ts:1573 / 2222` → `viewportChanged(paneId)` → 500ms trailing-edge
   debounce → `fitPane(entry)`。
   - window resize 时容器作为 flex/grid 子节点尺寸改变 → 该 observer **会**触发，
     因此 window resize 与 split 走的是**同一条** grid-reflow 路径。`viewportChanged`
     不区分两者,无 "只在 split 生效" 的漏洞。
2. **全局 host canvas** 在 `+page.svelte:207 globalHostCanvas` action 单独装
   `ResizeObserver(parent)` → `manager.resizeHost(dims)` (`manager.ts:779`) →
   重配 swap chain + 遍历所有 pane 调 `_recomputeViewport` (同步更新 scissor) +
   `_invalidateHost()` + `wake()`。**无 debounce**,跟随 window 实时。
3. `fitPane` (`manager.ts:3872`) host 分支调 `_recomputeViewport` 设新 scissor,
   并 `await entry.resizeHandler?.(rows,cols,isAlt,isInlineTui)` →
   `onPtyResize` (`RidgePane.svelte:789`) → `invoke('resize_pane', …)`。
4. 后端 `resize_pane_inner` (`terminal.rs:1016`): `master.resize` (SIGWINCH) +
   `parser.resize(rows,cols)` 发 Resize delta 帧;前端经 `apply_delta(Resize)`
   异步更新 kernel grid。这是 P4 之后 kernel grid 唯一更新通道 —
   `fitPane` 不再直接 `kernel.resize`。

### 自愈 (self-heal) 覆盖 window resize 与 shrink-then-grow — 已验证

- `fitPane` 的 `sizeChanged` 判定 (`manager.ts:3983`) 不仅比较
  `lastReportedRows/Cols`,还比较 `kernel.rows()/cols()`:
  ```
  rows !== lastReportedRows || cols !== lastReportedCols
    || rows !== kernelRows || cols !== kernelCols
  ```
  → 即便上一次 fit 在 pty-delta Channel 注册前跑过(Resize delta 被丢、kernel
  停在 80×24),下一次 fit 仍会因 `rows !== kernelRows` 重新下发。该自愈对**任何**
  fit 调用生效(`fitPane` 是公共路径),不限于 split,所以 window resize 同样被覆盖。
- `unpark` (`manager.ts:2208-2209`) 把 `lastReportedRows/Cols` 重置为 -1,强制
  下次 fit 重发 → split/reparent 后尺寸必同步。
- shrink-then-grow: shrink 与 grow 各产生一次容器尺寸变化 → 各触发一次
  `fitPane`;每次 `_recomputeViewport` 重设 scissor、`invalidateAll` 清行哈希、
  同步渲染一帧 + 150ms forceFullRedraw。无 "卡在旧 grid" 的接线缺口。

### 与 P4 prior-art "split kernel race" 的关系

`RidgePane.svelte:1021 enableDeltaModeThenFit(...→ fitPaneNow)` 确保 fit 在
`ensurePtyBridge` (Channel 注册) + `setPaneDeltaMode` 之后才跑(onMount 步骤 5b→7),
race 已被关闭;`fitPane` 的 kernel-grid 自愈是第二道保险。两者皆在位。

---

## 二、GM live 证据 (stray "opencode" glyph) — 判定: 渲染器内部 (P4),不在本次范围

**证据**: `docs/plans/test-desktop-08-resize-small.png` 显示——窗口从
3440×1392 缩小到 1100×700 后,**Monaco 文件编辑器**(显示 `package.json`,右上
`json 已保存`)区域中部漂着一个孤立的 "opencode" 字形(带下划线残迹)。
`test-desktop-09-resize-restore.png` 显示窗口还原后该字形**消失**,编辑器干净。

**分析**:
- 全局 WebGPU host canvas 位于整个 workspace DOM **之后**(pane 容器
  `background: transparent` 让其透出,见 `manager.ts:1061`)。该 "opencode" 是
  某个终端 pane 的残留像素从透明的编辑器区域透了出来。
- fit 接线侧已正确处理: `resizeHost` 在 window resize 当帧**同步**调
  `_recomputeViewport` 收紧 scissor。问题在于 host 帧的 `LoadOp::Clear` + per-pane
  scissor 重绘**没有清掉**那块孤儿区域 —— 极可能是 `ensureHostFrame` 的惰性开帧
  逻辑(`manager.ts:4328`,无 pane dirty 时不 beginFrame,故不发 Clear)在缩小瞬间
  让旧 swap-chain 像素残留透出。还原放大后整屏重绘即清除,符合 "瞬态陈旧像素" 特征。
- 该清屏/scissor/swap-chain 时序属于 **SurfaceHost / WebGPU 渲染器内部**,正是 P4
  正在改的部分。**不应**在本次(fit 接线)修改。

> **待 P4 处理**: host 帧在可见 pane 几何收缩(scissor 变小)时,即使无 pane dirty
> 也需开一帧发 `LoadOp::Clear`(或对 "上一帧 scissor ⊃ 本帧 scissor" 的差集区域强制
> 重绘),以清除孤儿终端像素。建议把 `resizeHost`/`_recomputeViewport` 里 scissor
> 收缩的事件与一次强制 host clear 关联(P4 渲染循环内实现)。

---

## 三、Input 转发审查

### 键盘 (manager.ts:2641 handleKeyDown) — 正确

- `keydown` 监听挂在容器 (`RidgePane.svelte:1597 onkeydown`),IME helper textarea
  是其子节点,事件冒泡到容器 → 焦点接线无 "键到不了 PTY" 缺口。
- `onContainerPointerDown` (`RidgePane.svelte:1540`) 把焦点交给 IME helper(或 direct
  模式下容器),`activePaneId.set` + `touchTuiSticky`;`onContainerMouseDown`
  `preventDefault` 防焦点被抢回容器。焦点链完整。
- 特殊键/修饰键: `encodeKey(ev.key, ctrl, alt, shift, meta)` 交由 kernel 编码
  (DECCKM 等),Mac 下 Cmd→Ctrl 映射 (`manager.ts:2646`)。host 优先快捷键
  (Ctrl+Shift+V/C、Win Ctrl+V、Mac Cmd+V、F11、Ctrl+,) 表完整且 TUI-gate 避让
  正确 (`handleHostPriorityShortcut`)。
- IME: compositionstart/update/end 全链 + wasm preedit overlay 锚点单一来源
  (`inputAnchorResolved`),alt-screen 锁定 / shell 模式跟随。无明显缺口。

### 鼠标上报 (SGR/1006) — 正确

- pointerdown/move/up + wheel 全部经 `kernel.encodeMouse(row,col,btn,action,
  shift,ctrl,alt)` 转 SGR 字节 → `dataHandler` → PTY。
- `mouseReportingModes() !== 0` 时鼠标绝对优先 (`manager.ts:1342`),release 单独补发
  (btn=3) 防 TUI 卡按下态 (`manager.ts:1535`);?1002 用 `PointerEvent.buttons` 而非
  host `selecting` 标志,避免中途切换鼠标模式时 host 选区残留泄漏。
- pointermove 经 rAF 批处理 + (row,col,buttons,action) 去重;pointerdown 取消挂起的
  move 防 "选区从光标上一刻位置开始"。
- wheel→SGR (button 64/65) 减去 padding 后算 row/col (`manager.ts:2707`),与 click
  一致。
- 右键菜单在 `isMouseReporting` 时让位给 TUI (`RidgePane.svelte:1308`)。

---

## 四、发现 (Findings)

| # | 严重度 | 位置 | 根因 | 处理 |
|---|--------|------|------|------|
| F1 | LOW | `manager.ts:3782` (原) | `viewportChanged` 文档注释写 "1000 ms",实际 `RESIZE_SETTLE_MS = 500` (`manager.ts:380`)。文档漂移,误导后续调试本次正在追的 resize 节奏问题。 | **已修** — 注释改为 500 ms。 |
| F2 | MEDIUM | `RidgePane.svelte onContainerWheel` + `manager.ts handleWheel` | **滚轮在 "alt-screen 但未开鼠标上报" 的 TUI 上失效**。`handleWheel` 因无 mouse reporting 返回 false;alt-screen 无 host scrollback (`total===0`),滚轮变死键。`less`/`man`/`git log`/`fzf`/`claude /theme` 菜单等只读箭头键的全屏程序无法滚轮滚动。这是 xterm/Windows Terminal/iTerm2/kitty 默认开启的 `alternateScroll`。 | **已修** — 新增 `manager.wheelAltScroll(paneId, ev)`,在 alt-screen 且鼠标上报关闭时把滚轮翻译成 ArrowUp/Down 按键(经 `kernel.encodeKey` 走 DECCKM-aware 编码,无二次编码漂移),每 ~30px 一次按键、单事件上限 5 次。仅用既有 kernel 导出,**未触碰渲染器**。在 `onContainerWheel` 的 TUI-mouse 分支之后、host-scrollback 分支之前接入。 |
| F3 | INFO | `+page.svelte:207` host-canvas observer vs `manager.ts:779 resizeHost` | window resize 时 host scissor 实时跟随,但 **grid reflow 只走每-pane 容器 observer 的 500ms debounce**;OS 窗口边缘拖拽的 `pointerup` 通常落在 webview 之外的窗口铬上,`_ensureResizeReleaseListener` 的 document `pointerup` 不一定触发 → grid 在用户停手后约 500ms 才重排。**这是刻意设计**(见 `viewportChanged` 大段注释:拖拽中不实时跟随,避免错位/不完整重绘),非 bug。若 GM 认为 500ms 延迟对 "自适应全屏" 体感偏慢,可考虑给 window-resize 专门补一个 settle 触发,但需 live 复核体感后再定,本次不改。 | 不改,记录。 |
| F4 | INFO | `docs/plans/test-desktop-08-resize-small.png` | stray "opencode" glyph(见第二节)。 | **待 GM live 确认 + 转 P4**。属渲染器 host-clear/scissor/swap-chain 时序,本次范围外,不强改。 |

---

## 五、验证结果

- `pnpm check` (svelte-check): 我改动的 `manager.ts` 与 `RidgePane.svelte`
  **0 error / 0 warning**。输出仅有 2 个错误,均来自**未跟踪、与本任务无关**的并行
  FileEditor 工作 (`tests/e2e/fileEditor.spec.ts` 的模块解析,文件为 `??` 未入库)。
- `pnpm exec vitest run terminal`: **160 passed / 5 failed**。5 个失败全部是
  **已知的 P4 worker-rendering WIP 测试**(`renderWorker.test.ts`、
  `workerRendererBridge.test.ts`、`workerRendererSingleton.test.ts`,均断言
  `isWorkerRenderingEnabled()/isActive()` 默认为 true,而 P4 WIP 故意返回 false)。
  按任务要求**不**通过翻默认值去 "修" 它们,且不计为本次回归。新增的 `wheelAltScroll`
  未引入任何新失败。
- 未改动 `src-tauri`,故无需 `cargo check`。

## 六、改动文件

- `src/lib/terminal/manager.ts` — F1 注释修正;F2 新增 `wheelAltScroll` 方法。
- `src/lib/components/RidgePane.svelte` — F2 在 `onContainerWheel` 接入 `wheelAltScroll`。
