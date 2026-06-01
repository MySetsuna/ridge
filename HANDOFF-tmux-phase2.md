# 交接提示词 — tmux native 引擎「GUI 召唤」Phase 2（继续 B）

> 临时交接文件。换设备后在新会话里**整段复制下面「提示词正文」**贴给 Claude 即可接手。
> 验证通过、Phase 2 收尾后可删除本文件。

---

## 提示词正文（复制以下全部）

我在 Wind(ridge)这个 Tauri+Svelte 终端模拟器里继续一个功能。请用中文回复,遵守仓库 CLAUDE.md(含 codegraph MCP 优先用于结构性查询)。项目根 `C:\code\wind`,后端在 `src-tauri/`。**先 `git pull`(develop 分支)拿到最新代码**,再用 codegraph/Read 摸清现状。

**背景**:我在给 ridge 的 tmux native 引擎加「把无头 native 会话召唤进 GUI 分屏围观」的能力。已完成两阶段并**编译通过**(`cargo check --lib --bin tmux` 干净),代码已提交并推送到 `origin/develop`(commit `433a97b` feat(tmux): native session engine, capture-pane, GUI summon)。现在要做 **task #12(继续 B)**,同时我会另跑验证 A 把结果贴给你。

**已落地(别重做,先摸清)**:
- 架构「方案 A」:native pane 仍是 PTY 唯一拥有者;reader 线程「存环(`ring`,供 capture)+ 广播(`output_tx`,供 GUI)」;GUI 召唤时建**领养 pane**——`PtyHandle` 共享 native 的 `writer/master`(都是 `Arc<parking_lot::Mutex<…>>`),`_child:None`、`native_ref:Some((socket,global_id))`、`native_cancel`;用 `native::BroadcastReader`(把广播包成阻塞 `Read`)喂现有 `spawn_pty_reader`,**复用与普通 pane 完全一致的 `pty-delta` 渲染/输入/resize 链路**。关闭召唤 pane = detach(`kill_pty_if_present` 里 `native_ref` 分支:不杀子进程,从 `pane_tree` 摘除 + emit `teammate-layout-changed`)。
- 触发器:shim `tmux attach`(`src-tauri/src/bin/tmux.rs::cmd_attach_session`)→ `POST /api/v1/tmux/summon`(带 `X-Ridge-Workspace`)→ 召进**发起方工作区**。
- 关键符号:`teammate/native.rs`(`summon`/`SummonPane`/`set_attachment`/`BroadcastReader`/`capture`/`finalize_capture`、`ring`/`output_tx`/`attachment`、writer/master 已改 `parking_lot::Mutex`)、`teammate/server.rs`(`summon_into_workspace`/`route_tmux_summon`/`/tmux/summon`、`route_tmux_capture`/`/tmux/capture-pane`)、`teammate/mod.rs`(`pub(crate) mod native;`)、`engine/pty.rs`(`PtyHandle._child:Option`、`native_ref`、`native_cancel`)、`commands/terminal.rs`(`kill_pty_if_present` detach 分支 + 构造 + `_child` Option 适配)、`commands/process.rs`(`_child.process_id()`→`as_ref().and_then`)、`bin/tmux.rs`(`cmd_capture` native 路由、`cmd_attach_session`→summon)。

**task #12 要做的(继续 B)**:
1. **前端 GUI 会话列表**(次要触发器):
   - 后端加 `GET /api/v1/tmux/list-all-sessions`:跨**所有自定义 socket**列 native 会话(socket / name / 窗口数 / pane 数 / 是否已 attach)。native.rs 加一个枚举所有 socket 会话的 fn。
   - 加一个 **Tauri 命令** `summon_native_session(socket, target)`(前端无 token,走命令而非 HTTP):它对**活动工作区**调用 summon 逻辑。可能需把 `server.rs::summon_into_workspace` 的核心抽成 `(state, app, socket, target, wid)` 签名以便命令复用(现在它依赖 `TeammateCtx`)。
   - 前端 Svelte:在**现有侧边栏**加一小块「native 会话」列表,每条一个「打开」按钮(已 attach 的标注),点击调上面的命令。先用 codegraph/Read 摸清现有侧边栏结构(参考 `src/lib/remote/RemotePanel.svelte` 及 workspace 侧栏组件)。范围克制,就是列表+打开按钮。
2. **kill / 自然退出 → 自动消视图**:当前 `tmux kill-session` 杀掉已召唤的 native pane 后,GUI 会留个空壳 pane(broadcast 关闭→`BroadcastReader` EOF→`spawn_pty_reader` EOF→`detach_terminal` 只删了 `terminals`、没删 `pane_tree` 叶)。要让 native pane 死亡时自动:关 `pane_tree` 叶 + emit `teammate-layout-changed`。两条思路任选:(a)`detach_terminal`(`engine/pty.rs`)对 `native_ref` pane 做完整移除;(b)`route_tmux_kill` 在调 `native::kill_*` 后,据返回的 attachments 移除对应 GUI pane。注意要拿到 AppHandle 才能 emit(`state.app_handle.get()`)。
3. **resize 打磨**:确认 `resize_pane` 对领养 pane(共享 master)正常;召唤初始尺寸=native 尺寸,前端 mount 时会 resize。

**验证 A(我另跑,会把结果贴给你)**:我重装 release 后跑——①验收脚本仍应 10/10;②`tmux -L planteam capture-pane -p -t %N` 能取回各面板屏;③在 Ridge 面板里 `tmux -L planteam attach -t plan-team` → plan-team 三面板应 split 进当前工作区、claude 可见可交互;关召唤 pane=detach 不杀。**最大未验证假设:领养 pane 在前端零改动下能否正确渲染**(走的是和普通 pane 相同的 `pty-delta` 路径,应该行)。我贴结果后:若渲染 OK 就继续 B;若渲染异常,排查 `delta_mode`/主循环 `PtyOutput` 臂(`src-tauri/src/lib.rs`)/`set_pane_delta_mode`/领养 pane 是否需预先开 delta、以及 `spawn_pty_reader`→主循环的 parser 喂入。

**关键约束/坑**:
- **无热重载**:当前跑的是安装版 release(`C:\Program Files\ridge\`),改后端要 `pnpm build:teammate-shim && pnpm tauri:build` 再装回重启(重启杀当前会话)。**开工先确认有没有 tauri dev 在跑**(有就别并行 `cargo check`;通常没有,可自由 `cargo check --lib --bin tmux` 验证)。
- 共享给 `PtyHandle` 的字段必须用 **`parking_lot::Mutex`**(不是 `std::sync::Mutex`),否则类型不匹配。
- `commands/terminal.rs` 用 **Tab 缩进**,Edit 易因空白不匹配失败——优先用**无前导空白的唯一子串**做锚点替换。
- **Tauri lib 单测在 Windows 无法作为独立 exe 运行**(`STATUS_ENTRYPOINT_NOT_FOUND`),纯逻辑靠 `cargo check` + shim bin 测试(`cargo test --bin tmux`)兜底。
- 仅 **git-bash** 支持(send-keys 写 `/tmp` 一致性);WSL 不支持。
- 每个功能点**单独 commit**(我自己写的代码 commit 末尾加 `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`),**只在我要求时**才提交;plan 不写手测清单。
- 验收脚本 `~/.claude/agent-team/ridge-tmux-acceptance.sh` 是回归门槛,每阶段后应仍 `FAIL=0 SKIP=0`。

请先用 codegraph 摸清现有侧边栏 + Tauri 命令注册 + `summon_into_workspace`,给我一个简短落地计划再动手。
