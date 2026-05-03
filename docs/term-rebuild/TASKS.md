# Ridge Term — 任务清单与进度跟踪

> 创建日期：2026-05-03
> 上次更新：2026-05-03
>
> 本文档是 [OVERVIEW.md](OVERVIEW.md) §3 进度表的可执行版本：包含具体任务、责任划分、验收标准、进度记录。
> 与 OVERVIEW.md 的关系：OVERVIEW 描述目标与里程碑；本文档跟踪"下一步做什么"。
> 编辑约定：完成的任务**不删**，改成 ✅ 并在「进度记录」尾部追加一行（日期 + 摘要 + 提交点）。

---

## 0. 当前进度快照（2026-05-03 重新核对）

OVERVIEW.md §3 的进度表早于本次 patch 写定，已严重过期。**真实状态**如下：

| Round | 范围 | 实际状态 | 备注 |
|---|---|---|---|
| 1 | VT 内核骨架 | ✅ | 27 单测通过 |
| 2.1 | wcwidth + alt screen + DECSTBM + DEC modes | ✅ | 28 单测通过 |
| 2.2 | 渲染抽象 trait + Canvas2D 后端 | ✅ | `src/render/{backend.rs, canvas2d.rs, renderer.rs}` 全部落地 |
| 2.3 | JS 表面 API（write/onData/resize/key encoder） | ✅ | `src/lib.rs` `TerminalKernel`/`RenderHandle` wasm-bindgen 导出 |
| 2.4 | TerminalManager + RidgePane + PaneRouter | ✅ | `src/lib/terminal/manager.ts`、`src/lib/components/{RidgePane, PaneRouter}.svelte` 落地，`SplitContainer` 已切到 PaneRouter |
| **— 协议补全 patch（OVERVIEW §4 列表）** | ECH/ICH/DCH/REP/DECSCUSR/DSR/DA/?2026/?1004/OSC0/1/2/7/8 | ✅ | 92 单测 + 7 集成测试，pending_response + pending_events 通道接通 |
| **— round 4 部分提前** | 鼠标拖选（含 word/line/shift-click）、Ctrl+F 搜索、IME v2 cursor-tracking、Ctrl+click 链接 | ✅ | 与 INTEGRATION_R2_4.md 中"已知不工作"清单背离——实际代码已完成 |
| 3 | WebGPU 后端 + 字形 atlas | ⏳ 未开始 | `RenderBackend` trait 就绪等实现 |
| 4 | IME v3 + reflow + scrollback bridge + 链接 affordance 收尾 | ⏳ 部分完成 | 见 §2 |
| 5 | OSC UI 接入收尾 | ⏳ 部分完成 | TitleChanged 已写 store，UI 验证待做 |
| 6 | parking lot + 双 scrollback 去重 | ⏳ 未开始 | OVERVIEW §R5 |
| 7 | 删 xterm | ⏳ 未开始 | 取决于 round 4-6 |

**结论：当前位于 round 2.4 末尾 + round 4/5 局部提前交付**。下一阶段优先级是
（a）修补本次 review 发现的问题、（b）补齐 round 4/5 真正未完成的部分、
（c）启动 round 3 WebGPU。

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

### 1.4 [LOW] `?2026` sync output 超时后无 cool-down ⏳

- **文件**：`src/lib/terminal/manager.ts:715-727`
- **现象**：超时分支会在每帧都进入"挂着但仍 render"状态，syncStart 不重置。文档已承认这是 acceptable degradation；如要改进，超时一次后强制 `entry.syncStart = null` 退出 sync。
- **决定**：暂不改，等观察到实际 TUI 卡死再说。**保留追踪**。

### 1.5 [LOW] `canvas2d.rs::measure_font` 用 `'M'` 测宽 ⏳

- **文件**：`src/render/canvas2d.rs:98-103`
- **影响**：CJK fallback 字体下宽字符可能错位 1-2 px。
- **决定**：暂不改，round 3 WebGPU 时一并重做 metrics。**保留追踪**。

---

## 2. Round 4 收尾（IME v3 / 反向 scrollback / reflow）

### 2.1 后端 scrollback bridge — `Shift+PageUp` 越过 wasm buffer 边界 ⏳

- **背景**：OVERVIEW §R5。前端 wasm kernel 的 scrollback 容量等于 `scrollbackLines: 2000`；后端 `state.rs` 保留 4 MB 块。当用户翻历史超过 2000 行就翻不动了。
- **文件**：
  - `src/lib/terminal/manager.ts`：新增 `fetchOlderScrollback(paneId, byteBudget)`，调用 Tauri `get_pane_scrollback_before`；
  - `src/lib/components/RidgePane.svelte`：在 `Shift+PageUp` 时检查 `manager.scrollState`，到顶时触发 fetch 并 `feed()`。
- **风险**：feed 老字节会污染当前 cursor 行；需 kernel 提供"只追加 scrollback、不动当前 cursor"的 API（`Terminal::prepend_scrollback`）。
- **拆分**：先做 kernel API（Rust），再接前端。

### 2.2 IME v3 `MutationObserver` 守护 ⏳

- **场景**：未来如果 portal/dragdrop 把 RidgePane 容器 reparent，`imeHelper` 的绝对定位会失效。
- **文件**：`src/lib/components/RidgePane.svelte`
- **修法**：可选——在 dev 环境观察到布局抖动再加。当前不阻塞。

### 2.3 Resize reflow（软换行行重排）⏳

- **背景**：现在 resize 直接截断长行；收宽后再放宽不会自动恢复。
- **文件**：`src/term/grid.rs::resize`（注释里直接写了 "Naive: truncate/pad"）
- **优先级**：低（用户多在固定宽下使用，splitpanes 拖动场景被 120ms debounce 覆盖）。如果 round 6 之前没人投诉就拖到 round 6。

### 2.4 Grapheme cluster（emoji ZWJ 序列）⏳ 远期

- **现状**：0-width 字符直接丢，`👨‍👩‍👧‍👦` 这种会拆成 4 个独立 emoji。
- **决定**：等用户报告再做，依赖 `unicode-segmentation` 接入。

---

## 3. Round 5 收尾（OSC UI 接入）

### 3.1 验证 `paneOscTitleStore` 真的驱动 SplitContainer 标题 ⏳

- **文件**：`src/lib/components/SplitContainer.svelte`
- **任务**：跑一遍 `printf '\x1b]0;hello\x07'`，确认 pane 标题栏立刻变为 "hello"。如果不变则需要：
  1. 检查 SplitContainer 是否订阅了 `paneOscTitleStore` 与 `terminalTitles` 的合并；
  2. 确认 OSC 0/1/2 的优先级覆盖了 backend polling 的 process name。

### 3.2 `HyperlinkOpen`/`HyperlinkClose` 事件目前只是 `console.debug` 占位 ⏳

- **文件**：`src/lib/components/RidgePane.svelte:218-222`
- **现状**：链接的视觉下划线（`draw_hyperlink_underlines`）已经在 renderer 里画了，Ctrl+click 也能打开 URL。**这两个事件目前看起来是冗余信号**，需确认是否真的需要前端响应——可能可以删掉。
- **任务**：要么实装（例如：HyperlinkOpen 时通知 Explorer 高亮当前包含活跃链接的 pane），要么从 `KernelEvent` enum 删除。

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

## 5. Round 6 — parking lot + 双 scrollback 去重

### 5.1 split 时保活 pane（park/restore）⏳

- **背景**：xterm 时代，`parkTerminal/restoreTerminal` 把 Terminal DOM 缓存，split 重新挂时复用。RidgePane 当前每次卸载即销毁 wasm kernel，PTY 由后端保留但 scrollback 体验不连续。
- **文件**：`src/lib/components/RidgePane.svelte` 顶部注释明确标"round 6"。
- **方案候选**：
  - A. 把 `manager.detach` 改成 `park`，保留 kernel 但移除 canvas，restore 时重新 attach。
  - B. 保持 detach，依赖后端 scrollback replay + activate 重新初始化（更接近 OVERVIEW D5 的"PTY 字节流不变"原则）。
- **决定**：先尝试 B（简单），如果用户感知到滚动位置丢失再上 A。

### 5.2 删除前端 wasm kernel 的 scrollback 重复 ⏳

- **背景**：OVERVIEW §R5。kernel 自带 2000 行 scrollback；后端 4 MB block 也存。重复占内存。
- **方案**：kernel scrollback 容量降到 256（够一屏 + 几页），翻历史走后端 `get_pane_scrollback_before`（与 §2.1 同一 API 路径）。
- **依赖**：§2.1 完成。

---

## 6. Round 7 — 删除 xterm

只在 round 4-6 全部稳定 ≥ 2 周、且至少 3 个用户主动跑实验渲染器无大问题后启动。

- **任务**：
  1. `package.json` 删 `@xterm/*` 依赖
  2. 删 `src/lib/components/Pane.svelte` + `PaneRouter.svelte`，把 `RidgePane.svelte` 直接 import 进 `SplitContainer`
  3. `src/lib/stores/settings.ts` 删 `useExperimentalRenderer` 字段
  4. `src/lib/stores/terminalRegistry.ts` 改成只持有 paneId（不再持有 Terminal 实例 / parking lot）
  5. 后端 `state.rs` 不动（PTY 协议未变）
  6. 全量 regress：跑 INTEGRATION_R2_4.md §Step 7-8 验证清单 + 复杂 TUI（vim、lazygit、btop、ratatui demo）
- **回滚成本**：≥ 1 commit revert，前提是 round 4-6 之间没人重写 SplitContainer。

---

## 7. 集成与验证遗留

### 7.1 `useExperimentalRenderer` 没有写入 typed `UserSettings` ⏳

- **文件**：`src/lib/stores/settings.ts`
- **现状**：`PaneRouter.svelte` 用 `($settingsStore as { useExperimentalRenderer?: boolean })` 旁路读取，所以 toggle 能工作，但 typed 接口没有声明这个字段，IDE 自动补全找不到，validation 也跳过。
- **修法**：按 INTEGRATION_R2_4.md §Step 5 把字段加到 `UserSettings` interface + `DEFAULTS` + `load()`。
- **优先级**：低，但属于 INTEGRATION_R2_4 验收清单的强制项。

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

- 2026-05-03 — 初次创建本文档；核对 OVERVIEW.md §3 进度表后确认实际位于 round 2.4 末尾 + round 4/5 部分提前；触发因素是 RidgePane 输入失效 bug 修复回归 — _pending commit_
- 2026-05-03 — 同步更新 OVERVIEW.md §3 表格为真实进度（rounds 2.2/2.3/2.4 ✅，rounds 4/5 部分完成）— _pending commit_
- 2026-05-03 — 修复 RidgePane 键盘焦点失效（`onkeydown` 上提到 container + `onmousedown.preventDefault()` 防焦点抢占）— _pending commit_
- 2026-05-03 — §1.1 修复 `pane-pty-closed` 重建路径：handler 改 async，依序 `create_pane` + `activate_pane_pty`，每步 guard `alive`；svelte-check 0 错误（2 处 pre-existing 警告）— `5c11914`
- 2026-05-03 — §1.2 + §1.3 修复：`manager.ts` `feed()` 无条件 `takePendingEvents()` + dev-only warn；`RidgePane.svelte` `onDestroy` 清理 `bellFlashTimer`；svelte-check 0 错误 — _pending commit_
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
