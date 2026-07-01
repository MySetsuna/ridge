# 多主机外部终端 + 无头终端 ·「主机 / Hosts」侧边栏 设计 (2026-06-30)

## 0. 背景与动机 (Context)

两件事合并设计：

1. **重做被移除的「无头终端唤起」能力**。早先 `NativeSessionsPanel.svelte`（commit `0d56e94` 删除）+ 后并入 `GlobalStatusPanel` 的条件块提供过「发现/召唤本地无头 tmux 会话」。后端命令 `list_native_sessions` / `summon_native_session` 仍在（`src-tauri/src/commands/terminal.rs:1717+`），native 引擎在 `packages/ridge-tmux`。
2. **新能力：桌面端向外接入远端 ridge host 与 rdg host**，把它们的终端/无头终端通过拖拽停靠、菜单、已存在 pane 的右键菜单引入本地工作区。

**核心洞察 —— 三者是同一个抽象**：无头会话、远端 ridge 会话、rdg 会话，对本地工作区而言都是「PTY 不归本地工作区持有的 pane」。它们已有现成先例：领养 native 视图的 detach 语义（`kill_pty_if_present` 的 `native_ref` 分支，`terminal.rs:1421`）——关闭 = `set_attachment(None)` + 摘除布局树，**不写 exit、不杀子进程**。本设计把 `native_ref` 泛化为统一的 **Foreign Provider 引用**，三类共用一套数据模型、生命周期、菜单与标识。

**用户已确认的三项决策**：
- 范围：三类（headless / 远端 ridge / rdg）一起设计并实现，统一 provider 抽象。
- 新侧边栏 tab 名：**主机 / Hosts**。
- 在已存在的本地 pane 上右键接入外部终端时：**弹 dock 区域选择**（复用拖拽停靠的 left/right/top/bottom/center 预览）。

**预期结果**：一个「主机 / Hosts」侧边栏 tab 承载所有外部终端来源；外部/无头 pane 在工作区里有清晰标识；在工作区关闭这些 pane = detach（会话存活），只有在 Hosts tab 里才能真正终止。

---

## 1. 统一抽象：Foreign Terminal Provider

### 1.1 概念模型

```
Host（主机/来源）
 ├─ kind: headless-local | remote-ridge(LAN/cloud) | rdg
 ├─ id / label / 连接状态
 └─ Session（会话）*           ← provider 真正持有的 PTY
      ├─ id（provider 域内唯一）
      ├─ title / cwd / 运行状态
      └─ attachment: 当前被哪个 (workspace, pane) 领养，或 detached

WorkspacePane（工作区里的 pane）
 └─ origin: local | foreign{ hostId, sessionId }   ← foreign = 仅「视图/领养」，非持有者
```

- **本机（无头）** 始终作为一台特殊 Host 存在（`kind=headless-local`），其 Session 即本地无头 tmux 会话，无网络。
- 一个 Session 在同一时刻**至多一处 attachment**（单活领养）。在别处再领养 = 移动（或提示「已在工作区 X 打开，跳转?」）。

### 1.2 前端数据模型（`src/lib/types.ts`）

`PaneNode` leaf 增加可选 `origin`（默认 local，省略即本地，**零回归**）：

```ts
export type PaneOrigin =
  | { kind: 'local' }
  | { kind: 'headless'; hostId: string; sessionId: string }
  | { kind: 'remote';   hostId: string; hostLabel: string; sessionId: string }
  | { kind: 'rdg';      hostId: string; hostLabel: string; sessionId: string };

// leaf: 追加
origin?: PaneOrigin;
```

新 store 文件 `src/lib/stores/hosts.ts`：
```ts
interface HostSession { id: string; title?: string; cwd?: string; running: boolean;
                        attachedWorkspaceId?: string; attachedPaneId?: string; }
interface Host { id: string; kind: 'headless'|'remote'|'rdg'; label: string;
                 status: 'connected'|'connecting'|'disconnected'|'error'; sessions: HostSession[]; }
export const hostsStore = writable<Host[]>([]);   // 后端 host_list_snapshot 权威回填
```

### 1.3 后端数据模型（Rust）

`PtyHandle.native_ref: Option<(String, usize)>`（`engine/pty.rs:33`）泛化为：

```rust
pub enum ForeignRef {
    Headless { socket: String, global_id: usize },          // == 现 native_ref
    Remote   { host_id: HostId, remote_pane_id: String },   // LAN/cloud ridge
    Rdg      { host_id: HostId, remote_pane_id: String },
}
// PtyHandle:  foreign_ref: Option<ForeignRef>   (native_cancel 保留语义)
```

> 迁移策略：`Headless` 变体行为 == 现有 `native_ref`，把 `terminal.rs:1421` 的 native 分支改写成 `match foreign_ref`，headless 分支零行为变更，新增 remote/rdg 分支。`engine/pty.rs:132` 的 reader-EOF 快照同样泛化。

新后端状态（`state.rs`，`AppState`）：
```rust
hosts: RwLock<HashMap<HostId, HostConnection>>,
// HostConnection { kind, label, status, transport, capability_set, sessions: HashMap<RemotePaneId, SessionMeta> }
```

---

## 2. 后端架构：多主机代理 (Multi-host proxy)

**模型：本地 Rust 后端作为客户端向外连接，把远端 PTY 当本地 foreign pane 接管**。前端始终只和本地后端对话（`invoke` + delta channel），由后端做多路复用。这与无头 registry「后端持有」一致，避免前端 transport 全局 attach（现 web-remote 是整个 app 切成一个远端客户端，不适合多主机并存）。

### 2.1 三类 provider 的接管路径

| Provider | 连接 | PTY 来源 | 复用 |
|---|---|---|---|
| **headless-local** | 无（进程内） | `packages/ridge-tmux` registry | `summon_native_session` 既有路径 |
| **remote-ridge** | 出站 WS（LAN）/ WebRTC（cloud） | 远端 ridge `/ws` 上的 pane | `packages/ridge-cli` 的 mux/rpc 协议、`REMOTE_ALLOWLIST` |
| **rdg** | 出站 mux（127.0.0.1+token / TOTP 握手） | rdg host 的 pane | `ridge-cli` `protocol.rs`/`mux.rs`/`rpc.rs` |

远端两类共享一个 **Rust 侧出站客户端**（新模块 `src-tauri/src/hosts/`，或下沉到 `packages/ridge-core` 便于 rdg 复用）：
- **PTY 输出**：远端推 `0x10 pane_raw` 帧 → 后端按 `ForeignRef.remote_pane_id` 映射到本地 pane_id → 走既有 delta channel 推给前端（`register_pane_delta_channel`）。
- **输入/控制**：前端 `write_pty_bytes_workspace` / `resize_pane` → 后端识别 pane 为 foreign → 路由到对应 `HostConnection` 发 RPC（而非本地 PTY 写）。
- **能力门控**：远端 host 通过 `$/hello` 上报 capability（CLI 是 reduced set `["pane","fs","search"]`），前端据此灰置不支持的 pane 操作（沿用 §11.1 既有约定）。

### 2.2 关键不变量

- foreign pane 的本地 `terminals` 条目只是「视图句柄 + 路由信息」，**不持有真实子进程**。
- 真实会话存活在 provider 处（无头 registry / 远端 host）。本地断开/关闭仅影响视图。

---

## 3.「主机 / Hosts」侧边栏 tab UX

新增 tab，遵循既有侧边栏机制（`+page.svelte` 的 `SidebarTab` 联合类型 + 图标栏按钮 + 面板渲染块 1532-1584）。新组件 `src/lib/components/hosts/HostsPanel.svelte`。

```
┌─ 主机 / Hosts ──────────────────────[ + 连接主机 ][⟳]┐
│                                                       │
│  ▼ 🌙 本机（无头）                            3 会话   │   ← headless-local，始终存在
│      • build-watch        running · ~/wind   [接入][⋯] │
│      • dev-server         running · ~/api    [接入][⋯] │
│      • log-tail (已领养→工作区 2)  ●busy      [跳转][⋯] │
│                                                       │
│  ▼ 🌐 LAN · 192.168.1.5  (alias: 工位台机)   connected │   ← remote-ridge
│      • zsh                running           [接入][⋯]  │
│      • claude (detached)  running           [接入][⋯]  │
│                                                       │
│  ▼ 🖥 rdg · prod-box      connected                    │   ← rdg
│      • deploy             running           [接入][⋯]  │
│                                                       │
│  ▷ 🖥 rdg · staging       disconnected      [重连][⋯]  │
└───────────────────────────────────────────────────────┘
```

- **`+ 连接主机`** → 连接对话框（`HostConnectDialog.svelte`，复用 `RidgeDialog`）：
  - LAN ridge：`ip:port` + token（或扫描/粘贴 QR，复用 `RemotePanel` 的 LAN IP 探测 `detect_lan_ips`）。
  - rdg：地址 + TOTP（复用 `ridge-cli` 握手；类似 `rdg login`）。
  - cloud ridge：选已登录设备（复用 cloud 配对）。
- **`⟳`**：刷新会话快照（headless 复用既有 5s 轮询语义，远端按 host 推送）。
- **会话行**：状态点（running/detached/busy）+ 标识徽标 + 标题/cwd + 右侧行内动作 `[接入/跳转]` + `[⋯]` 菜单。
- **唯一真正关闭入口**：会话行 `[⋯]` → 「终止会话」（见 §6.3、§7）。

---

## 4. 标识 / 徽标 (Pane identification)

工作区 pane header 复用既有 pill 样式（`SplitContainer.svelte:565` 的 AGENT/STARTING 绿/琥珀胶囊），在 proc/cwd 之前插入 **来源徽标**（仅当 `origin && origin.kind !== 'local'`）：

| origin.kind | 文案 | 配色（沿用 token 风格） | 图标 |
|---|---|---|---|
| headless | `HEADLESS` | slate（`bg-slate-500/15 text-slate-300 border-slate-400/40`） | Moon / Ghost |
| remote | host alias（如 `工位台机`） | 蓝（`sky-500/15 …`） | Globe |
| rdg | host alias（如 `prod-box`） | 紫（`violet-500/15 …`） | ServerCog |

- 徽标 `title`（tooltip）：`来自 <host label> · 关闭仅断开，真正终止请在「主机」面板`。
- **关闭按钮语义变化**：foreign pane 的 header `×`（`SplitContainer.svelte:634`）改为 detach 语义图标（如 `Unplug/Eject`）+ tooltip「断开（会话继续运行）」，与本地 pane 的 `×`（真关闭）区分。Hosts tab 行内不显示工作区 `×`。
- Explorer/sidebar 的 pane chip 同步显示来源徽标（与 header 共用渲染助手，DRY）。

---

## 5. 拖拽停靠 (Drag-to-dock from Hosts tab)

从 Hosts tab 的会话行拖入工作区，复用既有停靠管线：
- 会话行 `use:` 一个轻量拖拽源 action（参照 `paneDockDrag.ts` 指针事件 + 阈值；但拖的是「会话」非「pane」）。新 store `hostSessionDragSource`（`{ hostId, sessionId }`）。
- 悬停工作区 pane 时复用 `paneDockResolve.regionAtPoint` + `paneDockHover` 预览（`SplitContainer.svelte` 叶级覆盖层已有方向半区预览，零改）。
- 落点 pointerup → 调 `attachForeignSession(hostId, sessionId, targetPaneId, region)`（见 §6.4）。

---

## 6. 菜单设计（用户的核心诉求）

四处菜单面，统一动词：**接入**（attach，把会话引入工作区）/ **断开**（detach，从工作区移除但保活）/ **终止**（terminate，真关闭）。

### 6.1 已存在 pane 的右键菜单 → 新增「接入终端 ▸」子菜单

在 `RidgePane.svelte`（终端右键菜单，`onContextMenu` 1546-1606，`ContextMenuItem` 支持 `children`/`icon`）的 Split 分组后插入一个分隔符 + 子菜单，**保持 Split Right/Down 简单不动**：

```
… 复制 / 粘贴 / 全选 / 清空
─────────
向右拆分 / 向下拆分
接入终端 ▸                          ← 新增子菜单（icon: PlugZap）
   ├ 新建无头终端                    ← 本机 headless 新建一个
   ├ ──────
   ├ 🌙 本机（无头） ▸               ← 各 host 子菜单，列其会话 + 「+ 新建终端」
   │     ├ + 新建终端
   │     ├ build-watch
   │     └ dev-server
   ├ 🌐 工位台机 ▸
   │     ├ + 新建终端
   │     └ zsh / claude…
   ├ 🖥 prod-box ▸
   └ ── 管理主机…（切到 Hosts tab）
─────────
切换 shell ▸ / 关闭面板
```

- 选中某会话/新建 → **弹 dock 区域选择**（§6.2），落点确定后接入。
- 子菜单数据来自 `hostsStore`（已连接主机 + 会话），无连接主机时只显示「新建无头终端」+「管理主机…」。

### 6.2 Dock 区域选择浮层（右键接入落点 = 用户决策）

复用拖拽停靠的视觉与解析，做成一个**轻量「区域选择」覆盖层**：
- 在目标 pane 上显示 left/right/top/bottom/center 五区高亮（复用 `dockRegionClass` + `paneDockHover` 预览块样式）。
- 鼠标移动高亮所在区，点击确认；Esc 取消。
- 新组件 `DockRegionPicker.svelte`（一次性覆盖层，pointer-driven，复用 `regionAtPoint`）。确认 → `attachForeignSession(..., region)`。

### 6.3 Hosts tab 菜单

**主机级 `[⋯]`**：
- 新建终端（在此主机起一个新会话）
- 重命名（设 alias）
- 重连 / 断开连接（仅断开 host 传输，不终止其会话）
- 忘记此主机（移除配置）

**会话级 `[⋯]`**：
- 接入到当前工作区（→ §6.2 dock 区域选择）/ 若已领养则「跳转到该 pane」
- 断开（detach，若当前已领养）
- 重命名
- ──────
- **终止会话**（terminate，**唯一真关闭**；危险项，红色 + 确认对话框「该会话及其进程将被真正结束，无法恢复」）

### 6.4 接入动作（前端 API，`src/lib/stores/hosts.ts`）

```ts
attachForeignSession(hostId, sessionId, targetPaneId, region): 
  → invoke('attach_foreign_session', {...})    // 后端：split targetPaneId 出新 pane，
                                                 //       PtyHandle.foreign_ref 指向该会话，路由建立
  → syncPaneLayoutFromBackend()                  // 回填 origin 徽标
newHeadlessSession() / newHostSession(hostId)    // provider 起新会话再 attach
detachForeignPane(paneId)                        // = 工作区关闭 foreign pane（§7）
terminateSession(hostId, sessionId)              // 真关闭（带确认）
```

---

## 7. 生命周期：断开 vs 真关闭（核心不变量）

| 触发 | foreign / headless pane 行为 | 本地 pane 行为（对照） |
|---|---|---|
| 工作区 pane header `×` | **detach**：摘除布局树 + 解除 attachment，会话保活；Hosts tab 标记为 detached | 真关闭 + 杀 PTY |
| 关闭整个工作区 | 其内所有 foreign pane 全 detach（不杀） | 杀各 PTY |
| App 退出 | headless 会话 tmux 存活；远端会话存活于 host | — |
| **Hosts tab → 会话 → 终止** | **真关闭**：provider 侧 kill（headless registry kill / 远端 `close_pane` RPC / rdg session kill）+ 若在领养则同时摘除 pane | — |
| 远端 host 断线 | 受影响 foreign pane 标记「连接丢失」遮罩，可重连恢复；**不**当作真关闭 | — |

**实现落点**：`close_pane`（`commands/pane.rs:548`）→ `kill_pty_if_present`（`terminal.rs:1421`）的 `match foreign_ref`：
- `Headless` 分支 == 现 native 分支（零变更）。
- `Remote/Rdg` 分支：发 detach（不发 `close_pane` 给远端）、解除路由、摘树、emit `detached`。
- 前端 `closePane`（`paneTree.ts:1575`）对 foreign pane 走 detach 文案/动效，不销毁 provider 会话。
- 单活领养：`attach_foreign_session` 若该会话已被别处领养 → 返回冲突，前端提示「跳转 / 移动」。

---

## 8. 安全 (Security)

- 出站 host 连接的 token/TOTP 种子安全存储（复用既有 cloud_http / auth.json / TOTP 下沉的做法），不硬编码、不进日志。
- 远端命令仍过 `REMOTE_ALLOWLIST` + `MUTATING_METHODS` 只读门（双向：我们作为客户端也只在允许集内调远端）。
- **终止会话**为危险不可逆操作 → 强制确认对话框（符合「hard-to-reverse 操作先确认」约束）。
- foreign pane 不得绕过本地 read-only 门控把写操作偷渡到远端。

---

## 9. 分阶段实现 (Build order，单一 cohesive 努力内的合理次序)

> 用户选「三类一起」：抽象一次设计到位；下面是**构建次序**（非「headless 优先再补」的功能分期）。每阶段独立 commit。

- **P0 · 抽象地基**：`ForeignRef` 泛化（`engine/pty.rs` + `terminal.rs:1421` match 改写，headless 零行为变更）；前端 `PaneOrigin` + `hosts.ts` store；pane header 来源徽标（§4）；`close_pane` detach 分流（§7）。TDD：headless detach 回归测试 + remote/rdg 分支单测。
- **P1 · Hosts tab + headless**：`HostsPanel.svelte` + `SidebarTab` 接线；本机（无头）host 复用 `list_native_sessions`/`summon_native_session`；新建无头会话；会话级「终止」打通 registry kill。端到端：无头会话 接入→detach→真终止。
- **P2 · 右键菜单 + dock 区域选择 + 拖拽**：§6.1 子菜单、§6.2 `DockRegionPicker`、§5 拖拽源。先用 headless 验证全交互闭环。
- **P3 · 远端 ridge host（LAN）**：`src-tauri/src/hosts/` 出站 WS 客户端 + 多路复用 PTY 路由 + capability 灰置；`HostConnectDialog` LAN 接入。
- **P4 · rdg host**：复用 P3 客户端 + rdg mux/TOTP 握手；cloud ridge 接入（如时间允许）。

---

## 10. 影响文件清单（代表性）

**前端**
- `src/lib/types.ts` — `PaneOrigin` + leaf `origin?`
- `src/lib/stores/hosts.ts` —（新）host/session store + attach/detach/terminate API
- `src/lib/components/hosts/HostsPanel.svelte` / `HostConnectDialog.svelte` —（新）
- `src/lib/components/hosts/DockRegionPicker.svelte` —（新，§6.2）
- `src/routes/+page.svelte` — `SidebarTab` 加 `'hosts'` + 图标栏按钮 + 面板渲染块
- `src/lib/components/SplitContainer.svelte` — header 来源徽标 + foreign pane `×`→detach 语义
- `src/lib/components/RidgePane.svelte` — 右键菜单「接入终端 ▸」子菜单
- `src/lib/stores/paneTree.ts` — `closePane` foreign 分流（detach 文案）
- 复用：`paneDockDrag.ts` / `paneDockResolve.ts` / `contextMenu.ts` / `ContextMenu.svelte` / `RemotePanel`（LAN IP 探测）

**后端**
- `src-tauri/src/engine/pty.rs` — `native_ref`→`foreign_ref: Option<ForeignRef>`；reader-EOF 快照泛化
- `src-tauri/src/commands/terminal.rs` — `kill_pty_if_present` match 改写；`summon_native_session` 复用；新增 `attach_foreign_session` / `host_*` 命令
- `src-tauri/src/state.rs` — `hosts` 注册表
- `src-tauri/src/hosts/`（新）— 出站 host 客户端 + 多路复用路由（P3/P4）
- `packages/ridge-tmux` — headless 会话 kill/list 复用
- 复用：`packages/ridge-cli` 的 `mux.rs`/`rpc.rs`/`protocol.rs`；`ridge_core::capability`
- capability 白名单：`packages/ridge-core/src/capability.rs` + 镜像 `src/lib/remote/cloud/remoteAllowlist.ts`（新增 host_* 命令）

---

## 11. 验证 (Verification)

- **后端单测**：`ForeignRef` 三分支的 detach vs terminate 行为；headless 分支回归（与现 native 行为字节级一致）；单活领养冲突。
- **前端单测（vitest）**：`hosts.ts` 的 attach/detach/terminate 状态机；origin 徽标渲染；dock 区域选择解析。
- **dev:cdp 真机 e2e**（沿用 `pnpm tauri:dev:cdp` + `scripts/cdp-*.mjs`，见记忆 [[env_cdp_dev_testing]] / [[feedback_self_verify_via_cdp]]）：
  1. 起本机无头会话 → Hosts tab 可见 → 接入工作区（dock 区域选择）→ 徽标正确。
  2. 工作区 `×` → 会话变 detached、仍在 Hosts tab → 再接入恢复。
  3. Hosts tab「终止」→ 会话消失 + 进程结束。
  4. （P3）LAN 接一台 ridge → 远端 pane 输入输出/resize 闭环 + 断线遮罩 + 重连。
- 后端改动需 rebuild 本地 ridge 运行时（会杀当前会话）→ 真机 e2e 由用户在 rebuild 后确认；dev:cdp 走调试实例不杀正式会话。
- 全绿门槛：`cargo check`/`clippy` 0 警告、`pnpm check` 0/0、相关 vitest 通过。

---

## 12. 开放问题 (待实现期确认)

- 远端 host 的会话「真终止」是否需要远端授权额外确认（远端可能多人共享）？倾向：终止远端会话时弹更强确认 + 显示 host label。
- detached 会话的 GC 策略：headless tmux 永久存活直至显式终止；远端会话由远端 host 决定，本地仅展示。是否提供「断开 host 时一并终止其本地领养 pane（仅 detach）」——是，仅 detach。
- 同一会话跨工作区「移动」vs「拒绝」：默认拒绝 + 提供「移动到此」动作。

---

## 13. 实现状态 (2026-07-01)

- **P0 · 抽象地基** ✅ `99255dc`：origin DTO（`PtyHandle.native_ref` 派生 headless）+ 前端 `PaneOrigin` + `paneOrigin.ts` 徽标 + SplitContainer 头部 HEADLESS/LAN/rdg 胶囊 + foreign pane `×` detach 语义提示。cargo/check 绿。
- **P1 · Hosts tab + headless** ✅ `5feee9f`：侧边栏「主机」tab + `HostsPanel` + `hosts.ts`；后端 `new_headless_session`（专用 `headless` socket）/`terminate_native_session`（唯一真关闭）注册进 invoke_handler + 远程 dispatch + 白名单（89→91，含 TS 镜像与计数测试）。
- **P2 · 菜单 + dock 选择 + 拖拽** ✅ `6b566b8`：RidgePane 右键「接入终端 ▸」子菜单 + `DockRegionPicker`（四向，无 center）+ `attachSessionAt`（summon+dock_pane 组合）+ `hostSessionDrag`（复用 SplitContainer 方向半区预览）。
- **P3/P4 · 远端/rdg 基础层** ✅ `3815518`：`PtyHandle.remote_ref`（additive）+ `PaneOriginDto::Remote/Rdg` 派生 + `crate::hosts` 注册表（`HostRegistry`/`HostRecord`，凭据不落库）+ `host_list_snapshot`/`connect_host`/`disconnect_host`/`forget_host`（桌面本地命令）+ `HostConnectDialog`（LAN/rdg 参数录入）+ HostsPanel 合并远端主机/忘记按钮。
- **⏳ 待下一里程（需 rebuild + 真实远端主机联调）**：远端/rdg 的 **live PTY 流传输** —— 本地 Rust 出站连接（LAN WS / rdg mux+TOTP）+ 把远端 pane 当本地 foreign pane 的 I/O 路由（write/resize→host 连接、远端 pane_raw→本地 delta channel）+ capability 灰置 + 断线重连。`remote_ref` 数据模型与 UI/命令面已就位，接线即可点亮。

> 验证边界：P0–P3/P4 基础层均 `cargo check -p ridge` 0 警告新增 + `pnpm check` 0/0 + 相关 vitest 绿。headless 全链路（列举/新建/接入/detach/终止）为纯前端复用既有后端命令，可经 `pnpm tauri:dev:cdp` 真机 e2e；后端新增命令（headless new/terminate、host 注册）需 rebuild 本地 ridge 运行时后由用户真机确认。
