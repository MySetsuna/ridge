# Resize 错位修复：inline-TUI（Claude Code 非全屏）历史行 reflow

> 2026-06-16 · 分支 develop · 紧接 `2026-06-15-resize-tui-redraw-fix` 的后续修复

## 现象（用户报告）

> pane resize 时，**没有开启 `CLAUDE_CODE_NO_FLICKER`** 的 Claude，仍然出现**错位、没有正常 reflow**。

`CLAUDE_CODE_NO_FLICKER=1`（即 `/tui fullscreen`）的 Claude resize 正常，不带该参数的 Claude resize 错位。

## 关键事实（调研结论）

### 1. `CLAUDE_CODE_NO_FLICKER` 决定渲染面 = alt vs 主屏

调研 Anthropic 文档与社区：`CLAUDE_CODE_NO_FLICKER` / `/tui fullscreen` 让 Claude Code
**在 alternate screen buffer 上绘制（像 vim/htop）**，只渲染可见内容。

- **开启**：alt 屏 → resize 命中 §1.22 alt-wipe（`grid.rs` `wipe_fired`）→ Claude 全量重画 → **正常**。
- **不开（默认）**：**inline 渲染在主屏**。会话历史/工具输出是**主屏永久内容**，只有底部输入框是 Ink 的 live frame。

### 2. Ink 默认渲染器对 resize 天然脆弱（Ink #907）

Ink 标准模式按 `lastOutput` 行数做 `eraseLines(N)` 增量重绘。窄化后旧帧 rewrap 占用**更多物理行**，
而 Ink 的 N 没变 → cursor-up-by-N 清不干净 → 顶部残留旧行。Ink 的 SIGWINCH 重绘**只重画自己的 frame 行**，
**从不重画上方历史**。这与团队 §3 注释里的实测一致：「Ink's diff redraw on SIGWINCH only re-emits the
input box's own rows」。

### 3. Windows 端（conhost / Windows Terminal）的做法

`ResizePseudoConsole` 后，conhost 的 `ResizeWithReflow` 会**对换行（wrapped）的行重新折行**；
Windows Terminal 也用同样方式 reflow 自己的 buffer。**「宽度变了就要把 wrapped 历史行重新折行」是 Windows
原生终端的标准行为**。但 conpty 的 viewport replay 不可靠（已知 issue：放大丢行、多次 resize 重复行、
从 home 只重印最新几行），所以不能纯靠 conpty replay 来 reflow 历史。

## 根因

`grid.rs::resize_with_inline_tui` 里：

```rust
let reflowed = cols_changed && !self.is_alt && !inline_tui_active && self.primary.cursor.row > 0;
```

`!inline_tui_active` —— **inline-TUI 路径完全跳过 reflow**。于是窄化时，输入框上方的会话历史只被
`naive_resize_screen` 按新列宽**截断（truncate）**，而非 **rewrap**，正是「没有正常 reflow」。

更糟：`2026-06-15` 的修复（`is_inline_tui_active_with_modes_at` 加 DECCKM/mouse 信号）让启发式
**成功识别**了默认模式 Claude → 把它从「shell 路径（会 reflow 历史）」**改道进了「inline 路径（不 reflow）」**。
所以那次修复**反而引入/放大**了本症状：

| 路径 | 历史 reflow | 输入框处理 |
|---|---|---|
| shell 路径（旧：默认 Claude 误判为此） | ✅ reflow `[0..cursor]` | cursor-below 清理（输入框上边框残留 ❌） |
| inline 路径（新：默认 Claude 命中此） | ❌ 跳过 | §A.3 整 frame wipe ✅ |
| **本次修复（合并两者）** | ✅ reflow `[0..frame_top)` | §A.3 wipe `[frame_top..]` ✅ |

## 修复

**在 inline-TUI 路径里，也对 frame 上方的历史行做 reflow**，frame 区仍照旧 wipe 交给 Ink 重画。

边界（live region 起点）按模式取：
- shell：`cursor.row`（prompt 行，既有行为，**字节不变**）。
- inline-TUI：`frame_top = last_abs_csi_row`（Ink frame 顶 = 最近一次绝对定位 CSI 的行）。

### 代码改动（仅 `packages/ridge-term/src/term/grid.rs`）

1. `reflow_primary_screen(old_cols, new_cols)` 抽成薄 wrapper，转调新的
   `reflow_primary_screen_at(old_cols, new_cols, boundary)`（把内部写死的 `cursor.row` 参数化为 `boundary`）。
   shell 路径传 `cursor.row` → 行为完全不变。
2. `resize_with_inline_tui`：
   - 计算 `inline_frame_top`（仅当 `inline_tui_active && last_abs_csi_at_ms != 0`，按 `old_rows` clamp）。
   - `reflow_boundary = if inline_tui_active { inline_frame_top } else { cursor.row }`。
   - `reflowed = cols_changed && !is_alt && reflow_boundary > 0` → 调 `reflow_primary_screen_at`。
   - `inline_tui_wipe` 的 `wipe_from_row`：reflow 跑过时取**重折后的** `cursor.row`（`reflow_primary_screen_at`
     已把 `cursor.row` 设为 `boundary + 行数 delta`）；没跑 reflow 时回退到原 `last_abs_csi_row` 锚点 / 整屏。

### 为什么只改 grid.rs 就够

`src-tauri/src/engine/parser.rs::resize` 在 `terminal.resize()` 后把 snapshot 清空再 `diff_into_frame()`，
即 resize 帧 = `Resize{rows,cols}` + **重折后整屏 Cells**（full reframe）。前端镜像收到精确的重折单元格，
无需自身再跑 reflow，两侧天然一致。无需任何 flag 管线改动。

### 安全性 / 影响面

- **shell 路径字节不变**：`reflow_boundary` 对 `!inline_tui_active` 恒为 `cursor.row`，等价旧 `reflow_primary_screen`。
- 仅 `inline_tui_active == true` 分支新增 reflow。
- 复用既有、已测的段落 reflow 逻辑（含 `saturating_sub` 防小窗口溢出 panic）。
- 既有测试 `inline_tui_resize_full_wipes_primary_visible_region`（`last_abs_csi_at_ms==0` → frame_top=0 →
  不 reflow → 整屏 wipe）与 `plain_primary_resize_skips_inline_tui_wipe`（shell 路径）行为不变。

## 验证

1. **Rust 单测**（确定性，本次新增）：构造「旧宽下 wrapped 的会话历史 + abs-CSI 标记 frame 顶 + inline frame」，
   窄化 resize（inline=true），断言：上方历史按新宽**重折**（非截断），frame 区被 wipe，cursor 落在重折历史下沿。
   修复前红、修复后绿。
2. `cargo test -p ridge-term --lib` 全绿（既有 354 + 新增）。
3. `cargo check`（src-tauri）0 警告；rebuild wasm 让镜像生效。
4. **真机/CDP**（运行时）：`localStorage.RIDGE_DIAG='1'`，不带 `CLAUDE_CODE_NO_FLICKER` 的 Claude 反复拖
   splitter，看 `__RIDGE_KERNEL.lastResizeDiags()` 的 `reflowed=true`、`inline_tui_wipe=true`，且会话历史视觉上
   正确 rewrap。

## 修复 B：后端权威 inline-TUI 判定（§resize-flag-authority，CDP 实测发现）

落地 A 后用 `pnpm tauri:dev:cdp` 真机验证时发现**更大的根因**：

**现象**：前端镜像（`JsTerminal`）是 **delta-only** —— 只 apply Cells/Cursor/Resize 等
delta，**从不解析原始 VT 字节**。CDP 实测镜像 `lastAbsCsiPosition()==null` 恒成立，故
`kernel.isInlineTuiMode()`（依赖 `last_abs_csi_at_ms`）在 delta 模式（现唯一模式）下**结构性恒为
false**。

**后果**：`manager.ts::fitPane` 把 `isInlineTui = kernel.isInlineTuiMode()`（恒 false）传给
`resize_pane`，于是后端 `resize_pane_inner` 的 `wipe_first = is_alt || is_inline_tui` 与
`skip_silence` 对**非 alt 的 inline TUI（默认 Claude）永不为真** —— **2026-06-15 的
wipe-before-SIGWINCH 顺序修复在生产中从未生效**（PTY resize 先于 wipe → SIGWINCH 抢跑 + 80ms
静默吞重绘）。这极可能才是用户「修了还错位」的主因。注意后端 `Terminal::resize` 内部的
reflow/wipe **内容分支**用的是后端自己的启发式（能看到原始字节，正确），所以**内容**对、但
**顺序/静默**错。

**修复**：`resize_pane_inner` 改从**权威后端 parser** 推导 `is_alt`/`is_inline_tui`（与执行 wipe
的是同一张栅格），不再信任恒 false 的前端 flag：

```rust
let (parser_is_alt, parser_is_inline_tui) = { /* lock parser, 读 pre-resize 快照 */
    p.is_alt_screen(), p.is_inline_tui_mode_at(now_ms) };
let is_alt = is_alt || parser_is_alt;            // OR 前端值，保留未来非 delta 路径
let is_inline_tui = is_inline_tui || parser_is_inline_tui;
let wipe_first = is_alt || is_inline_tui;
```

新增 `PaneParser::is_alt_screen()` / `is_inline_tui_mode_at(now_ms)`（薄委托 `Terminal`）。Shell
不命中启发式（无 cursor-hide+abs CSI）→ 行为不变。

**CDP 真机实证**（临时 `eprintln` 追踪，验后已删）：
```
parser_inline=false ... is_inline_tui=false wipe_first=false   ← shell resize
parser_alt=false parser_inline=TRUE is_inline_tui=TRUE wipe_first=TRUE rows=26 cols=18  ← inline-TUI 窄化
```
即前端 flag 为 false，但后端 parser 正确判定 inline 并启用 wipe-first —— 修复前此处会是
`wipe_first=false`（错位）。`cargo check`（ridge v0.0.5）0 警告；app 重建后正常运行。

## 残留 / 后续

- conpty viewport replay（skip_silence 下未丢）仍可能在 frame 区落少量字节，但 frame 区交给 Ink 重画，影响小。
- scrollback（已滚出可见区的历史）reflow 仍是既有的「deferred concern」，本次不动。
- 仍建议把 `/tui fullscreen`（alt 屏）作为最稳的 resize 体验推荐给用户。
