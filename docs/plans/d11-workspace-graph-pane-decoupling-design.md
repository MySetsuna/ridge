# D11 落地设计 —— WorkspaceGraph / PaneTree 解耦

> 状态：**设计稿（未开工）**。本文件把 handoff（[`unified-remote-architecture-handoff-final.md`](./unified-remote-architecture-handoff-final.md) §4 D11 / §5.1）**已拍板的高层决策**（"共享实体图谱 + 每连接视图"）落成**具体的 Rust 模块/类型/API/迁移序列**。语言约定：散文简体中文，标识符英文（与代码库一致）。
> 前置：本设计建立在已完成的 `ridge-core` 迁移之上（git/process/fs/shell/PTY 解析层 + dispatch/capability/ctx/sandbox 缝；见 develop `4277ed6`、`057ed06`，与 [[project_ridge_core_tier1_migration]]）。
> 范围：**只设计，不改代码**。是否执行、按哪几相执行，交评审。

---

## 1. 目的与边界

把今天只活在桌面 `AppState` 里的**工作区 / 分屏 / pane 模型**抽成 `ridge-core` 的一份权威领域模型，使：

- `core + Tauri = 桌面`：桌面 `AppState` 不再自持 `PaneTree`，改为薄持 `WorkspaceGraph` 并委托所有 pane CRUD。
- `core + TUI/headless = ridge-cli`：无头 host 用**同一份** `WorkspaceGraph`（退化成单 workspace / 单 leaf）替代当前写死的 `CLI_PANE_ID` 常量，从而 S5 的"完整 IDE"能真正复用桌面分屏语义。

**不在本设计范围**（host 特权 / 渲染细节，永久留在各 host）：`PtyHandle`/`PtyBridge` 句柄本体、teammate/agent 生命周期、`.ridge` 持久化策略、native pane 领养、delta 渲染、scrollback 缓冲。下面会精确划线。

---

## 1.5 评审定稿与最终执行方案（2026-06-07，三视角评审后）

> 本节是**执行权威**。下文 §2–§12 为支撑分析；凡与本节冲突，以本节为准。本节并入了三路并行架构评审（sequencing / contract-consistency / regression-hunter）的结论与人类对三个开放问题的拍板。

### A. 已拍板的决策
1. **cli pane id（原 §3.1 / §11-Q1，关闭）**：**等 S3 全量切 graph-allocated Uuid**，cli 与 controller 随 S3 线协议**同步切换**。**删除**过渡期方案 A（UUIDv5+字符串别名）。后果：原"P4 在 S3 之后"改为 **P4 并入 S3 范围**（pane-id 切换发生在 S3 内部，不是 S3 的后继步骤）。
2. **排期**：D11 作为**一个 S5 子项集中排期**（单一 owner + 单一连贯设计），但**内部依赖是"杠铃"不是链**：P1+P2 只依赖已就位的 S1（`ridge-core`/`Ctx` 缝），S3-无关，可最先动；P4 绑 S3；P3 现在**设计**、实现**挪到 S4/S5**（首个真多连接消费者旁边）。
3. **PTY 句柄归属（原 §11-Q2，确认）**：图谱持 **pane 身份+布局+共享属性**；host 旁表持 `PtyHandle`/`PtyBridge` 句柄**按同一 pane id 对齐**。评审确认这是**唯一同时满足 D11 与 R3（zero-Tauri）的解读**，对契约忠实、不丢东西（D10 attach 快照的屏幕字节本就来自 host per-pane 缓冲，图谱只贡献 `locked_size`；PTY 输出广播本就 host→client 直走，图谱不在字节路径）。

### B. 核心模型修订（最 load-bearing 的两条 —— 初稿在此处是错的）
1. **锁模型 —— 图谱不得是独立第二把锁**。初稿设想 `Arc<RwLock<WorkspaceGraph>>` 与 host 旁表锁并存。评审（regression-hunter）证明这会**重新打开 `pty_generation` 守护本要关闭的 `[teardown, register)` 竞态窗口**，并制造 write-then-read 锁倒置死锁（parking_lot `RwLock` 非重入；LAN structural handler 会 `workspaces.read()`）。**定稿**：图谱状态持在 host **既有的同一把** `workspaces` 写锁之内（图谱是"锁下的数据"，自身不带锁）；`close`/`dock` 等事务由 host 在自己锁内**一把锁完成**（purge 旁表 + bump `pty_generation` + 图谱结构删，原子）；**所有事件一律在释放锁之后发**（对齐今天 `pane.rs:371` `drop(map)` 后 `try_send` 的安全序）。原 §11-Q3"单锁 vs per-workspace"是伪问题——真问题是"图谱锁 vs 旁表锁的序"，定稿用"同一把锁"消解之。
2. **事件归属 —— "合并为一条"是错的**。初稿 §7.1 说图谱 `close` 的 broadcast 与 `PaneClosed`"合并为一条"。评审证明二者语义不可合并：`PaneClosed`=PTY 生命周期信号（前端**重建 shell**，`lib.rs:420`）；`PaneTreeChanged`/`PanesChanged`=布局重渲染。**定稿**：图谱只发**结构事件**（tree-changed 类，`EventScope::Broadcast`）；host **保留** respawn-vs-suppress 决策——尤其 **native pane 必须抑制 `PaneClosed`**（`pty.rs:392` 的 `native_ref` carve-out，否则鬼影 shell）；普通 pane reader EOF **不调** `pane_tree.close`（只 `detach_terminal`），所以图谱-owns-emit 在那条路径**没有 hook**，`PaneClosed` 必须继续由 host 发。

### C. 事件 fan-out 的真实形态（contract 评审，HIGH）
今天一次 pane CRUD 在 `server.rs` 触发的是**三件不同的事**，不是一条：
- `{"type":"panes"}` 列表帧是从 **host 旁表**（`terminals` + `pending_spawns` + `handle.parser.lock().title()` live OSC title，`server.rs:1917-1954`）**重枚举**的——**图谱 broadcast 产生不了它**。定稿：图谱发结构事件后，**host 仍须重枚举 panes 帧**。
- 同时**移除** `server.rs` 旧的 `remote_structural_tx(PanesChanged)` + `event_tx(PaneTreeChanged)` 发射（`server.rs:1598-1603/1622-1627`）与图谱 broadcast **同步替换**，否则 double-emit。
- `pty-meta`/`pane-cwd-changed`（`server.rs:1886-1900`）由 PTY reader 的 `RemotePtyEvent::Metadata` 驱动 = **host 运行时态**，**不**并入图谱 CRUD broadcast（否则 cwd 双发）。

### D. serde 测试缺口（P1 前必补，regression-hunter HIGH）
落盘路径 = `RidgeFile.pane_tree: serde_json::Value`（`ridge_file.rs:333/509`），整棵 `PaneTree→PaneNode→Pane→PaneMode→SplitDirection` serde 表面都在磁盘上，且 `PaneNode`/`SplitDirection` 是 **externally-tagged**（`{"Leaf":"<uuid>"}` / `{"Split":{…}}` / `"Horizontal"`）。但现有 `pane_serde_roundtrip_*` 测试**只覆盖 `Pane`，没有覆盖 `PaneNode`/`PaneTree`/`SplitDirection`**——初稿 §9"已有测试守护"**夸大了**。定稿：**P1 移动前先补** `PaneNode`/`PaneTree`/`SplitDirection` 的 golden-string 测试 + 一个旧 `.ridge` fixture 反序列化测试；移动时**禁止**加任何 `#[serde(...)]` 标签属性（会改落盘 repr）。`locked_size` 若并入 `Pane` 必须 `#[serde(default, skip_serializing_if)]` + 仿 `pane_deserializes_missing_cwd_as_none` 加缺字段兼容测试。

### E. dispatch 命名 + 契约（contract 评审 F3/F4）
- dispatch key **必须用线协议/allowlist 名**：`get_pane_layout`/`get_pane_layout_for`/`split_pane`/`set_split_ratios_at_path`/`set_split_ratios_batch`/`dock_pane`/`close_pane`/`toggle_mode`/`resize_pane`/`create_workspace`/`close_workspace`/`rename_workspace`/`reorder_workspaces`——**不是**初稿里的内部短名（`layout`/`split`/`set_locked_size`/`dock`/`rename`/`remove`）。这些线名**已在 `REMOTE_ALLOWLIST`（`capability.rs:172-211`）且 `dispatch.rs:393-401` 标记为"allow-listed 但未迁→MethodNotFound"的预留槽**——P2 正好填进去。
- CRUD broadcast 通知的**方法名+payload schema** 与 `panes` 帧目前**不在协议 SSOT**（`docs/contracts/ridge-cloud-protocol.md`）。按"改契约在先"（R1），**先在 S0 补 §7/§9**，再落 P2/P4。

### F. dock_pane 跨工作区（初稿漏，likely breakage）
`pane.rs:260-326` 是最复杂的现存变更：detach 源树 + 迁 `Pane` 元数据/`PtyHandle`/`pane_sizes`/`teammate_pane_titles` + attach 目标树 + 源空则删工作区 + 改 active——**跨两工作区、两归属域**。定稿 §5.2 API 必须显式列出此操作，并在**单锁事务**内完成（承 B.1）。

### G. 验证现实（regression-hunter Q5）
0xc0000139 崩的是**桌面 cdylib** 测试二进制，**不**崩 `ridge-core`/`ridge-cli` 独立 crate。后果：
- **serde（D）**：强网 = `ridge-core` golden 测试（crash-免疫），P1 前补。**这是唯一有强自动网的 hazard**。
- **事件/锁/side-table（B/C/F）**：纯 core 测不出（策略在 host），只能 `ctx.rs:221` `RecordingEventSink`（断言图谱只发结构事件、不发 PTY 生命周期）+ **CDP e2e**（新 `scripts/cdp-pane-graph.mjs`：断言用户关 pane=**1 次**重建、native detach=**0 次**重建、并发 open→close→split 无孤儿 `terminals` 行）。本机桌面 e2e 恰最弱——所以 **P2 必须先建 S7 characterization/golden + `cdp-pane-graph.mjs`**，不能靠"应该没问题"。

### H. 修订后的执行序（critical path：S3 是最长杆）
- **Wave A（S1 性质，可先行，S3-无关）= 合并 P1+P2**：先补 serde golden（D）→ 移 `PaneTree`+`PaneMode` 入 `ridge_core::workspace`（`AppError→CoreError` 一次）→ `WorkspaceGraph` 持在 host **同一把** `workspaces` 锁内 → `commands/pane.rs` 12 命令薄壳（dispatch 用线名 E）→ 结构事件走 broadcast 且**释放锁后发**（B.1）→ host **保留** panes 重枚举 / `PaneClosed` / native 抑制（B.2/C）→ **移除** `server.rs` 旧 structural double-emit（C）→ dock 跨工作区单锁事务（F）。门：S7 golden + `cdp-pane-graph`（G）。
- **Wave B（随 S3）= P4**：S0 先补契约帧 schema（E）→ cli 单 leaf 图谱 + pane-id 全量切 Uuid（cli+controller 同步）。
- **Wave C（随 S4/S5）= P3**：`ViewRegistry` + 悬空引用回退（多连接才可观测/可测）。
- **最该守的风险**：把 `Pane.id` 的**落盘 serde**（Wave A 冻结、永不被 S3 动）与 **S3 线协议 pane-id**（S3 独有）**显式解耦**——二者都是 `Uuid`，看似可换实则不可，是单向门陷阱。

---

## 2. 现状解剖

### 2.1 桌面 `Workspace`（`src-tauri/src/state.rs`）逐字段分层

```rust
pub struct Workspace {
    pub pane_tree: PaneTree,                              // ← 共享图谱（移核）
    pub terminals: HashMap<Uuid, PtyHandle>,             // ← host 侧句柄表（按 pane id keyed，不移）
    pub teammate_tmux_pane_cursor: usize,                // ← teammate，桌面专有，不移
    pub teammate_pane_titles: HashMap<Uuid, String>,     // ← teammate，桌面专有，不移
    pub pane_sizes: HashMap<Uuid, (u16, u16)>,           // ← 含两义，见 §2.3
    pub last_pane_index: Option<usize>,                  // ← tmux last-pane，桌面专有，不移
    pub created_at: SystemTime,                           // ← workspace 元数据（可移核）
    pub teammate_pane_states: HashMap<Uuid, PaneState>,  // ← teammate，不移
    pub teammate_agent_pane_map: HashMap<String, Uuid>,  // ← teammate，不移
    pub pending_spawns: HashMap<Uuid, PendingSpawn>,     // ← 桌面两段式 spawn，不移
    pub pty_generation: HashMap<Uuid, u64>,              // ← 桌面 teardown/replace 防重入，不移
    pub associated_file_path: Option<PathBuf>,           // ← .ridge 持久化，桌面专有，不移
    // … teammate metrics 等
}
```

判据：**凡是按 `pane id` / `workspace id` keyed 的 host 侧旁表（PTY 句柄、teammate、generation、pending_spawn）都"不移"**——它们引用图谱的 id，但内容是 host 运行时态，desktop 与 cli 合理地不同。图谱只持**布局 + pane 元数据 + 共享属性（锁定尺寸）**。

### 2.2 `PaneTree`（`src-tauri/src/engine/pane_tree.rs`）—— 几乎纯

`PaneTree { root: PaneNode, panes: HashMap<Uuid, Pane> }`，全部是纯树算法（`split`/`close`/`resize`/`set_split_ratios_*`/`dock_pane`/`swap_leaves`/`detach`/`attach_external_leaf`/`get_all_leaves`）。外部依赖只有三处：

- `crate::types::PaneMode`（纯 enum：`Terminal | Editor{file_path,language}`）→ 随迁。
- `crate::utils::error::AppError`（`PaneNotFound(Uuid)` / `PtyError(String)`）→ 映射到 `CoreError`。
- `uuid::Uuid` + `Uuid::new_v4()` → `ridge-core` 需新增 `uuid` 依赖（纯 crate，桌面已用同版本，`Cargo.lock` 不分裂）。

**结论**：`PaneTree` 的可移植性与已迁的 parsers 同级——这是 Phase 1 的安全切片。

### 2.3 `pane_sizes` 的两义性（设计要厘清）

当前 `pane_sizes: HashMap<Uuid,(u16,u16)>` 同时承担两件事：(a) split-target 选择算法的**实测尺寸**输入；(b) 雏形的 per-pane 尺寸。D11 明确"**每 pane 锁定渲染尺寸**"是**共享属性**（任意 controller 显式 `resize` last-write-wins，viewport 不驱动）。设计取舍：

- 把**锁定渲染尺寸** `locked_size: Option<(u16,u16)>` 作为**共享属性**并入 `Pane`（进图谱、随 attach 快照 D10 下发、改动走 broadcast）。
- split-target 选择用的**实测尺寸**留在桌面 host 侧旁表（它是 GUI 布局产物，cli 无此概念）。
- 两者今天混在一个 map，迁移时**显式拆开**，避免把 GUI 实测尺寸误当共享属性广播。

### 2.4 cli 的 pane 现状（`packages/ridge-cli/src/session.rs`）

cli 是**单 pane host**：写死 `CLI_PANE_ID = "ridge-cli-pane"` + `CLI_WORKSPACE_ID` 两个**字符串常量**，controller 据此 `subscribe-pane`/`write_to_pty`/`resize_pane`、host 用它给 PTY 字节打 `0x10` 帧。**没有 `PaneTree`**。这正是要弥合的语义差。

---

## 3. 关键语义差异及统一策略

| 维度 | 桌面 | cli 现状 | 统一后 |
|---|---|---|---|
| pane 数量 | 多 pane 分屏树 | 单 pane | 同一 `PaneTree`；cli = **单 leaf 退化树** |
| pane id | `Uuid`（运行期生成） | 固定字符串 `"ridge-cli-pane"` | 见 §3.1 兼容方案 |
| 布局 | split/dock/ratios | 无 | cli 不发 split 命令即可（图谱支持但不用） |
| 锁定尺寸 | 雏形 | `resize_pane` 改 PTY | 共享属性，last-write-wins |

核心洞察：**桌面的多 pane 是一般态，cli 的单 pane 是退化态（root 即唯一 Leaf）**。同一个 `PaneTree` 模型覆盖两者——cli 只是从不调用 `split`。无需两套模型。

### 3.1 cli 固定 pane id 的兼容（一处真边角）

cli 当前对外暴露 `"ridge-cli-pane"` 字符串常量，controller 协议依赖它。图谱用 `Uuid`。两条路：

- **方案 A（推荐，最小破坏）**：cli 用一个**确定性派生的 Uuid**（如对常量串做 UUIDv5）种入单 leaf 图谱，对外仍可保留旧字符串别名直到 S3 统一协议切换。
- **方案 B**：等 S3 统一线协议把 pane id 全量改成图谱分配的 `Uuid`，cli 与 controller 同步切换。

本设计倾向 A 作过渡、B 作终态；**该选择需评审拍板**（属 §11 开放问题）。

---

## 4. 目标架构

### 4.1 新模块 `ridge_core::workspace`

```
ridge_core::workspace
├── pane_tree.rs   // 从 engine/pane_tree.rs 整文件移入（含全部测试）
├── mode.rs        // PaneMode（从 types.rs 移入或 re-home）
├── graph.rs       // WorkspaceGraph：workspaces 集合 + 锁定尺寸 + 名称/created_at
└── view.rs        // ViewState + ViewRegistry（每连接视图，keyed by ConnectionId）
```

### 4.2 三层归属（refine 自 handoff §5.1 表）

| 层 | 持有方 | 内容 |
|---|---|---|
| **共享图谱** `WorkspaceGraph` | `ridge-core`（host 经 `Ctx::CoreState` 持 `Arc<RwLock<…>>`） | `HashMap<WorkspaceId, WorkspaceMeta{ pane_tree: PaneTree, name, created_at }>`；`PaneTree` 内每 `Pane` 带 `mode` / `cwd` / `shell_kind` / `locked_size` |
| **host 侧旁表** | 各 host（desktop `AppState` / cli session） | PTY 句柄表（`HashMap<PaneId, PtyHandle|PtyBridge>`）、teammate 全家桶、`pty_generation`、`pending_spawns`、scrollback、split-target 实测尺寸、`.ridge` 路径 |
| **每连接视图** `ViewRegistry` | `ridge-core` | `HashMap<ConnectionId, ViewState{ active_workspace, focused_pane, scroll, selection, unsaved_buffers, theme }>` |

**关键解耦点**：图谱**不持 PTY 句柄**。handoff §5.1 写"PTY 句柄"属共享，但 `PtyHandle`（desktop：parser/delta_mode/native_ref/resize_silence）与 `PtyBridge`（cli）都**不可移植**。解法：图谱持有 pane 的**身份与布局**；PTY 句柄留在 host 旁表，**按图谱的 pane id keyed**。图谱负责"哪些 pane 存在、怎么排"，host 负责"这个 pane id 的 PTY 实体"。两者用 id 对齐，生命周期由图谱 CRUD 驱动（关 pane → 图谱删 → host 收到事件 → 关 PTY）。

### 4.3 复用已建成的 `Ctx` 事件路由（D11 的一半已就位）

`ridge_core::ctx` 已有 `ConnectionId = Option<String>` 与 `EventScope { Broadcast, Connection }`，且**注释明确写给 D11**：broadcast = pane CRUD / 布局 / 锁定尺寸 / PTY 输出；connection = focus / scroll / selection。**本设计不需要新建事件机制**，只需让图谱 CRUD 走 `events().broadcast(...)`、视图变更走 `events().emit(EventScope::Connection, conn, ...)`。这把"广播 vs 单连接"从今天散落在 `lib.rs`/`server.rs` 的手写逻辑收敛到一处。

---

## 5. 类型与 API 设计

### 5.1 移核类型（Phase 1）

- `PaneTree` / `PaneNode` / `Pane` / `SplitDirection` / `DockRegion` —— 整体移入 `ridge_core::workspace::pane_tree`，**保持 serde 表示逐字不变**（`.ridge` 持久化兼容是硬约束，见 §9）。
- `PaneMode` 移入 `ridge_core::workspace::mode`（或 `pane_tree` 内），serde 兼容。
- error：`PaneTree` 方法签名从 `Result<_, AppError>` 改 `Result<_, CoreError>`。映射：
  - `AppError::PaneNotFound(uuid)` → 新增 `CoreError::PaneNotFound(Uuid)`（或复用 `InvalidArgs`，但专用变体更清晰且利于 dispatch 的 JSON-RPC code 稳定）。
  - `AppError::PtyError(String)`（这里实为"非法 split path / 非 split 节点"等结构错误）→ `CoreError::InvalidArgs`。
  - 桌面薄壳把 `CoreError` 经既有 `to_command_string()` 映回 `Result<_, String>`，**Tauri 边界字节不变**。

### 5.2 `WorkspaceGraph`（Phase 2）

```rust
pub struct WorkspaceGraph { workspaces: HashMap<Uuid, WorkspaceMeta>, /* + active 默认策略 */ }
pub struct WorkspaceMeta { pub pane_tree: PaneTree, pub name: String, pub created_at: /* 注入时间，见下 */ }
```

API ≈ 今天 `commands/pane.rs` 的命令面（已是 `PaneTree` 方法，只是加 workspace 维度 + 事件）：

- `layout(ws) -> PaneNode`、`split(ws, target, dir) -> Uuid`、`set_ratios_at_path/batch`、`dock(ws, src, tgt, region)`、`close(ws, pane)`、`set_locked_size(ws, pane, cols, rows)`、`toggle_mode(ws, pane)`、`create_workspace/rename/remove`。
- 每个**变更**方法内部 `events().broadcast("pane-…", payload)`，使所有连接同步（替代今天桌面手写 emit + LAN `server.rs` 手写转发）。

**时间注入**：`created_at` 不能在 core 用 `SystemTime::now()` 吗？可以——core 是 host 运行时，不是确定性 workflow，`SystemTime::now()` 合法（与 `commands::process` 读时钟同理）。但若要可测，构造时由 host 注入更干净。设计取**注入**（`new_workspace(now: SystemTime)`），便于 golden 测试。

### 5.3 `ViewState` / `ViewRegistry`（Phase 3）

```rust
pub struct ViewState {
    pub active_workspace: Option<Uuid>,
    pub focused_pane: Option<Uuid>,
    pub scroll: /* per-pane scroll 偏移 */,
    pub selection: /* 编辑器/树选区 + 光标 */,
    pub unsaved_buffers: HashMap<PathBuf, String>, // 未落盘 buffer（每连接）
    pub theme: ThemeId,
}
pub struct ViewRegistry { views: HashMap<ConnectionId, ViewState> }
```

- 视图变更（focus/scroll/selection/theme）走 `EventScope::Connection`，只回发起连接。
- 桌面 in-process IPC：`ConnectionId = None`（单隐式连接），`ViewRegistry` 退化成单条——零行为变化。

---

## 6. 悬空引用 + 并发输入（handoff R4 点名的两条边角）

- **悬空引用回退**：图谱 `close(pane)` / `remove(workspace)` 必须扫 `ViewRegistry`，把任何 `focused_pane`/`active_workspace` 指向被删实体的连接自动回退（兄弟 leaf / 默认 workspace）+ 经 `EventScope::Connection` 通知该连接。设计为图谱删除 API 的**原子后置步骤**（删 + 回退在同一把写锁内），避免窗口期。
- **并发同 pane 输入**：输入按连接路由到各自 `focused_pane`；只有两连接**恰好聚焦同一 pane** 才字节交错。v1 取最省事：**接受交错**（或该 pane 单写者锁）。这条不属图谱本身，属输入派发层，本设计只标注归属，不展开。

---

## 7. host 接线

### 7.1 桌面

- `AppState` 去掉 `Workspace.pane_tree` 字段，改持 `Arc<RwLock<WorkspaceGraph>>`（或在 `Workspace` 内持 `WorkspaceMeta` 引用图谱）。
- 旁表（`terminals`/teammate/`pty_generation`/…）保持原样，仍按 pane id keyed。
- `commands/pane.rs` 的 12 个命令改薄壳：解析 → 调 `graph.<op>()` → 错误 `to_command_string()`。**行为零变化**。
- `engine::pty` reader 的 EOF 清理（`ws.pane_tree.close(pane)` + 旁表清理）改为调图谱 `close` + 本地旁表清理——注意图谱 `close` 现在会 broadcast，需确认与既有 `PaneClosed` 事件不重复（合并为一条）。

### 7.2 cli

- 删 `CLI_PANE_ID`/`CLI_WORKSPACE_ID` 写死常量，改为种一个单 workspace + 单 leaf 图谱（pane id 见 §3.1 方案 A）。
- cli 旁表只有一个 `PtyBridge`（按图谱 pane id keyed）。
- 为 S5"完整 IDE"，cli 后续可支持 split（图谱已支持）——但 MVP 不强求。

---

## 8. 分阶段迁移（每相 cargo + CDP 可验、行为零变化）

| 相 | 内容 | 验证 | 风险 |
|---|---|---|---|
| **P1** | `PaneTree`+`PaneMode` 整体移入 `ridge_core::workspace`（纯，含全部既有测试）；error 映射 `CoreError`；桌面 `engine::pane_tree`/`types::PaneMode` 改 re-export 薄壳；core 加 `uuid` 依赖 | `cargo test -p ridge-core`（pane_tree 既有 ~9 测试随迁即跑）；`cargo check` 0 警告；`pnpm cdp:pty` + 新增 CDP split/close/dock e2e | 低（与 parsers 同级纯移动）|
| **P2** | 引入 `WorkspaceGraph`（workspaces map + 锁定尺寸拆分）；桌面 `AppState` 委托 pane CRUD；图谱变更走 `EventScope::Broadcast`；合并重复 `PaneClosed` | CDP e2e：一个连接 split/close/resize → 断言广播到达；桌面回归（分屏开/关/拖拽/比例） | 中（动 AppState + 事件路径，需运行时验）|
| **P3** | `ViewRegistry`（每连接视图）+ 悬空引用回退；focus/scroll/selection/theme 走 `EventScope::Connection` | 多连接 CDP：A 删 pane → B 的 focus 回退 + 收到通知；A focus 不打扰 B | 中高（多连接语义，需多 controller e2e）|
| **P4** | cli 采用图谱（单 leaf）；§3.1 pane id 兼容；S3 统一协议对齐 | cli 单测 + 跨 host parity（S7 golden：desktop vs cli 同命令同结果）| 高（改 cli 协议，跨 repo controller）|

**P1 是立即可做的安全切片**（与刚完成的 PTY 解析层完全同性质）。P2+ 进入"动 AppState + 多连接语义"，建议每相独立评审 + 独立 CDP 验证。

---

## 9. 硬约束与验证

- **`.ridge` 持久化兼容**：`PaneTree`/`Pane`/`PaneMode` 的 serde 表示**逐字不变**（已有 `pane_serde_roundtrip_*` 测试守护；移核时这些测试随迁，是回归网）。`locked_size` 若并入 `Pane` 须 `#[serde(default, skip_serializing_if=...)]` 以兼容旧 `.ridge` 文件。
- **桌面回归**：动 `AppState`/`commands/pane.rs` 属 handoff R2 高风险区。验证手段：CDP e2e（新增 split/close/dock/resize 断言）+ S7 characterization/golden（采集现状 pane 命令 req/resp 回放打图谱）。**不靠"应该没问题"**。
- **CDP e2e 扩展**：现有 `scripts/cdp-pty-parsers.mjs` 模式可复制出 `cdp-pane-graph.mjs`：经 LAN WS 发 `split_pane`/`close_pane`/`dock_pane`/`set_pane_size`，断言 `panes` 列表 + 布局 + 广播事件符合预期，幂等（同 §"用 nonce 绕去重"经验）。
- **零 Tauri**：`ridge-core` 仍不得引 Tauri（R3）；`uuid` 是纯 crate，合规。

---

## 10. 范围边界（永久留 host，不进图谱）

`PtyHandle`/`PtyBridge` 句柄本体、parser/delta_mode/native_ref/resize_silence、teammate/agent 全家桶、`pty_generation`、`pending_spawns`、scrollback 缓冲、split-target 实测尺寸、`.ridge` 持久化策略、`reveal_in_file_manager` 等 GUI host 动作。图谱只认 **id + 布局 + 共享 pane 属性**。

---

## 11. 开放问题（需评审拍板）

1. **cli pane id 兼容**（§3.1）：过渡期方案 A（UUIDv5 派生 + 字符串别名）vs 直接 B（等 S3 全量切 Uuid）。
2. **PTY 句柄归属表述**：handoff §5.1 把"PTY 句柄"列为共享；本设计改述为"图谱持身份、host 旁表持句柄（按 id 对齐）"。需确认这与 handoff 意图一致（实质等价，只是澄清句柄不可移植）。
3. **`WorkspaceGraph` 锁粒度**：`Arc<RwLock<WorkspaceGraph>>` 单锁 vs per-workspace 锁。v1 倾向单锁（简单，pane 操作非热路径）；若实测争用再拆。
4. **是否纳入 `created_at` / workspace 元数据**：可移核，也可留 host。倾向移核（图谱自洽），但 `.ridge` 现有字段归属需对账。
5. **执行节奏**：P1 是否现在就做（安全切片），还是整个 D11 作为一个 S5 子项集中排期。

---

## 12. 一句话结论

D11 的高层决策（共享图谱 + 每连接视图）已定，事件路由层（`EventScope`/`ConnectionId`）已就位，`PaneTree` 本身几乎纯——**P1（PaneTree 移核）可作为下一刀立即安全落地**；P2–P4 进入 AppState/多连接语义，按相评审 + CDP/golden 验证推进。本设计把"要移什么、移到哪、API 长什么样、句柄怎么对齐、边角怎么处理、分几相验"全部钉死，供执行。
