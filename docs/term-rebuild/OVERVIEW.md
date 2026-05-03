# Ridge Term — 项目总览

> 自研 Rust + WASM 终端模拟器，用于替换 ridge 项目当前的 xterm.js + WebGL addon。
> 本文档描述**最终交付形态**、设计取舍、当前进度、剩余里程碑、已知风险。

---

## 1. 为什么自研

xterm.js + addon-webgl 在 ridge 的实际使用中暴露了 5 类痛点（详见 `BUGFIX.md` 的诊断章节）：

1. 输入响应慢
2. 渲染抖动 / resize 抽搐
3. 多 pane（10+）时 UI 假死
4. 内存膨胀（每 xterm 实例 ~5MB + 4MB 后端 scrollback）
5. 选择 / 复制 / 搜索体验差（跨软换行断裂等）

经过分析，痛点 3/4 的根因是 **每个 pane 各自持有一个 WebGL context + 字形 atlas + VT 解析器**，这是 xterm 架构层面的限制，无法通过调参解决。

自研的核心架构赢面：**所有 pane 共享一个 GPU surface 和一个全局字形 atlas**，把 N×资源 压成 1×。这条路 xterm 不可能走（它的 IRenderer 接口是按实例设计的）。

---

## 2. 最终架构

```
┌────────────────────────────── 浏览器/WebView2 进程 ──────────────────────────────┐
│                                                                                   │
│  ┌─ JS 侧 (Svelte) ──────────────────────────────────────────────────────────┐  │
│  │                                                                             │  │
│  │  Pane.svelte (×N)        ─┐                                                │  │
│  │   - 仅持有 paneId + 容器     │  逻辑挂在全局 ridgeTerm 单例                   │  │
│  │   - 不再持有 Terminal 实例   ▼                                                │  │
│  │                          ┌─────────────────────────────┐                    │  │
│  │   ridgeTerm (全局单例)   │  TerminalManager (TS)         │                  │  │
│  │   ┌────────────────────┐│   - paneId → grid 映射         │                  │  │
│  │   │ 1 × <canvas>       ││   - 按需 mount/unmount         │                  │  │
│  │   │   全屏 / overlay   ││   - 每帧只重绘活跃可见 pane    │                  │  │
│  │   └────────────────────┘└──────────────┬──────────────┘                    │  │
│  │                                         │ wasm-bindgen FFI                  │  │
│  └─────────────────────────────────────────┼──────────────────────────────────┘  │
│                                            │                                       │
│  ┌─ Rust → WASM (ridge-term crate) ────────▼──────────────────────────────────┐  │
│  │                                                                              │  │
│  │  TerminalRegistry  ── PaneId → Terminal kernel                               │  │
│  │       │                                                                      │  │
│  │       └─► Terminal (per pane)                                                │  │
│  │             ├─ vte::Parser (Paul Williams ANSI 状态机)                       │  │
│  │             ├─ Grid: 主屏 + alt 屏 + cursor + scroll region (DECSTBM)        │  │
│  │             ├─ Modes: DECAWM/DECTCEM/bracketed paste/mouse 等                │  │
│  │             ├─ AttrTable: SGR 属性 flyweight (250k cell → ~3MB)              │  │
│  │             └─ Scrollback: ring buffer，分配回收                             │  │
│  │                                                                              │  │
│  │  Renderer (全局唯一)                                                          │  │
│  │       ├─ trait RenderBackend                                                 │  │
│  │       ├─ WebGpuBackend  (优先)                                                │  │
│  │       └─ Canvas2dBackend (fallback)                                           │  │
│  │                                                                              │  │
│  │  GlyphAtlas (全局共享)  ── (font, size, glyph) → texture region             │  │
│  │  InputEncoder           ── 键盘事件 → PTY 字节 (受 modes 影响)                │  │
│  └──────────────────────────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────────────────────┘
                                           │
                                           │  Tauri IPC (现有)
                                           ▼
                            ┌───────── Tauri 主进程 ──────────┐
                            │  pty.rs / commands/terminal.rs    │
                            │  (本项目不动后端，IPC 接口保持)   │
                            └───────────────────────────────────┘
```

### 关键设计决策

**D1: 共享 surface、共享 atlas**
- 一个 `<canvas>` 元素覆盖所有 pane 区域，按 scissor rectangle 分区
- 同字号的字形位图全 pane 共用一份
- 收益：10 pane 时 GPU context 1 个（旧方案 10 个）、atlas 1 份（旧方案 10 份）

**D2: VT 内核与渲染器解耦**
- 内核只产 grid 数据 + 脏区标记
- 渲染器读 grid，按帧重绘
- 替换渲染器后端不需要改内核（先 Canvas2D 验证正确性，再换 WebGPU 优化性能）

**D3: PTY 字节流不变**
- 后端 Tauri 的 pty.rs 完全不动
- WASM 内核接收 `Uint8Array` 走 `feed()`，行为对齐 xterm 的 `term.write(bytes)`
- 后端 emit 的 `pty-output-{ws}-{pane}` 事件继续用

**D4: 不复刻 xterm 全部 API，只覆盖 ridge 用到的**
- 见 `INTEGRATION.md` 的"API 表面对照"章节
- 砍掉的：插件系统、IDecorationsService、IBufferLine 公开 API、CanvasAddon
- 保留语义但 API 形状不同的：选择、搜索（重新设计为字符流而非 cell 流）

**D5: 增量迁移**
- 内核 + xterm 并存验证 → 用户级实验开关 → 全量替换 → 删 xterm
- 任何阶段可以回退到 xterm，不锁死

---

## 3. 当前进度（按 round 计）

> 上次更新：2026-05-03（核对 patch 2026-05-02 后实际落地状态）。可执行任务列表请见 [`TASKS.md`](TASKS.md)。

| Round | 范围 | 状态 |
|---|---|---|
| 1 | VT 内核骨架（vte 接线、grid、cursor、scrollback、SGR） | ✅ 27 测试通过 |
| 2.1 | wcwidth 完整表 + alt screen + DECSTBM + DEC modes | ✅ 28 测试通过 |
| 2.2 | 渲染抽象 trait + Canvas2D 后端 | ✅ `src/render/{backend.rs, canvas2d.rs, renderer.rs}` 全部落地 |
| 2.3 | JS 表面 API（write/onData/resize/key encoder/render call） | ✅ `src/lib.rs` `TerminalKernel`/`RenderHandle` wasm-bindgen 导出，含 `pending_response`/`pending_events` 通道 |
| 2.4 | TerminalManager (TS) + 共享 canvas 单例 + Pane.svelte 替换 | ✅ `manager.ts` + `RidgePane.svelte`（`PaneRouter.svelte` round 7 已删除，`SplitContainer.svelte` 直接 `import RidgePane`）；**注**：「共享 canvas 单例」推迟到 round 3 一并做，当前每 pane 一个 `<canvas>` |
| — | 协议补全 patch（OVERVIEW §4 列表） | ✅ 2026-05-02：ECH/ICH/DCH/REP/DECSCUSR/DSR/DA/?2026/?1004/OSC0/1/2/7/8；113 单测 + 22 集成测试（含本会话新增） |
| — | round 4 部分提前 | ✅ 2026-05-02：鼠标拖选（含 word/line/shift-click）、Ctrl+F 搜索（含 scrollback）、IME v2 cursor-tracking、Ctrl+click 链接；与 INTEGRATION_R2_4.md 中"已知不工作"清单背离 |
| 3 | WebGPU 后端 + 字形 atlas（替换 Canvas2D）+ 共享 surface | ⏳ 未开始 |
| 4 | IME v3 + scrollback bridge + reflow + 链接 affordance 收尾 | ⏳ 部分完成（反向 scrollback bridge ✅ `99ad061`、reflow Phase 1 ✅ 用户、mouse / IME v2 / 搜索 / Ctrl+click ✅；reflow Phase 2 / IME v3 守护 / grapheme 远期） |
| 5 | OSC UI 接入收尾 | ✅ 实质完成（§3.1 标题渲染验证通过、§3.2 HyperlinkOpen/Close 决定删除、§3.3 Bell 音频远期；详见 TASKS.md §3） |
| 6 | parking lot 重新设计 + split 保活 + 双 scrollback 去重 | ✅ 实质收尾（§5.1 park/unpark + ptyBridge 落地；§5.2 双 scrollback 去重核查后决定不做，详见 TASKS.md §5.2） |
| 7 | 删除 xterm 依赖、清理 | ✅ 基本完成（xterm npm dep / Pane.svelte / PaneRouter / terminalRegistry 全部移除；CLAUDE.md 描述已对齐；剩 §7.2 浏览器实跑回归） |

**当前实际位置：round 2.4 → 4/5 → 6/7 全链路代码层面收尾完毕**。**剩余主线工作只剩 round 3**（WebGPU + atlas + 共享 surface），以及 §7.2 浏览器实跑回归（需用户）。round 4 的 §2.3 reflow 由用户独立推进。

---

## 4. 已实现行为清单（截至 round 2.4 + 协议补全 patch 2026-05-02）

### VT/ANSI 解析
- C0 控制：BS / HT / LF / VT / FF / CR
- CSI verb: A/B/C/D（cursor 移动）、E/F（CNL/CPL）、G/`/d（绝对定位）、H/f（光标到）、J/K（erase）、S/T（scroll）、L/M（IL/DL）、r（DECSTBM）、h/l（mode set/reset）、m（SGR）
- **CSI cell-edit verbs（2026-05-02 补）**：X（ECH 擦字）、@（ICH 插字）、P（DCH 删字）、a（HPR）、e（VPR）、s/u（SCO 光标存/取）。这一组是 Ink/PSReadLine/readline/ratatui 做局部刷新依赖的核心动词，**不接 = 字符残留 + 错行**。详见 [PARTIAL_REDRAW_PROTOCOL.md](PARTIAL_REDRAW_PROTOCOL.md)。
- **CSI 查询响应（2026-05-02 补）**：n（DSR-status/DSR-CPR/DECXCPR）、c/`>c`（DA primary/secondary）、t（窗口尺寸 18/19）。响应通过新的 `Terminal::pending_response` 队列回送 PTY；不接 = PowerShell 退 TUI 后 prompt 落到错行。
- **OSC events 通道（2026-05-02 补）**：解析 OSC 0/1/2（标题）、7（CWD）、8（hyperlink）、BEL，通过新的 `Terminal::pending_events: Vec<KernelEvent>` 队列暴露给 JS。`KernelEvent` enum 用 serde tag-content 序列化为 JS 对象。`manager.ts` 在 feed 后 drain 并调 `eventHandler` 路由到 Svelte stores。已接通 CWD → `paneCwdStore`；标题/超链接/Bell 暂为占位符待 round 4-5 UI 接入。
- ESC: 7/8（DECSC/DECRC）、D/E/M（IND/NEL/RI）、=/>（DECPAM/DECPNM）、c（RIS）

### SGR
- 0/1/2/3/4/5/7/8/9/21..29 全部 flag
- 30..37 / 40..47 / 90..97 / 100..107 ANSI 16
- 38;5;n / 48;5;n 256 色
- 38;2;r;g;b / 48;2;r;g;b truecolor
- 38:2:cs:r:g:b / 38:2:r:g:b 冒号子参数形式

### 屏幕语义
- 主屏 + alt 屏（DECSET/RST 47 / 1047 / 1049）
- 滚动区（DECSTBM）—— SU/SD/IL/DL/IND/RI 全部区域感知
- DECAWM pending wrap（vim 右下角字符正确）
- 宽字符（CJK + emoji 强制宽 2）
- 软换行标志（reflow 预留）

### 模式
- DECAWM(?7) / DECTCEM(?25) / 光标闪烁(?12)
- 应用键盘(?1) / app keypad
- bracketed paste(?2004)
- **同步输出模式(?2026)（2026-05-02 补）** — kernel 追 `Modes::sync_output`；manager rAF 在 sync_output=true 时 hold frame；JS 端 150ms timeout fallback 防卡死
- 鼠标(?9/?1000/?1002/?1003/?1004/?1006)
- DEC origin(?6)、insert mode(4)、LNM(20)

### 缓冲与回收
- AttrTable flyweight（u16 索引去重）
- Scrollback ring buffer，row 分配跨界回收（无 alloc churn）

---

## 5. 已知未实现（后续 round 处理）

| 功能 | 计划 round | 备注 |
|---|---|---|
| 渲染（任何形式） | 2.2 | 没渲染就看不到屏幕 |
| 键盘 → PTY 编码 | 2.3 | onData 等价物 |
| Resize → 后端 IPC | 2.3 | 你现有 `resize_pane` 沿用 |
| OSC 0/1/2 标题 | ✅ 2026-05-02 完成 | RidgePane 在 onEvent 把 TitleChanged/IconNameChanged 写到 `paneOscTitleStore` + `terminalTitles`，与 Pane.svelte 的 backend-event 路径并行幂等；onDestroy 清理 |
| OSC 7 cwd | ✅ 2026-05-02 完成 | RidgePane TitleChanged 之前已接 `setPaneCwd(workspaceId, paneId, value)` |
| OSC 8 超链接 — 数据层 + Ctrl+click | ✅ 2026-05-02 完成 | cell.rs 加 HyperlinkSpan + Row.hyperlinks Vec；parser 在 OSC 8 之间为每个 print 标记 cell（自动 coalesce 邻接同 uri）；JsTerminal::hyperlinkAt 查询；manager pointerdown 检 Ctrl+click → @tauri-apps/plugin-opener 打开 |
| OSC 8 超链接 — 视觉下划线 + Ctrl-hover | ✅ 2026-05-02 完成 | Theme 加 hyperlink_color；新 `RenderBackend::draw_hyperlink_underlines` trait method（canvas2d 1px fill_rect at cell bottom）；renderer.tick 收集 viewport hyperlink rects 经 draw_frame 在 selection overlay 之前画；manager pointermove 检 ctrl+hyperlinkAt → `container.style.cursor = 'pointer'` |
| IME helper textarea — v1 基础合成 | ✅ 2026-05-02 完成 | RidgePane 加 invisible textarea 钉在 pane 左下，container.pointerdown → textarea.focus；compositionstart 设 isComposing 守卫，compositionend 经 manager.write 把 e.data 送 PTY；container tabindex=-1，键盘焦点全部经 textarea；onContainerKeyDown 检 isComposing/e.isComposing 跳过|
| IME — v2 cursor-tracking | ✅ 2026-05-02 完成 | lib.rs 暴露 `cursorRow/cursorCol`；manager `cursorPixelPosition(paneId)` 返回 `{x, y, cellH}`；RidgePane `repositionImeHelper()` 在 pointerdown→focus 之后 + compositionstart + textarea focus 时调，把 textarea 钉在光标下方一行（候选窗自然出现在那里）|
| IME — v3 pin observer 防破坏 | 4 (远期) | 当前没有外部代码改 textarea style，所以暂不需要 MutationObserver；如果未来加了 portal 等容器变换可能需要 |
| 选择 + 复制（鼠标拖动 + 视觉高亮） | ✅ 2026-05-02 完成 | manager 接 pointerdown/move/up，translate 像素 → cell；renderer 加 selection_bg + draw_selection_overlay，selection 变化 force redraw 防 ghost 残留 |
| 选择 — 双击词 / 三击行 | ✅ 2026-05-02 完成 | selection.rs 加 select_word/select_line；manager 路由 e.detail 2/3；多击不进 drag 模式 |
| 选择 — Shift-click 扩展 | ✅ 2026-05-02 完成 | manager pointerdown 检 e.shiftKey + 复用 entry.selectionStart 作为 anchor；后续 move 继续延伸（xterm 行为） |
| 在 pane 内搜索（含 scrollback） | ✅ 2026-05-02 完成 | `src/search.rs` 模块；初版仅 viewport，2026-05-02 后续扩展到 scrollback：用 abs_row 坐标统一编码（0..sb_len = scrollback, sb_len.. = viewport），next/prev 自动 scroll viewport 把活动匹配带到顶部；活动匹配复用 selection_bg overlay 高亮 |
| Grapheme cluster（多码点合并） | 远期 | 当前 0-width 字符直接丢弃，最常见 emoji ZWJ 序列会显示不对 |
| Resize reflow — Phase 1（live grid，主屏幕） | round 4 收尾 | 用户在 splitpanes 拖动后，长行需要按新列宽重排；详见 §7「Resize reflow 设计」+ TASKS §2.3 |
| Resize reflow — Phase 2（scrollback + 锚点迁移） | round 5+ | 翻历史时长行也按新列宽显示；selection / hyperlink 锚点跟随移动 |
| sixel / DCS 图形 | 不做 | 不在范围 |
| ~~同步输出模式（`?2026`）~~ | ✅ 2026-05-02 完成 | Ink/lazygit 用来防止帧分撕，详见 [PARTIAL_REDRAW_PROTOCOL §4.1](PARTIAL_REDRAW_PROTOCOL.md#41-同步输出模式2026--已交付2026-05-02-后续-patch) |
| ~~REP `CSI <n> b`（重复上一字符）~~ | ✅ 2026-05-02 完成 | Terminal 跟踪 last_printed (char,attrs)，REP 复读；详见 [PARTIAL_REDRAW_PROTOCOL §4.2](PARTIAL_REDRAW_PROTOCOL.md#42-rep-csi-n-b) |
| ~~DECSCUSR `CSI <n> SP q`（光标形状）~~ | ✅ 2026-05-02 完成 | vim insert mode / readline 视觉切换；`Modes::cursor_shape` + renderer 直接读 |
| OSC events 通道（typed event queue） | 5 | DSR 已有 `pending_response` 通道，OSC 需要 typed events（Title/Cwd/Hyperlink） |
| ~~焦点事件回送（`?1004`）~~ | ✅ 2026-05-02 完成 | manager 监听 container focusin/focusout，按 mode 发 `\x1b[I`/`\x1b[O`。详见 [PARTIAL_REDRAW_PROTOCOL §4.5](PARTIAL_REDRAW_PROTOCOL.md#45-焦点事件回送) |

---

## 6. 风险与限制

### 已识别的高风险点

**R1 — 没有自动化集成测试环境** _(2026-05-02 部分缓解 / 2026-05-03 复测)_
我（Claude）的沙箱不能跑 `wasm-pack`、不能跑你的 Tauri app。所有"看起来对"的代码每轮都要你帮忙跑 `cargo test --lib` 才能验证。
**已缓解**：`tests/` 集成测试目录（cargo 标准位置），`tests/common/mod.rs` 提供 `run_scenario / run_chunks` helpers，`tests/protocol_smoke.rs` 22 个 realistic 字节流场景（DSR-CPR、PSReadLine prompt redraw、Ink frame replace、ECH 字符残留 repro、?1049 alt-screen round-trip、OSC 8 跨 feed 持久化、ICH+DCH inline edit、`?2026` toggle、`?1004` focus reporting、REP、RIS、OSC 133/633 prompt marks、OSC 事件顺序等）。`cargo test` 一键 run 全部 135 个（113 unit + 22 integration）。后续协议补全 patch 都应附带集成场景。

**R2 — 第一版性能不一定打得过 xterm + WebGL** _(2026-05-03 状态)_
原预期 round 2.4 接入后比 xterm 慢 30-50%，由 round 3 WebGPU + atlas 收敛。实际：round 7 已 retire xterm，前端只剩 wasm + Canvas2D 一条路；用户未报告显著卡顿。性能层面没有 xterm/WebGL 旧路径可以"切回"对比，因此 round 3 WebGPU 仍按计划推进，但不再是阻塞项（详见 §4.1–4.4 / TASKS §4.1–4.4）。

**R3 — IME 移植踩坑** _(2026-05-02 完成)_
原 Pane.svelte 的 IME 修复代码（compositionend 后清空 textarea、helper-textarea pin 防止跟着光标跑）已在 RidgePane.svelte 重新实现 + 升级到 v2（cursor-tracking、composition guard、helper textarea 跟随 wasm kernel cursor 位置而非用户最后点击位置）。Pane.svelte 已删除。后续若 portal/dragdrop reparent 导致 helper 绝对定位失效，再补 MutationObserver 守护（TASKS §2.2 ⏳，当前不阻塞）。

**R4 — ConPTY reflow 协议** _(2026-05-03 状态)_
后端 resize-silence 机制（`pty.rs:95` 的 `RESIZE_SILENCE_WINDOW_MS = 800`）保留：丢掉 ConPTY reflow 风暴的设计原本是为 xterm，新内核接管后该 window 仍能稳定吃掉 PowerShell + ConPTY 的瞬时重画。延迟侧的关注点已转移到 `BUGFIX.md` 的 BUG-4（4ms 合批窗口）；进一步压缩到 < 10ms 需要 SharedArrayBuffer 替换 Tauri IPC（参见 `REPLACE_AND_FIX_PLAN.md`，未启动）。

**R5 — 后端 4MB scrollback / 前端 wasm 内核 buffer 的双重存储** _(2026-05-03 决议：保留)_
原计划：前端只缓存 viewport + 256KB tail，深翻历史走后端 `get_pane_scrollback_before`。实际：见 TASKS §5.2 — 缩小 wasm kernel scrollback capacity 至 256 行与 `push_front` evict-newest 语义冲突（深翻历史会逐出近期行）；改 evict-oldest 也不能扩 effective scrollback。最终方案 A：不去重，接受每 pane ~700 KB 重复，§5.2 关闭。`Shift+PageUp` 越过 wasm 边界时通过 `manager.prependScrollback` 走后端 `get_pane_scrollback_before`（TASKS §2.1 反向 scrollback bridge）。

### 不可逆改动（已实施）

Round 7 收尾时：
- ~~Pane.svelte 不再 import `@xterm/*`~~ — Pane.svelte 整体已删除（被 RidgePane.svelte 取代）。
- ~~terminalRegistry.ts 改成只持有 paneId~~ — terminalRegistry.ts 整体已删除（manager.ts 自管 PaneEntry）。
- 用户的本地工作区文件如果有 xterm 相关序列化字段会被忽略（仍适用）。

xterm 整套 npm 依赖（`@xterm/*`）已从 `package.json` 删除；`@xterm` 在 `src/` 下 0 引用。

---

## 7. Resize reflow 设计

### 7.1 问题陈述

用户拖动 splitter 改变 pane 大小后，新尺寸下应该看到「按新列宽换行」的内容；
当前 `Grid::resize` 只做 truncate/pad（grid.rs:185 对每行 `r.resize(cols)`），
导致：

- **收窄**：超出新列宽的字符被丢弃（不可恢复，除非应用自己重画）。
- **拉宽**：原本被软换行截断的长行，在右侧留出空白，没有「拼接回来」。
- **scrollback**：历史输出始终保持原列宽，翻历史看到错位的内容。

### 7.2 候选方案

| 方案 | 思路 | 优点 | 缺点 | 评估 |
|---|---|---|---|---|
| A. **真 reflow** | 把 `wrapped=true` 链拼成「逻辑行」，按新列宽重新分配到 grid 行 | 静态输出（scrollback、长命令输出）正确显示 | Rust 代码量较大；cursor / selection / hyperlink 锚点都要迁移 | **采纳（分阶段）** |
| B. **只发 SIGWINCH** | 让 shell / TUI 自己重画 | 零内核工作量；vim / less / htop 等本来就靠 SIGWINCH 重绘 | 不修 scrollback；屏幕上已有的静态文本仍然错位 | 已默认在做（`fitPane` 调 `resizeHandler` 触发 PTY SIGWINCH），是基线但不够 |
| C. **发假的 Ctrl+L** | 给 PTY 注入 form feed 让 shell 清屏 | 实现极简 | 入侵性强（注入用户没敲的字节）；丢有用内容；不修 scrollback | **不采用** — 用户明确说 "避免重复输出" |
| D. **scrollback 懒 reflow** | 主屏幕实时 reflow，scrollback 只在读取时 reflow | resize 更快 | 双状态机；复杂度高 | 暂缓 |

### 7.3 分阶段交付

- **Phase 1（round 4 收尾）**：live grid 列变 reflow，仅主屏幕。
  - alt 屏幕（vim / less / htop）继续走 truncate/pad —— TUI 本来就靠 SIGWINCH 重画，不应被 reflow 干扰。
  - scrollback 暂不 reflow（保留原列宽，翻历史会错位）。
  - selection 在 reflow 时清空（最简策略；用户重新选即可）。
  - cursor 用「逻辑位置」迁移（reflow 前记下 cursor 所在的逻辑行 + 列偏移，reflow 后按新列宽换算）。
  - hyperlink span 在 reflow 时随 cell 一起搬运（per-row 元数据，重新分配行时同步重写）。

- **Phase 2（round 5+）**：scrollback reflow + 选择/链接锚点迁移。
  - 同算法应用到 scrollback ring。4 MB scrollback 全量重排成本可接受（一次拖动 1-2 次 reflow，每次几十 ms）。
  - selection 锚点用「逻辑位置」记，reflow 后按新列宽推回 (row, col)。

### 7.4 触发时机

`manager.ts::viewportChanged(paneId)` 已经有 120 ms trailing-edge debounce — 用户停止拖动 120 ms 后才触发一次 `fitPane`。Phase 1 复用这个路径，无需新触发器。

```
ResizeObserver fires (~60Hz during drag)
  → viewportChanged() resets 120ms timer
  → user releases mouse, layout settles
  → 120ms later: fitPane()
      → handle.resize(canvas)         — 调整 canvas 尺寸 / DPR
      → resizeHandler(rows, cols)     — invoke resize_pane → PTY SIGWINCH
      → kernel.resize(rows, cols)     — Phase 1 在这里做 reflow（仅主屏幕，仅列变）
```

### 7.5 算法（Phase 1）

仅在「列数发生变化 AND 在主屏幕」时执行；其它路径维持原 truncate/pad。

1. **stitch**：从顶到底扫描 `screen.rows`，把 `row.wrapped=true` 的行和下一行的 cells 拼到当前逻辑行。逻辑行结束于 `wrapped=false`（硬换行）或 buffer 末尾。
2. **track cursor**：扫描时如果遇到 cursor 所在行，记录 cursor 在「当前逻辑行的第几个 cell」（绝对偏移 = 已累积 cells 长度 + cursor.col）。
3. **rewrap**：清空 grid，对每条逻辑行按 `new_cols` 切片，每段写入一行；如果还有更多段，前一行 `wrapped=true`，下一行接续。
4. **place cursor**：根据记录的逻辑偏移 → 新 (row, col) = (offset / new_cols, offset % new_cols)。
5. **scroll-up overflow**：如果重新分配后总行数 > new_rows，最早的若干行进入 scrollback（与正常 LF 滚动一致；alt 屏幕不入 scrollback）。
6. **selection 清空**：避免残留在错位的位置上。

### 7.6 测试覆盖

- **shrink reflow**：80 列 grid 上有一行 70 个 'a'，resize 到 40 列，应得到两行（40 + 30 个 'a'，第一行 wrapped=true）。
- **grow reflow**：40 列 grid 上有「40 个 'a' + wrapped + 30 个 'a'」两行，resize 到 80 列，应得到一行 70 个 'a'。
- **cursor 迁移**：cursor 在原 (5, 25)，reflow 到不同列宽后位置正确。
- **alt-screen no-reflow**：alt 屏幕上 resize 不触发 reflow，行只 truncate/pad。
- **wrapped 链 unwrap+rewrap**：3 行 wrapped 链 reflow 到不同列宽，unwrap 后 rewrap 结果正确。

---

## 8. 文档导航

- 你正在读：`OVERVIEW.md`
- 接入步骤：`INTEGRATION.md`（最终态接入，不含分阶段过渡）
- 主项目独立 bug 清单与 patch：`BUGFIX.md`（与本替换工作正交，可单独 merge）
