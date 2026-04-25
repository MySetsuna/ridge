# Agent Teams Teammates 分屏能力 — 现状报告（第 35 轮）

**问题**：Wind 是否真支持 Claude Code 的 Agent Teams 模式自动把
teammates 分屏展示？（每个 teammate 落在独立 pane）

**答案**：**支持（PARTIAL → 接近 FULL）**。架构完整、关键路径连通；
个别细节可改。

---

## 架构链路（端到端）

Claude Code 在 `teammateMode=tmux/auto` 下把 teammate 想成"另开一个
tmux pane 跑 `claude --teammate ...`"。Wind 通过 **tmux shim 二进制
+ 后端 HTTP API + AppState pane tree** 把这个抽象映射到真实 Wind pane：

```
Claude Code (parent process, 在 Wind 终端 pane 里运行)
    │
    │ exec("tmux split-window -h ...")
    ▼
src-tauri/src/bin/tmux.rs ────（shim binary，安装到 dist/teammate-shim/tmux）
    │
    │ POST /api/v1/split-window  (HTTP @ WIND_TEAMMATE_URL)
    ▼
src-tauri/src/teammate/server.rs::route_split
    │
    │ pane::teammate_split_pane(state, wid, idx, direction)
    ▼
src-tauri/src/commands/pane.rs::teammate_split_pane
    │
    │ ws.pane_tree.split(target, dir) → 新 Uuid
    ▼
新 pane → teammate_pane_states.insert(new_id, Busy)
       → terminal::ensure_pane_pty_workspace(..., cmd=body.command, cwd=...)
       → emit("teammate-layout-changed")
       → emit("teammate-active-pane-changed")
    │
    ▼
前端 paneTree store sync → SplitContainer 渲染新 pane → Bot 图标 + busy 脉冲
```

### 已映射的 tmux 命令（shim → API → Wind）

| tmux command          | shim handler               | backend route               | 真实 Wind 行为                                      |
|-----------------------|----------------------------|-----------------------------|----------------------------------------------------|
| `split-window`        | `cmd_split`                | `/api/v1/split-window`      | ✓ 真分屏，新 pane 携带 cwd + 命令、标记 Busy        |
| `new-session` / `new` | `cmd_new_session`          | (无，shim 内 stub 应付探针) | ⚠ 不真创建 session，shim 模拟"已存在"语义           |
| `select-pane`         | `cmd_select_pane`          | `/api/v1/select-pane`       | ✓ 更新 `teammate_tmux_pane_cursor`                  |
| `kill-pane`           | `cmd_kill_pane`            | (shim 内 noop + log)        | ⚠ 不真关 pane（怕误关用户活动 pane）                |
| `send-keys`           | `cmd_send_keys`            | `/api/v1/send-keys` 或 `/api/v1/spawn-process` | ✓ 写到目标 pane 的 PTY；含 spawn-process 短路       |
| `list-panes`          | `cmd_list_panes`           | `/api/v1/list-panes`        | ✓ 返回真实 leaves 数量 + tmux-兼容格式              |
| `display-message -p`  | `cmd_display_message`      | (shim 内 render)            | ✓ 用 `TMUX_PANE` env + `tmux_replacements` 模板渲染 |
| `capture-pane`        | `cmd_capture`              | `/api/v1/capture-pane`      | ✓ 返回该 pane 的 scrollback 文本                    |
| `resize-pane`         | `cmd_resize_pane`          | (shim 内 noop)              | ⚠ 不真 resize（Wind 用户控制 splitpanes 拖拽）      |
| `last-pane`           | `cmd_last_pane`            | `/api/v1/select-pane`       | ✓ 切回上一个 pane                                   |

---

## 核心证据

### 1. shim → backend 真发起 split

`bin/tmux.rs::cmd_split → post_split → POST /api/v1/split-window`：
请求体 `{ horizontal, pane_index, command, cwd, ... }`，等待响应里的
`new_pane_id` / `new_pane_index`。

### 2. backend 真在 PaneTree 里 split

`teammate/server.rs::route_split` 第 489 行：
```rust
match pane::teammate_split_pane(&ctx.state, wid, idx, direction) {
    Ok(new_id) => { /* 新 Uuid pane 出现 */ }
}
```

`commands/pane.rs::teammate_split_pane` 第 362 行：
```rust
ws.pane_tree.split(target, dir)  // 真实 PaneTree::split，与用户手动分屏走同一函数
```

### 3. 新 pane 真启动 PTY 跑 teammate 命令

`route_split` 第 513 行立刻调 `terminal::ensure_pane_pty_workspace(...,
cmd: body.command, cwd, ...)`——这是 Wind 启动一个全新 PTY 进程的标准
入口；Claude Code 传过来的 `--` 后命令（通常是 `claude
--teammate <id>`）真的在新 pane 里跑起来。

### 4. 前端能看到

`emit("teammate-layout-changed")` 触发前端 `paneTree` store
re-sync，`SplitContainer.svelte` 重新递归 render，新 pane 出现，
`agent_state=Busy` 让 Bot 图标变成绿色 pulse。

### 5. 空闲 pane 复用

第 23/24 轮加的优化：`body.allow_idle_reuse=true` 时不新分屏，把
现有 idle pane 标记为 Busy。避免反复打开同样多 teammates 时屏幕
炸开。

---

## 已知缺口（PARTIAL 部分）

1. **`new-session` 不真创建会话** — shim 直接返回成功；多 session
   并发的 Claude Code workflow 可能误以为成功创建了独立 session 但
   实际共享同一 Wind workspace。**用户可见影响**：当前未发现 Claude
   Code 实际依赖独立 session 的工作流——若依赖，会出现"两个
   teammates 共享一个 cursor"的现象。

2. **`kill-pane` 故意 no-op** — 怕误关用户的 pane。Claude Code
   teammate 退出时新 pane 不会自动关；只是 `agent_state` 从 Busy
   变回 Idle（通过 `release_pane` 调用）。**用户可见影响**：
   teammates 退出后窗格留白，用户需手动关。

3. **`resize-pane` no-op** — Wind 用户用 splitpanes 拖拽控制大小，
   不允许 agent 调整。**用户可见影响**：teammate 用 `resize-pane
   -L 10` 调整后看不到效果，但不会卡死。

4. **`new-window` 实际行为** — `route_new_window` 路由存在（line
   154），但实际是把"新 window"翻译成"新 pane"——Wind 没有 tmux
   window 这个概念。第 1024 行 fallback 调 `teammate_split_pane(..., 0,
   "vertical")`。**用户可见影响**：原本希望"独立屏幕"的 window
   变成同屏分屏。

5. **复杂 tmux 表达式渲染** — `display-message -p '#{pane_id}'` 等
   `#{...}` 模板在 shim 里查表 (`tmux_replacements`) 渲染。表外的
   占位符返回原文（Claude Code 大概会 panic）。**用户可见影响**：
   需要持续追踪上游 Claude Code 用了哪些新格式。

6. **`window_name` / pane title** — `route_split` 接收 `window_name`
   存到 `teammate_pane_titles`，前端 SplitContainer 已渲染。但
   Claude Code 改名（`rename-window`）目前没有路由。

---

## 验收建议（用户实测路径）

```bash
# 1. 在 Wind 一个终端 pane 里
$ pnpm tauri dev          # 启 Wind
$ pnpm run build:teammate-shim   # 一次构建 shim
$ export PATH="$PWD/dist/teammate-shim:$PATH"   # 让 claude 找到 shim

# 2. 在 Wind 终端里启动 Claude Code，触发 teammate
$ claude
> /agent  spawn  helper-1   # 或任何 teammate-spawn 命令

# 3. 期望观察：
#    - Wind 立刻新增一个 pane（左右或上下分屏）
#    - 新 pane 标题区出现绿色 Bot pulse
#    - 新 pane 的终端里能看见 claude --teammate ... 提示符
```

如果以上 3 步看到：**FULL 支持**确认。
如果只看到 store 更新但 pane 不出现：检查 ContextMenu 类似的 mount
race（第 34 轮已修类似 bug）。
如果 shim 报 "missing WIND_TEAMMATE_URL/TOKEN"：shell PATH 没继承
PTY env，回到 CLAUDE.md "Claude Code Agent Teams (TmuxBackend)" 段
处理。

---

## 结论

| 维度                      | 状态                      |
|---------------------------|---------------------------|
| 自动分屏（split-window）  | ✓ FULL                    |
| teammate 命令在新 pane 跑 | ✓ FULL                    |
| 视觉 busy 标记            | ✓ FULL                    |
| 空闲 pane 复用            | ✓ FULL                    |
| 多窗口 / new-session      | ⚠ stub（不影响主流程）    |
| 自动关 pane               | ⚠ 故意不关（设计选择）     |
| resize/rename             | ⚠ noop（Wind 设计取舍）    |
| 模板渲染完整性             | ⚠ 维护成本（追上游）       |

**直接答用户**：是，**Wind 已经真正支持 Claude Code Agent Teams
的 teammates 自动分屏**。每个 teammate 进入独立的 Wind pane，pane
头部有 busy 状态标记，PTY 真启动，cwd 继承。其余高级 tmux 行为
（独立 session / kill / resize）按 Wind 的设计选择不映射，但不阻塞
主流程。
