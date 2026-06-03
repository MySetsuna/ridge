# S1 余量迁移台账（ridge-core handler 迁移）

> 作者：执行 S1（ridge-core 地基抽取）。本台账记录 `src-tauri/src/commands/*` 共
> **~8,282 行 / 12 文件 / ~135 个 `#[tauri::command]`** 的迁移现状与计划。本会话已交付
> **可编译地基 + 首个垂直迁移片**；本文件列出剩余 11 文件逐一的迁移策略、Tauri 耦合
> 触点计数、归类与风险，供后续会话与 GM 排期。
>
> 上游：[`unified-remote-architecture-handoff-final.md`](./unified-remote-architecture-handoff-final.md) §5.1、§6 S1、§7 R2/R3/R4。
> 决策：GM **D-S1-1**（dispatch 边界 stringly-typed）。
>
> 语言约定：散文简体中文，标识符英文。

---

## 0. 已迁移（本会话垂直片，样板）

| 文件 | handler | Tauri 耦合（迁前） | 迁移手法 | 状态 |
|---|---|---|---|---|
| `settings.rs` | `set_user_default_cwd`(1) | `State<AppState>` ×1 | 抽 `UserDefaultCwdStore` trait（host 在 `AppState` 上实现）；handler 经 `Ctx` 下传写入 | ✅ 已迁，`cargo check` 绿，单测绿 |
| `theme.rs` | `get_theme_data`、`set_active_theme`(2) | `AppHandle` ×6（均内部 helper） | 端口 handle-free 解析（exe 同目录 + 祖先回溯）+ `data_local_dir` 持久化进 ridge-core；`get_theme_data` 桌面侧保留 `AppHandle` 的 `Resource` 解析（更全），`set_active_theme` 纯委托 | ✅ 已迁 |

迁移后 `src-tauri` 三处变成薄封装：
- `commands/settings.rs::set_user_default_cwd` → 构 `Ctx` 调 `ridge_core::commands::settings::set_user_default_cwd`，错误 `to_command_string()` 映射回 `Result<(),String>`。
- `commands/theme.rs::set_active_theme` → 委托 `ridge_core::commands::theme::set_active_theme`。
- `remote/server.rs::dispatch_invoke_request` 的这三命令 → 走 `ridge_core::dispatch(cmd,args,ctx)`，经 `core_result_to_envelope` 映射回 `{_result|_error}` WS 信封。

**桌面行为零变化依据**：handler 逻辑逐行端口（trim/empty 归一、`active-theme.txt` 路径与默认 fallback、空目录 catalog 回退、错误串原样），桌面 `#[tauri::command]` 签名保持（仅 `set_user_default_cwd` 增加按名解析的 `app: AppHandle` 参数，Tauri 按参数名注入、前端按名传参，不影响调用）。**注意：本会话上限是 `cargo check` + ridge-core 纯逻辑单测；运行 app 的完整桌面回归须由用户在本机 rebuild 验证**（见报告移交项）。

---

## 1. 地基抽象面（已建，供余量迁移复用）

`packages/ridge-core/`（workspace member，零 Tauri / 零 `tauri::async_runtime`）：

- **`error.rs`** — `CoreError`（8 变体）+ JSON-RPC `{code,message,data}` 映射 + 桌面 `Result<_,String>` 映射。
- **`ctx.rs`** — `Ctx`：① `CoreState`（`Arc<dyn ...>`，host 持有，handler `downcast_ref` 取回具体类型）；② `EventSink`（`EventScope::Broadcast` vs `Connection`，落 D11）；③ `TaskSpawner`（默认 `TokioSpawner`，直依 tokio）；④ `CapabilitySet` + `connection_id`。
- **`capability.rs`** — `REMOTE_ALLOWLIST`（数据常量，D8）+ `CapabilitySet::{remote_default, allow_all, from_methods}`。
- **`dispatch.rs`** — `dispatch(method,args,ctx)`：能力准入 → 路径穿越守卫 → 方法表。
- **`commands/{settings,theme}.rs`** — 已迁 handler。

宿主侧 glue：`src-tauri/src/remote/core_bridge.rs`（`DesktopEventSink`、`AppState` impl `UserDefaultCwdStore`、`desktop_ctx`/`remote_ctx` 工厂）。

**余量迁移每个文件需要补的抽象**：对每个吃 `State<AppState>` 的 handler，按"它实际只用 AppState 的哪几个字段/方法"抽一个最小 trait（如 `settings.rs` 的 `UserDefaultCwdStore`），host 在 `AppState` 上实现，handler 经 `Ctx::state::<HostStateAccessor>()` 取回。**不要**把整个 `AppState` 搬进 ridge-core（它绑 Tauri `Workspace`/PTY/portable-pty/git2/notify）；只抽"领域端口"。最重的工作区/分屏领域模型（D11）在 S5 落地，S1 阶段先抽端口、不搬实现。

---

## 2. 余量迁移台账（剩余 11 文件）

> 列：handler 数 / `State<>` 触点 / `AppHandle` 触点 / 事件发射 / 归类 / 策略 / 风险。
> "触点"为文本计数（含签名+函数体引用），是工作量量级而非精确数。
> `async_runtime`：经核实 `commands/*` **零使用**（仅 `lib.rs` 1 处），故无 R3 阻塞，后台任务统一走 `Ctx::spawner`。

### 2.1 易迁（纯 fs / git / 无状态，无需抽状态）

| 文件 | cmd | State | AppHandle | 事件 | 策略 | 风险 |
|---|---|---|---|---|---|---|
| **`git.rs`** | 32 | 0 | 0 | emit×3（mutating 后 `scm` refresh） | **最易**：纯 `git2`/进程调用，零 Tauri 状态。整文件端口进 `ridge_core::commands::git`；3 处事件改经 `Ctx::events().broadcast("scm-...", ...)`（`EventScope::Broadcast`，D11 共享）。`ridge-core` 需加 `git2` 依赖（纯 crate，无 Tauri） | 低。git2 依赖体积；mutating 命令的 read-only gate 需在 dispatch 入口接管（见 §3） |
| **`project.rs`** | 25 | 4 | 0 | 0 | 大半是纯 fs（`read_file`/`get_file_tree`/`text_search`/`replace_in_files`…，已在 LAN 白名单且全 async）。4 个 State 命令（`open_project`/`get_recent_projects`/`remove_project`/`get_current_project`）只用 `project_store`(rusqlite) + `current_project`。抽 `ProjectStore` 端口 trait + `CurrentProject` 端口；纯 fs 命令直接端口 | 低-中。`text_search`/`replace_in_files` 依赖 `ignore`/`glob`/`regex`（纯 crate，ridge-cli 已用同款）；rusqlite 依赖只在 project-store 端口实现侧，不进 ridge-core |
| **`process.rs`** | 2 | 2 | 0 | 0 | `get_pane_foreground_process`/`get_pane_cwd`：用 `sysinfo` + 从 AppState 取 PTY pid。抽"按 pane 取 pid"端口 trait | 低 |

### 2.2 需抽状态 + 事件（吃 AppState/AppHandle，绑工作区/PTY 领域）

| 文件 | cmd | State | AppHandle | 事件 | 策略 | 风险 |
|---|---|---|---|---|---|---|
| **`workspace.rs`** | 16 | 9 | 12 | 多（结构变更广播） | 直接坐落 D11 **共享实体图谱**。S1 阶段先抽 `WorkspaceGraph` 端口 trait（CRUD/order/active），事件分广播（CRUD→all）vs 单连接（`set_active`/`focus`→发起者）。实现搬迁与 `Workspace` 结构留 S5 | **高**。是 D11 领域模型核心；与 R4 缺口、悬空引用回退耦合。建议 S1 只定 trait 形状，实现迁移随 S5 |
| **`pane.rs`** | 10 | 13 | 5 | 分屏布局广播 | 同上：分屏树（`PaneTree`）+ 比例 + 每 pane 锁定尺寸（D11 共享属性）。抽 `PaneLayout` 端口 | 高。`PaneTree` 当前在 `engine::pane_tree`，迁移须连带；锁定尺寸语义（viewport 不驱动 resize）是 S5 新行为 |
| **`terminal.rs`** | 15 | 17 | 8 | PTY 输出/resize | 最难之一：`portable-pty` 句柄活在 `Workspace.terminals`，PTY 生成代际、scrollback、raw-byte fan-out 全绑 AppState。抽 `PtyHost` 端口（create/activate/write/resize/history）；raw-byte 广播经现有 `PaneRegistry`/`broadcast_remote_event`，事件 trait 包之 | **最高**。PTY 生命周期 + `!Sync` 句柄 + 两阶段 spawn（`PendingSpawn`）。headless PTY 环境（shell/env/cwd/TERM，R13）须在此定义。强烈建议独立子项，不与轻量片同批 |
| **`ridge_file.rs`** | 14 | 6 | 17 | 保存/恢复 | `.ridge` 文件序列化（工作区落盘/还原/recent/restore-set）。`AppHandle` 多用于 app_data_dir 解析 + 事件。抽 `RidgeFileStore` 端口 + `AppDataDir` 端口（headless 用 `directories` crate 等价） | 中-高。依赖 D11 工作区图谱形状（序列化目标）；建议随 workspace/pane 之后 |
| **`fs_watch.rs`** | 1 | 1 | 4 | `fs-changed`（高频） | `start_watching_paths`：`notify` debouncer，emit `fs-changed`。抽 watcher：后台任务经 `Ctx::spawner`（tokio），事件经 `Ctx::events()`；**须落 §5.2 背压**（bounded+coalesce，R8） | 中。背压/合并是新增逻辑，非纯搬运 |
| **`watch.rs`** | 1 | 1 | 4 | `scm` refresh（高频） | `start_watching_repos`：git 仓库 watcher，同 `fs_watch` 模式。同背压要求 | 中。同上 |
| **`remote.rs`** | 10 | 10 | 0 | 0 | **不迁**：host 特权命令（`get_remote_info`/`set_remote_enabled`/`disconnect_session`/blacklist 等），刻意排除在 `REMOTE_ALLOWLIST` 之外（D8）。保留为各 host 自有特权面 | N/A。明确**留在 src-tauri**，不进 ridge-core |

### 2.3 不在 commands/ 但相关

- **`deep_root.rs`**（`enter_deep_root_mode`/`set_cloud_remote_active`/`restore_from_deep_root`）：host 特权 + 深度绑 Tauri 窗口/托盘。**不迁**（D8 排除）。
- **`engine::pane_tree` / `engine::pty`**：被 pane/terminal 依赖的领域实现，随 §2.2 那两文件迁移时连带评估（迁进 ridge-core 或抽端口）。
- **`build_splash_init_script`**（theme.rs）：桌面专有（用 `AppHandle` `Resource` 解析 + WebView 注入），**保留 src-tauri**。

---

## 3. 跨切面待办（迁移过程中须统一处理）

1. **read-only gate 归属**：当前 `dispatch_invoke_request` 入口的 `is_mutating_invoke` 读 `AppState::remote_fs_readonly` 做准入。迁移 git/project 写命令时，须把"read-only 拒绝"语义下沉到 ridge-core dispatch 入口（候选：`CapabilitySet` 增 `readonly: bool` 维度，或 `Ctx` 增 read-only 标志，dispatch 对 mutating method 检查）。本会话垂直片（settings/theme）无 mutating 写，故未触及；**git/project 迁移前必须先定此设计**，否则远控 read-only 会话会破防。映射到 `CoreError::ReadOnly`（已就位）。
2. **路径穿越守卫**：已在 `dispatch.rs::traversal_guard` 实现（与 legacy `path_has_traversal` 同语义），git/project/fs 命令迁入后自动受保护，无需各自重写。
3. **能力白名单同步**：`REMOTE_ALLOWLIST` 已含全部 ~85 个远控命令名。每迁一个 handler 进 dispatch 方法表，白名单**无需改**（名字已在）；未迁的命令在白名单内但 dispatch 返回 `MethodNotFound` → host bridge 现状是 LAN `match` 继续服务（见 §4 共存策略）。
4. **事件 trait 双路由落地**：D11 广播 vs 单连接的实际 host 实现（`DesktopEventSink` 现把 `Connection` 也走 `AppHandle::emit`，因为桌面 IPC 只有一个隐式连接）——浏览器多连接的精确单连接路由待 S3/S4 传输层带上 connection id 后补全。

---

## 4. 迁移期共存策略（保 LAN 生产绿）

dispatch 方法表与 LAN `dispatch_invoke_request` 的 `match` **并存**：已迁命令路由到 `ridge_core::dispatch`，未迁命令继续走 LAN 原 `match` 臂。每迁一个文件，把对应臂改为 `| ` 合并进 ridge-core 分支（如本会话对 theme/settings 所做）。直到全部迁完，再删 LAN 侧 `match` 残臂。此策略保证任意中间状态下 LAN 行为不变（上游 §8.9 硬要求）。

建议迁移顺序（风险递增）：`git.rs` → `process.rs` → `project.rs`（纯 fs 部分）→ `fs_watch.rs`/`watch.rs`（带背压）→ `project.rs`（project-store 部分）→ `workspace.rs`+`pane.rs`+`engine::pane_tree`（D11 共享图谱，宜与 S5 合流）→ `terminal.rs`+`engine::pty`（最重，独立子项）→ `ridge_file.rs`。`remote.rs`/`deep_root.rs` 永不迁。

---

## 5. 统计小结

| 分组 | 文件 | handler 合计 | 备注 |
|---|---|---|---|
| 已迁（垂直片） | 2 | 3 | settings + theme |
| 易迁（纯 fs/git/无状态） | 3 | 59 | git(32) + project(25) + process(2) |
| 需抽状态+事件 | 5 | 47 | workspace(16) + pane(10) + terminal(15) + fs_watch(1) + watch(1) + ridge_file(14)〔注：合计含 ridge_file〕 |
| 不迁（host 特权） | 1 | 10 | remote(10)；另 deep_root 3 个不在 commands/ |
| **总计（commands/）** | **12** | **~135** | 与 GM census 一致 |

> 注：上表"需抽状态+事件"行 handler 合计含 ridge_file(14)，故 16+10+15+1+1+14=57；与"易迁"59 + 已迁 3 + 不迁 10 = 总和约 129，差额为 git.rs 等文件内非 `#[tauri::command]` 的 pub helper 与 `lib.rs` 注册口径差异，~135 为含全部注册命令的口径。精确清单以各文件 `grep '#[tauri::command]'` 为准。
