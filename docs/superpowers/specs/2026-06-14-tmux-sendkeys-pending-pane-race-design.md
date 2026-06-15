# tmux `send-keys 400 Bad Request` —— PendingSpawn 注入竞态 修复设计

- 日期：2026-06-14
- 影响面：Claude Code `teammateMode: tmux` 起 teammate 分屏（`split-window → send-keys[spawn-process] → send-keys[Enter]`）间歇失败，宿主报 `Failed to send command to pane %N: tmux: send-keys 400 Bad Request`，导致 teammate 拉起被中止。

## 1. 现象与复现证据

`%TEMP%/tmux-shim.log` 两次真实序列（同一输入，结果不同）：

失败（前台 spawn，ts 1781445189–191）：
1. `split-window … -P -F '#{pane_id}'` → 200，`new_pane_index=1` ✓
2. `send-keys -t %1 -l -- "cd … env … claude.exe …"` → 结构化 launch → `spawn-process` → ok ✓
3. `send-keys -t %1 Enter` → **status=err**（即 400）

成功（后台 spawn，ts 1781445279–282）：同样 3 步，第 3 步 `send-keys -t %1 Enter` → ok。

→ 并非 `send-keys` 不受支持；而是**紧接 `spawn-process` 之后的 `send-keys -t %1 Enter` 间歇 400**，同输入约 90s 后可成功 = 竞态。

## 2. 根因（已定位到行）

链路：`tmux.exe`(shim, `src-tauri/src/bin/tmux.rs`) → GUI teammate HTTP 路由 `/api/v1/send-keys`（`src-tauri/src/teammate/server.rs:route_send_keys`）。

- `-t %1` 在 shim 中按 GUI 路径处理（`%`/`@`/`$` 前缀一律走 GUI），POST 到 GUI `/api/v1/send-keys`，失败时打印 `tmux: send-keys {status}` = `tmux: send-keys 400 Bad Request`（`bin/tmux.rs:1393`）。
- 服务端 `route_send_keys`（`server.rs:1064`）：
  - `pid = teammate_pane_uuid_at_index(wid, 1)`（`pane.rs:386`，按 `pane_tree` 叶子位置解析）→ Err 则 400（`server.rs:1090`）。
  - `write_pty_bytes_workspace(wid, pid, "\r")`（`terminal.rs:1364`）→ 只查 `ws.terminals`，无则 `PaneNotFound` → 400（`server.rs:1094`）。

关键：**两阶段 PTY 创建**（`terminal.rs:ensure_pane_pty_workspace`）。

- 阶段一（`spawn-process` 调用）：`openpty()` 后把记录塞进 **`ws.pending_spawns`**（`terminal.rs:581`），**不**写 `ws.terminals`、**不**启动子进程。
- 阶段二（`activate_pane_pty`，由**前端** xterm 容器尺寸稳定后调用）：`slave.spawn_command(...)` 启子进程，再 `ws.terminals.insert(...)`（`terminal.rs:767`）。

因此 `spawn-process` 返回 200 后、前端激活前，面板只在 `pending_spawns`。而 `write_pty_bytes_workspace` 只认 `terminals` → 此刻 `Enter` 必 `PaneNotFound` → **400**。激活恰好先发生（较慢/后台路径）时 `terminals` 已就绪 → 成功。这就是间歇性的来源。

`pane_tree` 叶子在 split 时即加入并贯穿 PendingSpawn，故 `teammate_pane_uuid_at_index` 解析稳定；实际 400 来自**写路径**（`PaneNotFound`），非索引解析。

## 3. 修复

PTY master 的 writer 在阶段一即存在：`PendingSpawn.writer: Arc<Mutex<Box<dyn Write + Send>>>`（`terminal.rs:557`/`state.rs`），与 `TerminalHandle.writer` 同型。

**改 `write_pty_bytes_workspace`：先查 `terminals`，无则回退 `pending_spawns[pane].writer`，再无才 `PaneNotFound`。**

- 写入 master，字节进 tty 输入队列，子进程启动后即被读取；对 agent 启动无害（claude.exe 由 argv 起，尾随 `\r` 仅是 stdin 一个换行，shim 注释亦如此说明 `bin/tmux.rs:1361`）。
- 两个调用方（`server.rs:675` idle-reuse 写命令、`server.rs:1092` send-keys）均为注入路径，回退普遍安全。
- 即便某些 ConPTY 边界下缓冲字节未抵达子进程，**也已修复宿主中止**：宿主因 400 中止；返回 200 即让其继续。

覆盖窗口：`spawn-process` 返回 → 前端激活前（本次观测失败的全部窗口）。

## 4. 残留子窗口（本次不改，复审待命）

`activate_pane_pty_state`（`terminal.rs:637`）：`pending_spawns.remove`(664) → `slave.spawn_command`(702，子进程 spawn 慢，约 100ms 级) → `terminals.insert`(767) 三步未持同一把锁 → 存在「两图皆无」的窗口；此时 `Enter` 仍会 400。

未在本次修复内消除，理由：①本次无法运行时验证（重建即杀本会话），对脆弱并发路径取最小改动；②观测到的失败是「激活前」整段窗口，非此子窗口。

可选硬化（已设计、待复审/需要时启用）：把激活改为**原地 `inner.lock().take()` 抽取**（保留记录于 `pending_spawns`，master/writer 持续可达），并在**同一把写锁内** `terminals.insert` + `pending_spawns.remove`（原子换手），消除「两图皆无」窗口。`ready_tx`（仅 native `route_split` 用，GUI auto_place 传 None）在换手时从记录取出。

## 5. 验证计划（需用户重建后）

服务端修复位于 `ridge_lib.dll`/`ridge.exe`；生效需：重建 → 覆盖安装到 `C:\Program Files\ridge\`（须先关闭运行中的 Ridge）→ 重启。**该步骤会终止当前会话，需用户执行。** `tmux.exe`(shim) 无需改动。

- 编译：`cargo check`（src-tauri）本地可验。
- 运行时：重启后再起 teammate 分屏，连续多次 `split → spawn-process → Enter` 应稳定 200；或脚本直连 `/api/v1/send-keys` 对刚 spawn 未激活的 pane 验证 200。
