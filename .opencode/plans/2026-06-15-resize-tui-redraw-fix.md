# Pane Resize TUI Redraw Fix — Agent Handoff

## 现状

当 Claude Code（或任何 alt-screen TUI）在 wind 中 resize pane 时，如果子进程没有启用 `CLAUDE_CODE_NO_FLICKER`，画面会出现**字符错位、内容展示不全**。

具体现象：
- resize 后 TUI 内容行错位（offset rows and chars）
- 部分区域空白，部分区域残留旧尺寸的字符
- cursor 可能停在错误位置

## 根因分析

现有代码已经实现了完整的 resize 流程架构（见 `src/lib/terminal/manager.ts` 的 `fitPane` 方法、`packages/ridge-term/src/term/grid.rs` 的 `resize_with_inline_tui` 方法、`src-tauri/src/commands/terminal.rs` 的 `resize_pane_inner` 方法），但存在 **三个关键缺陷**：

### 缺陷 1：Backend resize 顺序反了（P0）

**文件**: `src-tauri/src/commands/terminal.rs:892` — `resize_pane_inner()`

当前执行顺序：
```
1. master.resize()   → ConPTY resize → SIGWINCH 发送给子进程
2. parser.resize()   → kernel 网格 resize + alt buffer 清空
```

**问题**：对 alt-screen TUI（如 Claude Code），SIGWINCH 在 kernel wipe 之前到达子进程。Claude Code 收到 SIGWINCH 后立即发出重绘字节，但这些字节到达 PTY reader 时，kernel 网格**还没有 resize**（还在旧尺寸），解析引擎把重绘内容写入了错误的位置。等第 2 步 `parser.resize()` 清空 alt buffer 时，Claude Code 的重绘已经被消费完了。

**正确顺序应该是**：
```
对 TUI 模式 (is_alt || is_inline_tui):
  parser.resize() → 先清空 alt buffer + resize kernel 网格
  master.resize() → 再发 SIGWINCH，此时画布已是空白

对 shell 模式:
  master.resize() → 先发 SIGWINCH（shell 重绘）
  parser.resize() → 再调整网格、清理 cursor 残影
```

### 缺陷 2：Inline-TUI heuristic 漏检 Claude Code（P0）

**文件**: `packages/ridge-term/src/term/grid.rs:396` — `is_inline_tui_active_at()`

当前 heuristic 要求 `cursor_visible == false` + 最近 2s 内有绝对定位 CSI。但：
- Claude Code 无 NO_FLICKER 时，resize 瞬间游标可能是可见的
- 可能有部分模式下没有绝对定位 CSI

**导致**：前端 `fitPane` 的 `isAlt || isInlineTui` 判断为 false → 退化为 shell 模式（只清理 cursor 行以下）→ 画面残留。

**修复方向**：
- 在 `is_inline_tui_active_at` 中增加 `app_cursor_keys (DECCKM)` 和 `mouse_reporting` 检测
- 前端 `fitPane` 增加 `forceTuiWipe` 标志做兜底

### 缺陷 3：前端 resizeHandler 传入的 TUI 标志可能不完整（P1）

**文件**: `src/lib/terminal/manager.ts:4255-4294`

当前 `fitPane` 的 TUI 检测只依赖 `isAltScreen()` 和 `isInlineTuiMode()`，没有考虑 `isAppCursorKeys()` 和 `isMouseReporting()` 作为 TUI 信号。

当 Claude Code 处于"刚启动还未切换 alt screen"或"inline TUI 模式间歇期"时，这两个检测都可能返回 false。

## 为什么现有的架构选择这样处理

wind 的 resize 架构基于几个关键决策：

1. **不要简单"清屏重绘"**：直接清屏会导致 flicker，对非 TUI shell（PSReadLine/cmd）会丢失用户已键入的文本。所以必须区分 TUI 模式和 shell 模式。

2. **TUI 模式下必须先 wipe 再 SIGWINCH**：因为 TUI 工具（Claude Code、vim、lazygit）使用 diff/增量渲染，不是每次 SIGWINCH 都会重绘所有单元格。如果画布不是空白，未更新的单元格会残留旧尺寸的内容。

3. **Shell 模式下必须先 SIGWINCH 再清理**：因为 PSReadLine/fish-zle 等 shell 在 SIGWINCH 后会发出 prompt 重绘，第 1 步 SIGWINCH 给了 shell 重绘机会，第 2 步 `cleared_below_cursor` 清理 shell 没覆盖到的残影。

4. **Resize-silence 窗口**：ConPTY 在 resize 后会 emit 整个 viewport 的 replay 字节（"垃圾"）。对 shell 模式开 80ms silence 窗口过滤垃圾；对 TUI 模式跳过 silence，因为 TUI 的重绘字节不能丢。

## 具体修改清单

### 修改 1：`resize_pane_inner` 调整 PTY 和 parser 的 resize 顺序

**文件**: `src-tauri/src/commands/terminal.rs`

核心改动：根据 `is_alt / is_inline_tui` 判断，交换 `master.resize()` 和 `parser.resize()` 的调用顺序。

```rust
// TUI 模式：先 resize parser（wipe alt buffer），再 resize PTY（发 SIGWINCH）
if is_alt || is_inline_tui {
    // Step 1: Parser resize (wipe alt buffer first)
    if let Some(parser) = &parser_for_delta {
        let mut p = parser.lock();
        let frame = p.resize(rows, cols);
        // 发送 delta frame...
    }
    // Step 2: PTY resize (SIGWINCH)
    master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
} else {
    // Shell 模式：先 PTY resize（SIGWINCH），再 parser resize
    master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
    if let Some(parser) = &parser_for_delta {
        let mut p = parser.lock();
        let frame = p.resize(rows, cols);
        // 发送 delta frame...
    }
}
```

注意：TUI 模式的 `master.resize()` 仍然需要根据 `skip_silence` 设置 silence 窗口。TUI 模式 `skip_silence = true`，所以 silence deadline 设 0。

### 修改 2：增强 `is_inline_tui_active_at` 的检测信号

**文件**: `packages/ridge-term/src/term/grid.rs`

在 `is_inline_tui_active_at` 中增加 DECCKM 和 mouse_reporting 检测，并将阈值放宽到 3s：

```rust
pub fn is_inline_tui_active_at(
    &self, 
    now_ms: i64, 
    cursor_visible: bool, 
    // 新增参数：接收当前 modes
    modes: &Modes,
) -> bool {
    // 现有检测：cursor hidden + 最近 abs CSI
    if !cursor_visible && self._recent_csi(now_ms, INLINE_TUI_DECAY_MS) {
        return true;
    }
    // 新增检测：app_cursor_keys (DECCKM) + mouse_reporting
    // 这些是更强的 TUI 信号，用更宽容的窗口
    if modes.app_cursor_keys || modes.mouse_normal 
        || modes.mouse_button_event || modes.mouse_any_event 
    {
        return self._recent_csi(now_ms, 3000); // 3s 窗口
    }
    false
}
```

同时修改 `Terminal::resize`（`terminal.rs:546`）和 `Grid` 内部调用点，传入 `self.modes`。

### 修改 3：前端 `fitPane` 增加 `forceTuiWipe` 标志

**文件**: `src/lib/terminal/manager.ts`

```typescript
const isAlt = entry.kernel.isAltScreen();
const isInlineTui = !isAlt && entry.kernel.isInlineTuiMode();
// 新增：兜底检测 — DECCKM 或 mouse_reporting 也是强 TUI 信号
const forceTuiWipe = !isAlt && !isInlineTui && (
    entry.kernel.isAppCursorKeys() || entry.kernel.isMouseReporting()
);
const wipeBeforePty = isAlt || isInlineTui || forceTuiWipe;
```

同时将 `forceTuiWipe` 通过 `resizeHandler` 传递给 backend，让 `resize_pane_inner` 也走 TUI 分支。

### 修改 4：增加第二段 forceFullRedraw 安全网

**文件**: `src/lib/terminal/manager.ts:4335-4341`

```typescript
// 第一阶段：150ms（现有）
setTimeout(() => { /* ... forceFullRedraw ... */ }, 150);
// 第二阶段：800ms 安全网 — 兜住 PTY reader 线程调度延迟
setTimeout(() => { /* ... forceFullRedraw ... */ }, 800);
```

## 验证方法

1. **手动测试**：在 wind 中打开 Claude Code（不设 `CLAUDE_CODE_NO_FLICKER`），反复拖拽 splitter resize，观察画面是否错位
2. **DIAG 日志**：通过 `localStorage.RIDGE_DIAG = '1'` 查看 `lastResizeDiags()` 确认 `wipe_fired` 是否 true
3. **单元测试**：在 `grid.rs` 测试中覆盖 `is_inline_tui_active_at` 的 DECCKM 分支
