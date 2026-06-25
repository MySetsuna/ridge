# Teammate tmux 垫片：后端楔死下的快速失败 + 楔死诊断埋点

日期：2026-06-25
范围裁决：用户选 **1+3**（垫片快速失败 + 埋点复现定位根因；**不动**后端运行时线程模型）。

## 1. 现象

另一会话中 Claude harness 创建 teammate pane 失败：

```
Error: Failed to create teammate pane:
```

冒号后**原因为空**。harness 走的是 `tmux -S C:/code/wind/teammate.sock split-window …`，
即 ridge 自带的 tmux 垫片（`/c/Program Files/ridge/tmux`，伪装 `tmux 3.4`）。

## 2. 硬证据（垫片日志 `%TEMP%/tmux-shim.log`）

垫片对每次调用都写日志。失败时间戳：

```
post_split → posting http://127.0.0.1:60077/api/v1/split-window   @ 640.201
HTTP error: error sending request                                  @ 700.206   ← 精确 60.005s
exit subcommand=split-window status=err
```

- 间隔 = 垫片 `client()` 的 `.timeout(60s)`（`src-tauri/src/bin/tmux.rs`）正好触发。
- 端口**在监听**：localhost 上无人监听会**瞬间** connection refused，不会等满 60s。
- `list-sessions`（另一 endpoint、纯读）同样卡满 60s 才 err。

**结论：不是 “server unreachable / 引擎掉了”，而是后端 teammate HTTP server 整体被永久楔死。**
（这纠正了上一轮排查 “纯属宿主侧、与代码无关” 的不准确定性。）

## 3. 结构性根因（高置信）

teammate HTTP server 跑在 **`new_current_thread()` 单线程 Tokio 运行时**
（`src-tauri/src/teammate/server.rs:192`，注释写明 “有意塌成 1 条 worker 省内存”）。

单线程运行时**零故障隔离**：只要某个 handler 在那唯一 worker 上阻塞（竞争 `parking_lot`
锁、或把同步锁守卫跨 `.await` 持有后被另一请求争用 → 经典 current-thread 自死锁），
后续**所有**请求一律排不上，各自卡满垫片 60s 超时 → harness 报 `Failed to create teammate pane`。

### 诚实边界

- `route_split` 自身写得干净（锁都在 scope 内 drop，最坏只 `timeout(3s, ready_rx)`，看门狗 `tokio::spawn` 异步）。
- 日志显示该 split **根本没返回 504**（否则垫片 ~3s 就拿到响应），说明那唯一线程在这次 split **之前/之时就已被别的东西楔死**。
- **究竟哪条同步调用 / 哪把锁第一个卡住，单凭一份日志无法坐实** → 需要复现 + 埋点。
  因此本轮**不盲改**后端锁逻辑（遵守 systematic-debugging 的 “无确凿根因不修复”）。

## 4. 方案

### Commit 1 —— 垫片快速失败（已证实可放心修）

一刀切 60s 超时对**交互式控制命令**是错的：后端健康路径 <3.5s 返回，垫片却让 harness
干等满一分钟才失败，且冒号后无原因。

- `command_timeout(sub) -> Duration`（纯函数，可单测）：
  - `send-keys` / `send`：保留 **60s**（可能触发 GUI pane 的人审 HITL，审批期间不可误杀）。
  - 其余控制命令（split/list/display/capture/select/kill/…）：**10s**（后端自身最坏 ~3s 的 ~3× 余量）。
- `client()`：加 `connect_timeout(3s)`，并按 `command_timeout` 设总超时（经 `OnceLock`，沿用既有 `SOCKET` 模式）。
- `backend_error_message(is_timeout, is_connect, detail) -> String`（纯函数，可单测）：
  超时 / 连接失败各自给一行人类可读原因；`post_split` 失败时 `eprintln` 出来，
  让 harness 的 `Failed to create teammate pane:` 后**不再是空白**。

效果：后端楔死时垫片**秒级**失败 + 明确原因，而非冻结一分钟。
（注意：这不让 pane 创建重新**成功**——后端真卡住时只是更快更清晰地失败。）

### Commit 2 —— 楔死诊断埋点（option 3：先埋点复现，再定点根因）

目标：下次楔死能一眼看出**哪个 handler 进去了没出来**。

- teammate HTTP 加 axum 中间件：每个请求进入打 `>> {method} {path} #{req_id}`，
  退出打 `<< {path} #{req_id} {elapsed_ms}`。楔死时单线程连退出日志都打不出，
  于是日志里那条**只有 `>>` 没有 `<<`** 的请求即元凶。
- `route_split` 内补几个 checkpoint 日志（openpty 前/后、await `ready_rx` 前），
  把 “卡在 split 内部哪一步” 再缩一圈。
- 复现脚手架：扩展 `scripts/teammate-tmux-smoke.*`，反复打 split 并记录每次耗时，
  配合上面中间件日志，等下次楔死被捕获后再做针对性死锁修复（后续 commit）。

## 4b. 增补（用户追加裁决）：端点重发现，修掉「换端口后静默全断」

### 问题

teammate server panic 自重启时 `run_server` 走 `bind 127.0.0.1:0` → **换新 ephemeral 端口**，
并更新 `teammate_binding`。但**现存 shell 的 shim 环境变量 `RIDGE_TEAMMATE_URL` 还是旧端口**，
→ 所有 tmux 命令连不上（端口错了，连接重试也救不了），直到 shell 重启。这是 4/4a 修完后
唯一剩余的「静默全断」，且罕见但全断。

### 方案：sidecar 端点文件（两端可算出同一路径）

- **位置**：`temp_dir()/ridge-teammate-endpoint-<sanitize(socket_path)>.json`，
  内容 `{"url","token"}`。
  - `socket_path` = `$TMUX` 第一段（`<pane cwd>/teammate.sock`），后端在注入 `$TMUX` 时已知、
    shim 从自己的 `$TMUX` 也能拿到 → **同一字符串 → 同一文件名**。
  - **不写**在 socket 路径旁：那会在用户 repo 目录落文件、污染工作区/被误提交。
  - `sanitize` = 非 `[A-Za-z0-9]` 一律换 `_`（确定性，两端同实现）；按 socket 路径分键 →
    dev（`C:/code/wind`）与 release（别的 cwd）天然不撞，多工作区各一份。
- **后端**（`teammate/endpoint.rs` + 进程级全局 socket-path 集合）：
  - PTY spawn（`ensure_pane_pty_workspace` 的 `Some(bind)` arm）：按该 socket 路径写 sidecar
    （当前 {url,token}）+ 记下 socket 路径。
  - server (re)bind（`run_server` 写完 `teammate_binding` 后）：用新 {url,token} **刷新所有**
    已记的 sidecar。→ 重启换端口后，sidecar 立即指向新端口。
- **shim**（`tmux.rs`）：`send_retry` 最终以**连接错误**失败时置全局标志；`main` 把分发包成
  闭包，先用 env url/token 跑；若失败且标志置位 → 从 `$TMUX` 读 sidecar，**拿到不同的** url
  就用它**重跑一次**该命令（连接错误 = 请求从未送达 → 重跑无重复副作用）。无连接错误则零开销，
  不做任何探测。

### 安全

token 落 `temp_dir()`（用户私有目录，与既有 `tmux-shim.log` 同处）= 与「token 本就在 PTY env 里」
**同一信任边界**（本地单用户桌面），不新增暴露面。

- **不**把 teammate 运行时改多线程（option 2）——那是发布宿主依赖的引擎、风险更高，用户未选。
- **不**在未拿到确凿根因前改后端锁 / handler 的并发结构。
```