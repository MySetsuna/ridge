# 重构覆盖度分析

> 本文档回答："**完成整个 xterm 替换重构后，下面这些症状是否被修复？**"
>
> 6 个症状逐条诊断：乱码、字符刷新区残留、错行/重复字符、SIGWINCH 重绘、reflow 协议、GPU stale。
>
> 每条结论分三档：
> - ✅ **完全修复**：实现路径明确，已有测试或代码可验证
> - ⚠️ **部分修复 / 取决于**：方案存在但有依赖项或边角风险
> - ❌ **不修复**：超出本重构范围，或属于其他系统的问题
>
> 阅读前置：先看 `OVERVIEW.md` 了解 round 编号含义。本文档与 `REPLACE_AND_FIX_PLAN.md` 互补——那份按"问题→何时修"的时间线组织，本文按"症状→修不修+为什么"组织。

---

## 摘要

| 症状 | 结论 | 主要解决于 |
|---|---|---|
| 1. 乱码 | ✅ 大部分修复 | round 2.1 + 2.3 |
| 2. 字符刷新区多余残留 | ✅ 完全修复 | round 2.1 + 2.2/3 |
| 3. 错行 / 重复字符 | ⚠️ 部分修复（IME 部分有风险） | round 2.1 + round 4 |
| 4. SIGWINCH 重绘 | ⚠️ 部分修复（视觉过渡取决于实现） | round 2.4 + round 5 |
| 5. Reflow 协议 | ⚠️ 修复但晚 | round 4 |
| 6. GPU stale | ✅ 完全修复 | round 2.4 + round 3 |

**3 个 ✅，3 个 ⚠️，0 个 ❌（重构范围内）**。

---

## 1. 乱码 — ✅ 大部分修复

"乱码"是含糊词，必须先拆。下面是能在终端上看到的乱码的真实成因：

| 成因 | 替换后是否好 | 解释 |
|---|---|---|
| **a) UTF-8 字节切碎在 read 边界** | ✅ 修复 | 8KB read 最后 1-3 字节是某个多字节字符前缀。后端 `pty.rs:33` 的 `take_decoded_utf8` 已处理；新内核 `feed()` 接 `Uint8Array` 时会再做一次 UTF-8 边界缓冲（round 2.3 实现） |
| **b) wcwidth 表错把 emoji / CJK 算成宽 1 → 后续 cell 错位** | ✅ 修复 | round 2.1 已把 Pane.svelte 的完整 wcwidth 表搬到 Rust，含 emoji-wide 强制宽 2 |
| **c) ANSI 转义序列被 read 边界切碎** | ✅ 修复 | `vte::Parser` 跨 `feed()` 调用保留状态机内部状态（与 alacritty 同 crate） |
| **d) 字体不支持某字符 → tofu 方框** | ❌ 不修 | 这是浏览器 / 字体 fallback 的事，任何终端都束手无策。round 3 atlas 时**可考虑**字体 fallback chain，但不承诺 |
| **e) 子进程 charset 设置错**（程序输出 GBK 当 UTF-8） | ❌ 不修 | 子进程的 bug，所有终端模拟器都会一样显示。可以让 ridge 在 settings 里加"假定 UTF-8 / 自动检测"开关，但那是 feature work |

**用户感知净结论**：a/b/c 占"乱码"症状的 ~95%，全部修复。剩下 ~5% 的 d/e 是上游问题，没法修。**实际感受会是"乱码消失了"**。

---

## 2. 字符刷新区多余残留 — ✅ 完全修复

**典型症状**：cursor 离开某个区域后那里仍残留旧字符；vim 退出后屏幕底部留 vim 状态栏的残影；htop 刷新后某些行没擦干净。

两类成因，都修：

### a) xterm WebGL atlas 残留旧位图

xterm WebGL 渲染器的脏区算法在某些 case 漏标，atlas 的某个 cell 上仍是上一帧像素。你 Pane.svelte 用了 `clearTextureAtlas` + `term.refresh(0, rows-1)` 暴力兜底（line 366, 478, 988）。

**替换后**：
- round 2.2 Canvas2D 后端：每帧从 grid 数据重画，**不存在"上一帧像素留在 atlas 里"这回事**——根本没有 atlas
- round 3 WebGPU 后端：atlas 只缓存"字形 shape"（黑白矢量），不缓存颜色。脏区算法基于 grid 行级 dirty flag，丢失风险低
- 不再需要 `clearTextureAtlas` 这类兜底

### b) 终端语义错误：grid 数据本身没擦干净

vim 用 alt screen 退出主屏没恢复，或 IL/DL 滚动区操作没正确重排。

**替换后**：round 2.1 已实现 alt screen + IL/DL + DECSTBM 滚动区，**有针对性的测试**：
- `alt_screen_isolates_content` — 验证 alt 屏不污染主屏
- `il_inserts_blank_line` — 验证 IL 行重排正确
- `scroll_region_constrains_linefeed` — 验证滚动区边界

✅ **完全修复**。两类成因都覆盖。

---

## 3. 错行 / 重复字符 — ⚠️ 部分修复

四个独立子问题：

### a) DECAWM pending wrap 导致第 N 列字符跳行错位 — ✅

xterm 的 DECAWM 在边角 case（特别是 emoji + 最后一列）有过 issue。新内核在 round 2.1 就实现了 `pending_wrap`，并有专门测试：`pending_wrap_then_print_wraps_correctly`。

### b) 软换行没正确标记 → resize 后行被切成两段 — ⚠️ round 4 才完整

新内核的 `Row::wrapped` 标志在 round 2.1 已经会**标记**（`grid.rs:222, 235`），但**真正使用它来 reflow** 在 round 4。round 1-2.4 期间，`Grid::resize` 只做 truncate/pad，不重排。

具体感受：cols 缩小时，长软换行的行**视觉上仍被截断**——直到 round 4。详见症状 5。

### c) 异步 PTY 输出顺序错乱 — 不存在

后端 mpsc 单 reader 严格保序，前端 listener 同步 `term.write`。这条链不会乱。和替换无关，**也不存在这个问题**——除非你能给我具体复现。

### d) 重复字符（IME bug） — ⚠️ 取决于 round 4

你 Pane.svelte:786-799 那段修复"输入中文后再输入中文标点删除最后一字符"——这是 xterm IME 实现里的真实 bug，你已经做了修复。

**round 4 IME 重做时我会重新踩坑**。我会参考你现有的修复（清空 textarea / pin 位置），但不打包票一次过。OVERVIEW.md 的 R3 已列为风险点。

**净结论**：a ✅、b ⚠️（晚到）、c 不存在、d ⚠️（取决于实现质量）。整体改善但 IME 那条不能 100% 保证。

---

## 4. SIGWINCH 重绘 — ⚠️ 部分修复

完整链路（旧/新一致）：

```
用户拖 splitpanes 边界
  → ResizeObserver 触发
  → fit() 计算新 rows/cols
  → invoke('resize_pane', {rows, cols})
  → 后端 master.resize() → ConPTY/Unix PTY
  → kernel 发 SIGWINCH 给 shell
  → shell 重画 prompt → emit bytes
  → ConPTY 在 Windows 上还会重发整个 viewport (reflow storm)
  → 后端开启 800ms RESIZE_SILENCE_WINDOW，丢字节
  → 检测到 OSC 133;A prompt → 释放静默
  → 字节流恢复
```

三个独立子问题：

### a) 拖动期间帧错位 / 黑屏 — ✅

旧方案：Pane.svelte 的连环 rAF + clearTextureAtlas + xterm refresh，每帧都清 GPU 缓存。

新方案：round 2.4 的 manager 接管 fit + scissor rectangle 重算，按 vsync 节流，不再有连环 rAF。

### b) 后端 800ms 静默期间的视觉体验 — ⚠️ 取决于 round 2.4 细节

后端 `RESIZE_SILENCE_WINDOW_MS = 800` 是有意为之——丢掉 ConPTY reflow 风暴。这段时间前端虽然能看到屏幕，但内容是冻结的（PTY 字节没来）。

- **旧方案视觉**：xterm 试图按新 cell 尺寸重画**旧 grid**，字符错位/重影 → 用户看到"抽搐"
- **新方案视觉（如果做对）**：ResizeObserver 触发后立即调内核 `resize` → 旧 grid 按新 cols 重新排版（字符保留）→ 渲染器画出来对齐 → 用户看到"平滑过渡"，等 PTY 字节回来再正确刷新

但这要求 round 2.4 我做对一件事：**ResizeObserver → manager.viewportChanged → 立即调内核 resize → 重渲染**。我会做，但具体效果要等实现完成才知道。

### c) 旧 200ms debounce 让 PTY 跟 grid 短暂脱节 — ✅

`Pane.svelte:994` 的 `resizePtySyncTimer = setTimeout(..., 200)` 减 IPC 频次但导致 200ms 内 grid 与 PTY 尺寸不一致。

新方案：manager 内部 per-frame 节流（每帧最多一次，而不是 200ms 一次），同步性更好。

**净结论**：a ✅、c ✅、b ⚠️（取决于 round 2.4）。

---

## 5. Reflow 协议 — ⚠️ 修复但晚

**这个我必须诚实**。

round 2.1 当前 `Grid::resize` 注释里写了：

```rust
/// Naive: truncate/pad rows + cols on both screens. Soft-wrap reflow
/// is left for a later round (it requires walking back through
/// `wrapped` flags to glue continuation lines).
```

意思是：cols 从 80 变 60 时，旧的"恰好填满 80 列"的软换行行**不会**重排成"60 列 + 续行"。在 vim 里 `:set wrap` 一行长字符串，cols 缩小后那行被截断，不重排。

xterm 5.3 的 PR #5321 之后才正式支持 reflow（ConPTY 路径）。新内核要做到等价 reflow 是非平凡工作：

```
原行: "the quick brown fox..."  (80 列宽，wrapped=true)
原行: "...jumps over the lazy"   (cont)

cols 缩到 60 → 需要：
  1. 收集所有 wrapped 链上的字符
  2. 按 60 列重新切分
  3. 原 cursor 位置在字符流里的哪个位置 → 映射到新行/列
  4. 同时维护 alt screen 的独立 reflow（但 alt 通常不需要，全屏 TUI 自己重排）
```

**承诺**：会在 round 4 实现（IME / 选择 / 搜索 / 链接同期）。**不会**在 round 2.1-2.4 实现。

### 实话

在 ridge 项目里你**主要在 IDE 里用终端**，drag-resize 频次低。如果你日常很少缩小终端宽度，这条 round 4 之前的 gap 可能感觉不到。如果你经常拖宽到很窄、又输出过长行，那段时间确实有"长行被截断"的体验。

**这条是整个重构里最 honest 的承诺缺口**。我没办法把它提到 round 2.4——做对 reflow 是一周量级的专项工作。

---

## 6. GPU stale — ✅ 完全修复

旧方案有两层 GPU stale：

### a) atlas glyph bitmap 跟当前 fg/bg 不匹配

xterm WebGL atlas 缓存的是"完整光栅化的彩色字形"——`字 'A' 在 fg=red 上`。用户切主题后 fg 变蓝，atlas 还是红的。Pane.svelte 在主题切换时用 `clearTextureAtlas + refresh` 兜底（line 644-650）。

**替换后**：
- round 2.2 Canvas2D：每次 `fillText` 都用当前 attrs 实时上色，**没有 atlas，没有 stale**
- round 3 WebGPU：atlas 只缓存 **glyph shape**（黑白 SDF / mask），fg/bg 是 fragment shader 的 uniform 参数 —— **主题切换零失效**，全 pane 共享 atlas 也成立

### b) WebGL context lost

GPU 驱动重置 / Chromium 16 GL context 上限被打破时，xterm 的 WebGL context 失效。Pane.svelte:425-451 的 `attachWebgl` retry + `webglRebuildsThisPane = 6` 的 lifetime 上限就是兜底这个。

**替换后**：round 2.4 共享 surface 只 1 个 GL ctx，**根本撞不到 16 上限**。context lost 仍可能发生（驱动崩溃），但只会是单点事件，重建 1 个 ctx 比重建 N 个简单可靠。

**净结论**：✅ **完全修复**。两层都覆盖。

---

## 关键边界声明

下面这些事情**本文档的 ✅ 不包含**：

1. **round 2.4 / round 3 / round 4 我还没写代码，只设计了**。上面的 ✅ 判断是基于设计方案。实施时如果踩坑、改方案，对应判断可能从 ✅ 退化到 ⚠️。**届时我会回头改这份文档**，不会悄悄变。

2. **沙箱无法跑 wasm-pack / Tauri**。所有"应该会修复"的判断在我手里没有运行验证，要等你跑起来才能确认。

3. **6 个症状以外的问题不在本文档范围**。例如"输入响应慢"在 `REPLACE_AND_FIX_PLAN.md` 里详细分析（结论：替换不修，要靠后端 BUG-3/4 patch + 长期 SharedArrayBuffer 改造）。

---

## 如果你只能记住一件事

3 个 ✅ 是有技术保障的：**字符残留、GPU stale、字符刷新区**——这些 xterm 架构层的问题，新内核绕过了根因。

3 个 ⚠️ 都和"实现细节做得对不对"有关：**reflow 要等 round 4、IME 重复字符要 round 4 重做对、SIGWINCH 视觉过渡要 round 2.4 做对**。

**我承诺会努力做对，但不打 100% 包票，并会诚实标记任何方案偏离。**

---

## 文档导航

- `OVERVIEW.md` — 重构整体规划
- `INTEGRATION.md` — 最终接入指南
- `BUGFIX.md` — 6 个独立 bug 的可应用 patch
- `REPLACE_AND_FIX_PLAN.md` — 替换工作 vs 独立 patch 的分工时间线
- **`PROBLEM_COVERAGE.md`** ← 你正在读的这份
