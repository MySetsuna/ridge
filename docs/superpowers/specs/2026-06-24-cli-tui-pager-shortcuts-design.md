# CLI TUI 多会话切换 + 工作区快捷键 (Pager)

## 目标

为 `rdg` 交互式 TUI（E2 passthrough）添加：
1. **Ctrl+Shift+方向键**：快速切换 pane（会话）
2. **Ctrl+F1~F12**：快速切换工作区

## 现状

`packages/ridge-cli/src/tui/` 的 passthrough TUI 仅支持**单会话**：
- `run_local()` → 创建 1 个 `LocalPtySession` → `event_loop()` 透传全部按键编码
- `Workspace` 结构体存在，但只被 dashboard 用于 `session_count`
- `ridge-core` 有完整的 `WorkspaceGraph` + `PaneTree`，但 CLI 未使用

## 设计

### 1. 架构总览

```
run_local()
  │
  ├─ 旧路径：→ run_session()    (单会话透传，不变)
  │
  └─ 新路径：→ run_pager()      (多会话 Tab 式)
         │
         ├─ WorkspaceManager     (多个 Workspace，每个含多个 Session)
         ├─ draw_chrome()        (crossterm 写入状态栏，不含 ratatui)
         └─ event_loop()         (拦截 Ctrl+Shift+←→/Ctrl+F → 控制流)
```

### 2. 新增模块

**`pager.rs`** (~350 行)：多会话 TUI 主循环
- `Pager` 结构体：持有 `WorkspaceManager` + 输出任务句柄
- `run_pager()`：进入 alt screen → 设 scroll region (DECSTBM) → 事件循环
- `draw_chrome()`：用 crossterm 写 2 行状态栏（tab + 分隔线）
- 事件循环：拦截快捷键 → 切换 session/workspace → 更新状态栏

### 3. 状态栏渲染（crossterm 直接写入，不用 ratatui）

```
┌───────────────────────────────────────────────────────────┐
│  [1] project  [2] *logs*  [3] misc  │  Ctrl+Shift+←→     │
│───────────────────────────────────────────────────────────│
│  (PTY passthrough 输出区域 — 自然滚动)                     │
```

- 第 1 行：会话列表 + 快捷键提示，当前会话用 `*` 包围高亮
- 第 2 行：分隔线
- 使用 DECSTBM 保护状态栏不被 PTY 输出滚动覆盖

### 4. 快捷键映射

| 按键 | 功能 |
|---|---|
| `Ctrl+Shift+Left` | 上一个 pane（会话） |
| `Ctrl+Shift+Right` | 下一个 pane（会话） |
| `Ctrl+Shift+Up` | 跳到第一个 pane |
| `Ctrl+Shift+Down` | 跳到最后一个 pane |
| `Ctrl+F1`~`Ctrl+F12` | 切换到工作区 N |
| `Ctrl+]` | 退出 TUI（保持不变） |

### 5. 输出路由

每个 `SessionHandle` 有 `broadcast::Sender<Vec<u8>>`。Pager 的输出机制：

```
[Session 0] ──broadcast──┐
[Session 1] ──broadcast──┤
[Session 2] ──broadcast──┤
                         ▼
              output_task (spawned per active session)
                         │
                    mpsc::channel
                         │
                         ▼
               event_loop → stdout.write_all()
```

- 切换 session 时 abort 旧 output_task，spawn 新 task 读新 session 的 broadcast
- output_task 只转发活动 session 的输出，其他 session 的输出被 broadcast channel 自动丢弃

### 6. 滚屏保护

使用 DECSTBM + DECOM（Origin Mode）隔离状态栏区域：

```
Enter:
  ESC[3;{rows}r    ← scroll region 从第 3 行开始
  ESC[?6h           ← Origin Mode：光标坐标相对 scroll region

红绘状态栏:
  ESC[?6l           ← 临时关闭 Origin Mode
  MoveTo(0,0) + 写入 + MoveTo(0,1) + 写入
  ESC[?6h           ← 恢复 Origin Mode

退出:
  ESC[r             ← 重置 scroll region
  ESC[?6l           ← 关闭 Origin Mode
```

## 变更清单

| 文件 | 操作 | 行数 |
|---|---|---|
| `packages/ridge-cli/src/tui/pager.rs` | 新建 | ~350 |
| `packages/ridge-cli/src/tui/workspace.rs` | 修改：+ WorkspaceManager | ~60 |
| `packages/ridge-cli/src/tui/keymap.rs` | 修改：+ is_control_shortcut() | ~20 |
| `packages/ridge-cli/src/tui/mod.rs` | 修改：+ pager 模块 | ~10 |
| `packages/ridge-cli/src/main.rs` | 修改：+ --sessions 参数 | ~8 |
