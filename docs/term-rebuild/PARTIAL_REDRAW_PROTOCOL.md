# TUI 局部刷新协议 — 深度调研

> 本文档由 2026-05-02 调研而成，对应 ridge-term `parser.rs` / `grid.rs` /
> `terminal.rs` 的协议补全 patch。
> 起因：claude code 局部刷新时出现"字符残留 + 错行"，根因不是渲染层，
> 是 VT 解析层缺失了一组 TUI 库赖以做 cell-level 局部刷新的核心动词。

---

## 1. TUI 库的局部刷新模式

下面三种是现实里 99% 的 TUI 应用使用的局部刷新模型：

### 模式 A — Ink / blessed 的"frame diff + 整行重写"

Ink 维护一个虚拟屏幕缓冲（`Output` 类），每帧 diff 上一帧得到 minimal patch：

```
帧 N 末尾光标在 (8, 0)
帧 N+1 渲染：
  CSI 8 A          ; 光标上移 8（回到帧 N 起点）
  CSI 0 J          ; 擦到屏幕末尾
  [写新内容]
  CSI <n> ; <m> H  ; 光标定位到帧末尾
```

**对终端的协议要求**：CUU、ED、CUP——我们都有。**不会触发本次 bug**。

### 模式 B — readline / PSReadLine 的"行内编辑"

PSReadLine 在用户输入时维护"光标 + 已输入字符"，每次按键：

```
CSI <n> P  (DCH n)    ; 删 n 个字符（左 shift）
[写新字符]              ; 插入
CSI <n> @  (ICH n)    ; 插 n 个空格（右 shift）
```

或纯擦写：
```
CSI 5 X  (ECH 5)     ; 在原位擦 5 个 cell（光标不动）
[写新内容]
```

**对终端的协议要求**：**ECH（X）/ ICH（@）/ DCH（P）**。
**这一组之前 ridge-term 全没接** → claude code 内部"擦旧帧再写新帧"的 ECH 被吃掉，旧帧字符就一直留在屏幕上。

### 模式 C — ratatui / lazygit 的"双缓冲 + 同步输出"

```
CSI ? 2026 h         ; begin synchronous update
[全屏重绘]
CSI ? 2026 l         ; end synchronous update（终端原子刷新到显示）
```

防止用户在帧绘制中途看到撕裂的中间态。**未实现 = 闪烁但不残留**。

---

## 2. 本次发现的根因清单

| 现象 | 协议根因 | 状态 |
|---|---|---|
| Ctrl+C 退 claude code 后 PS prompt 落到错行 | DSR (`CSI 6n`) 没响应 + `?1049l` 没 DECRC primary cursor | ✅ 已修 |
| claude code 局部刷新后字符残留 | ECH (`CSI <n> X`) 未实现，旧字符没擦 | ✅ 已修 |
| 输入插字串错位 | ICH (`CSI <n> @`) 未实现 | ✅ 已修 |
| 删除/补全候选不消失 | DCH (`CSI <n> P`) 未实现 | ✅ 已修 |
| 用 SCO 系列的库回不到原位 | CSI s / CSI u（DECSC/DECRC 别名）未实现 | ✅ 已修 |
| 部分库布局错乱 | `CSI 18 t`（窗口尺寸查询）无响应 | ✅ 已修 |
| 个别新 TUI 闪烁 | `CSI ? 2026 h/l`（同步输出模式）未实现 | ✅ 2026-05-02（详见 §4.1） |
| Ink 长行 reflow 后偶发错位 | resize reflow Phase 1（live grid 主屏幕）未实现 | ✅ 2026-05-03（Phase 1 ✅，Phase 2 scrollback / 锚点 ⏳ 远期，详见 §4.6） |

---

## 3. 实现要点（已落实 patch）

### 3.1 双向通道

之前 `Terminal::feed()` 是单向的——只消费字节，没法把 DSR/DA 响应送回 PTY。
新加 `pending_response: Vec<u8>` 字段：

```rust
pub struct Terminal {
    /* ... */
    /// Bytes the parser produced that must be sent BACK to the PTY.
    pending_response: Vec<u8>,
}

pub fn take_pending_response(&mut self) -> Vec<u8> {
    std::mem::take(&mut self.pending_response)
}
```

`Performer` 多一个 `&'a mut Vec<u8>` 借用，在 csi_dispatch 的 `'n'`/`'c'`/`'t'`
arm 里直接 push 响应字节。

JS 侧 `manager.ts::feed()` 在 `kernel.feed()` 之后立即 drain，复用现有的
`dataHandler` 把字节送回 PTY——PTY 看不出这是用户键盘输入还是 DSR 响应，
正好是协议要求的"和键盘同源"。

### 3.2 ECH / ICH / DCH 实现

三者都是行内 cell 操作，光标不动。`grid.rs` 加了对应方法：

```rust
pub fn erase_chars(&mut self, n: usize)   // ECH: 原位擦 n 个 cell
pub fn insert_chars(&mut self, n: usize)  // ICH: 插 n 空格、右 shift
pub fn delete_chars(&mut self, n: usize)  // DCH: 删 n cell、左 shift
```

ICH 的 shift 必须从右往左走（不然源 cell 会被覆盖），DCH 反之。三者都
显式 `pending_wrap = false`（xterm spec）。

### 3.3 ?1049 复合语义

xterm 的 `?1049` 不是单纯切屏，而是 `DECSC + ?1047h` 复合：
进入时 save primary cursor，退出时 restore。我们之前只切屏，所以
TUI 退出后 prompt 不知道该回到哪。

`ModeEffect` 拆出 `EnterAltScreenSaveCursor` / `LeaveAltScreenRestoreCursor`
两个新变体，`apply_mode_effect` 里：
- 进入：先在主屏 DECSC，再切 alt+clear，cursor 回 (0,0)
- 退出：先切回主屏，再 DECRC

`?47` 和 `?1047` 不变——它们语义上就不带 cursor 管理。

---

## 4. Pending 未实现 / 进一步工作

### 4.1 同步输出模式（`?2026`） — ✅ 已交付（2026-05-02 后续 patch）

[Contour/iTerm2 的提案](https://contour-terminal.org/vt-extensions/synchronized-output/)，
现在 lazygit、bottom、Ink 的某些版本都用：

```
CSI ? 2026 h    ; 开始同步更新（终端缓冲所有字节直到关闭）
[多次 cursor 移动 + 写入]
CSI ? 2026 l    ; 结束同步更新（终端原子 swap 到显示）
```

**已落实方案（manager-driven）**：

```rust
// modes.rs
pub struct Modes { /* ... */ pub sync_output: bool }
// `Modes::set` 中 2026 → flip sync_output（无 ModeEffect）

// lib.rs
#[wasm_bindgen(js_name = isSyncOutput)]
pub fn is_sync_output(&self) -> bool { self.inner.modes().sync_output }
```

```ts
// manager.ts rAF tick
const sync = entry.kernel.isSyncOutput();
if (sync) {
    if (entry.syncStart === null) entry.syncStart = now;
    if (now - entry.syncStart < SYNC_OUTPUT_TIMEOUT_MS /* 150ms */) {
        continue; // hold frame
    }
    // 超时落地 — 强制 render 一帧防卡死，但 syncStart 不重置
    // 避免在 stuck 状态下每帧 burst-render
} else if (entry.syncStart !== null) {
    entry.syncStart = null; // 干净退出，清状态
}
entry.handle.render(entry.kernel);
```

**关键决策**：timeout 放 JS 不放 Rust。理由：wasm 拿 monotonic 时钟要走
`web-sys::Performance` 增大依赖面，且这本来是个"渲染节流"决策——属于
manager 而非 kernel 的职责。kernel 只暴露 `is_sync_output(): bool`。

**测试**：modes.rs 加了 `synchronous_output_mode_2026_toggles` 单测覆盖
mode flip。78 通过 0 失败。

**遗留**：超时强制 render 后没退出 sync 会持续 burst（continue 不会再 hit
hold 分支因为 `now - syncStart` 一直 > 150ms）。当前实现选择"超时后每帧
都 render"，这是 acceptable degradation——TUI 即使忘了关 sync 也只是
渲染丢失原子性，不冻结。如果 burst 成本可见再考虑 cool-down。

### 4.2 REP `CSI <n> b` — ✅ 已交付（2026-05-02 后续 patch）

```rust
// terminal.rs
pub struct Terminal { /* ... */ last_printed: Option<(char, Attrs)> }

// parser.rs Performer::print
fn print(&mut self, c: char) {
    self.grid.print(c, *self.current_attrs);
    *self.last_printed = Some((c, *self.current_attrs));
}

// parser.rs csi_dispatch
'b' => {
    if let Some((ch, attrs)) = *self.last_printed {
        let n = first_param(params, 1);
        for _ in 0..n { self.grid.print(ch, attrs); }
    }
}
```

**测试**：`rep_repeats_last_printed_char` 覆盖显式 n 和默认 n=1 两种。
80 通过 0 失败。

**取舍**：last_printed 不在 LF/CR/erase/control 后清空——xterm 对此规范
没有完全约束（"REP after newline" 行为各家不同）。我们选保留，行为更
符合大多数实现（kitty / wezterm）。

### 4.3 DECSCUSR 光标形状 `CSI <n> SP q` — ✅ 已交付（2026-05-02 后续 patch）

```
0/1 → blinking block (default)
2   → steady block
3   → blinking underline
4   → steady underline
5   → blinking bar (vim insert mode)
6   → steady bar
```

**实现**：

```rust
// modes.rs
pub enum CursorShape { Block, Underline, Bar }
pub struct Modes { /* ... */ pub cursor_shape: CursorShape /* default Block */ }

// parser.rs csi_dispatch
'q' if intermediates.first() == Some(&b' ') => {
    let n = first_param(params, 0);
    let (shape, blink) = match n {
        0|1 => (Block, true),  2 => (Block, false),
        3 => (Underline, true), 4 => (Underline, false),
        5 => (Bar, true),       6 => (Bar, false),
        _ => (Block, true),
    };
    self.modes.cursor_shape = shape;
    self.modes.cursor_blink = blink;
}

// renderer.rs compute_cursor_draw — 用 modes.cursor_shape 映射到
// backend::CursorStyle（已支持 Block/Bar/Underline 三种）
```

**~~遗留~~ → ✅ 闪烁 cursor frame timer**（2026-05-02 后续 patch）：
`Renderer::tick` 多接 `now_ms: f64` 参数，由 `js_sys::Date::now()` 注入；
`((now_ms / 500.0) as i64).rem_euclid(2) == 1` 作为 blink_phase；phase 变
化时通过 `wrapping_add(1)` 给 cursor 行的 snapshot hash 加扰动 → 强制
该行 dirty → 重绘擦除/重画 cursor。Manager rAF 已经 16ms 一帧所以 phase
flip 在一帧内被检测到，不需要单独 timer。

**测试**：`decscusr_sets_cursor_shape_and_blink` 覆盖 4 个 sub-code +
out-of-range fallback。79 通过 0 失败。

### 4.4 OSC events 通道 — ✅ 已交付（2026-05-02 后续 patch）

实现落地：

```rust
// terminal.rs
#[derive(serde::Serialize)]
#[serde(tag = "type", content = "value")]
pub enum KernelEvent {
    TitleChanged(String),     // OSC 0/2
    IconNameChanged(String),  // OSC 1（独立保留）
    CwdChanged(String),       // OSC 7 file:// → 抽出 path
    HyperlinkOpen { id: Option<String>, uri: String },  // OSC 8 open
    HyperlinkClose,           // OSC 8 (空 URI)
    Bell,                     // BEL 0x07
}

pub struct Terminal { /* ... */ pending_events: Vec<KernelEvent> }
pub fn take_pending_events(&mut self) -> Vec<KernelEvent>
```

JS 端 `manager.ts::feed()` 在 drain `pending_response` 后再 drain
`takePendingEvents()`，分发给 `entry.eventHandler`。RidgePane.svelte
注册 handler，目前 `CwdChanged → setPaneCwd`；其它事件留 dev console
debug 占位，等 round 4-5 接 pane title bar / 链接 affordance / bell flash。

**已加测试**：osc_2 / osc_7（含 file:// path 抽取）/ osc_8 open+close /
BEL 共 4 个，cargo test --lib **77 通过 0 失败**。

**`FocusReport` 没放进 enum**——焦点事件方向是 JS → kernel → PTY，
不是 kernel → JS，逻辑上更适合放在 `manager.ts` 的 focus/blur 事件
监听处直接生成 `\x1b[I` / `\x1b[O` 字节并调 `dataHandler`，详见 §4.5。

### 4.5 焦点事件回送 — ✅ 已交付（2026-05-02 后续 patch）

```rust
// lib.rs
#[wasm_bindgen(js_name = isFocusReporting)]
pub fn is_focus_reporting(&self) -> bool { self.inner.modes().mouse_focus }
```

```ts
// manager.ts attach()
const focusListener = () => {
    const e = this.panes.get(paneId);
    if (e?.dataHandler && e.kernel.isFocusReporting()) {
        e.dataHandler(new TextEncoder().encode('\x1b[I'));
    }
};
const blurListener = /* same with '\x1b[O' */;
container.addEventListener('focusin', focusListener);
container.addEventListener('focusout', blurListener);

// detach() removes the listeners + clears entry handles
```

**为什么 focusin/focusout 而非 focus/blur**：focus/blur 不冒泡。当用户
点 canvas 而 tabIndex 在父 container 上时，focus 事件不会触发；focusin
冒泡能正确捕获到。

**为什么放 manager 不放 kernel**：`?1004` 是"用户 → terminal → app"方向
的事件流，事件源（DOM focus）在 JS 层。kernel 只暴露 mode bool 让 manager
判断要不要发字节。这避免在 wasm 里造一个"我要监听 DOM 事件"的耦合。

### 4.6 长行 resize reflow — Phase 1 ✅ 2026-05-03 / Phase 2 ⏳ 远期

Phase 1（live grid 主屏幕）已落地：`packages/ridge-term/src/term/grid.rs::reflow_primary` 在 `cols` 改变且 `!is_alt` 时 stitch wrapped 链 → 逻辑行 → 按 `new_cols` 重切片，cursor 偏移按逻辑位置迁移；alt 屏幕维持 truncate/pad（依赖 SIGWINCH 让 TUI 自己重画）。覆盖 10 条单测（详见 TASKS §2.3 Phase 1）。

Phase 2（scrollback reflow + selection / hyperlink 锚点跨 reflow 迁移）保留远期：要走 scrollback ring 同算法重排（4 MB 全量一次几十 ms 可接受）+ 锚点逻辑行 + offset 反推（避免 reflow 后选区 / 链接错位）。当前 scrollback 翻历史时长行仍按旧列宽显示。

---

## 5. 测试策略升级建议

当前 `cargo test --lib` 73 通过 0 失败，但都是单元测试。**风险**：
我们把 grid/parser 的单元行为测得很好，但**集成场景**（"PSReadLine 实际
跑起来 prompt 渲染对不对"）只能靠用户手动验证。

### 5.1 短期：scripted 字节流回归 — ✅ 已落地（2026-05-02）

实际实现路径稍微调整：用 cargo 标准的 `tests/` 集成测试目录，而非 fixture
文件。Helper 在 `tests/common/mod.rs`：

```rust
pub struct Snapshot {
    pub visible: Vec<String>,
    pub cursor: (usize, usize),
    pub scrollback_len: usize,
    pub pending_response: Vec<u8>,
    pub is_alt_screen: bool,
}

pub fn run_scenario(rows, cols, sb_lines, bytes: &[u8]) -> Snapshot;
pub fn run_chunks(rows, cols, sb_lines, chunks: &[&[u8]]) -> Snapshot;
```

`tests/protocol_smoke.rs` 7 个 realistic 场景：
1. `scenario_dsr_cpr_replies_after_content` — 原始 Ctrl+C bug repro
2. `scenario_psreadline_prompt_redraw_replaces_line` — `\r\x1b[K` 重写
3. `scenario_ink_frame_replace_via_cup_and_ed` — Ink frame N+1 redraw
4. `scenario_ech_clears_old_chars_in_place` — 字符残留 repro
5. `scenario_alt_screen_1049_preserves_primary` — ?1049 round-trip
6. `scenario_osc_8_persists_across_feed_chunks` — current_link 跨 feed
7. `scenario_ich_dch_combined_inline_edit` — cell-edit verb cooperation

`cargo test` 一键 run 全部 99 个（92 unit + 7 integration）。

**未来扩展**：用真实 `script -c` 录制字节流后转成 `&[u8]` 字面量塞进
新 test 文件。或者放 fixture 文件 + 用 `include_bytes!` 加载——先上现在
inline 形式因为最易读，fixture 文件等数据集大了再切换。

### 5.2 中期：headless wasm 集成测试

`wasm-pack test --node` 跑同样的字节流，验证 wasm bindgen 边界没 bug。

### 5.3 长期：对照 alacritty/wezterm

把同一段字节流喂给 alacritty 的 vte crate（我们已经依赖），对比 dump。
不一致 = 协议实现偏差。

---

## 6. 与 OVERVIEW round 路线图的对应

| Round | 原计划 | 本次新增 | 说明 |
|---|---|---|---|
| 2.4 | TerminalManager 接入 | + 协议补全 patch（DSR/DA/?1049/ECH/ICH/DCH/SCO/CSI t） | 必须在 round 2.4 验收前修，否则 PSReadLine + Ink 都用不了 |
| 4 | IME / 选择 / 搜索 | + DECSCUSR、`?2026`、`?1004` 焦点回送、REP | 都属"TUI 兼容性"范畴，和 round 4 同期做合适 |
| 5 | OSC 0/1/2/7/8 | + OSC events 通道架构（不只是数据，要事件） | OSC 解析是 1 天工作，事件通道是另 1 天 |
| 6 | parking lot 重写 | resize reflow 同期做 | 都涉及数据结构改动，一并处理避免反复改 |

---

## 7. 一句话总结

> TUI 局部刷新依赖一组"在原位修改 cell"的 CSI 动词（ECH/ICH/DCH）。
> ridge-term round 2.1 的"已实现 27 测试"漏掉了这一组——这就是
> claude code 字符残留 + 错行的根因。这一波 patch 把这组动词全补齐
> 并加了 5 个回归测试。剩下的 ?2026 / DECSCUSR / OSC events 排进
> round 4-5。
