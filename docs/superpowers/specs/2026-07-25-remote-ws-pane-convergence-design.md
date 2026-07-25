# Remote WS/pane 服务腿收敛 + 手机渲染保活 — 设计

- **日期**: 2026-07-25
- **状态**: 设计定稿 → 分阶段实施（后端本迭代落地；前端 P4 视 app 可跑性）
- **关联**:
  - [[2026-07-02-rdg-remote-unify-and-fixes-design]]（后端 remote 统一到 `packages/ridge-remote`，已完成）
  - [[2026-07-16-remote-frontend-unify-and-mobile-keepalive-design]]（前端统一到 `@ridge/remote`；R1–R6 记录 P1/P2 落地、P3 折叠、P4/P5/P6 待做）
  - `packages/ridge-remote/src/host.rs`（`RemoteHost` trait 注释：`PaneProvider/InvokeDispatcher/EventBus` 「拆分留待 WS leg 逐步收口时再做」——**本稿即接此收口**）

---

## 0. 缘起（用户诉求）

手机端远控暴露两症：复制唤起键盘（已修，b5c5a56/07c0522）、TUI 鼠标控制丢失。追根后用户明确扩大范围：

> 不论公网 Remote 还是局域网 Remote，不管 Remote 的桌面浏览器 UI 还是真正的桌面 UI，如果没有全都统一代码、复用、把差异适配代码压到最小、高内聚低耦合，就在这个迭代一起做；这可能是大改，注意改彻底，避免日后返工。

## 1. 诚实现状盘点（git + 设计稿 R1–R6 为据）

### 1.1 已统一 ✅

| 层 | 现状 |
|---|---|
| 后端 crate | `src-tauri/src/remote/` 已删；桌面/rdg/云共用 `packages/ridge-remote`（`server_app`/`auth`/`tls`/`mdns`/`serve`/`ua` + `RemoteHost` 伞状 trait）|
| 前端传输 P1 | L1 `ChannelTransport`/L2 `RpcClient`/WS 原语/cloud/auth 全迁 `@ridge/remote/shared/{transport,cloud}`；LAN 腿与 cloud 腿都实现同一 `RemoteLink` |
| 前端终端 P2 | `manager.ts`（多 kernel + scissor + keep-alive）迁 `@ridge/remote/shared/terminal` + 端口注入（Settings/TermSettings/Themes/Workspace/Cwd）。**真桌面 UI 与桌面浏览器远控 UI 已共用同一 manager** |
| 前端 P3 | 折叠：`RemoteLink` 即统一点，MainApp 对协议无感 |

### 1.2 仍分叉 ❌（本稿目标）

**A. 后端每连接 pane 服务腿三份手抄**（`RemoteHost::serve_websocket` 未收口，`host.rs` 注释已承认）：

| 腿 | 位置 | resync 帧 | 现况 |
|---|---|---|---|
| 桌面 LAN | `src-tauri/src/remote_host_impl.rs::handle_ws`（~1300 行）| 初次 subscribe 仅发**裸 tail 无 RIS/模式**；仅 desync 分支发 `build_resync_frame` | 初次连长 TUI 仍可能丢模式 |
| 桌面 cloud | `src-tauri/src/commands/cloud_pane.rs` | 已用 `build_resync_frame`（cc457c1）| 注释自认与 LAN/rdg「**逐字一致手抄**」、常量「同名同值」|
| rdg LAN | `packages/ridge-cli/src/tui/lan_host_impl.rs::run_ws` | **仅发 16B 前缀+裸 backlog，无 RIS 无模式** | **rdg 无终端解析（`ScrollbackRing` 只存原始字节）→ 无从知晓 modes → 手机控 rdg TUI 丢鼠标之根** |

三腿各自手抄：`RemotePaneSub` 帧格式（16B pane-id 前缀）、`RESYNC_MIN_INTERVAL`(1s 限频)、`RAW_CHAN_CAP`(512)、resync scrollback 尺寸、desync→`build_resync_frame`→发送。**这正是「适配差异未压到最小」的实证。**

**B. 前端手机渲染仍单例 kernel**（P4 未做）：`src/remote/MainApp.svelte` 仍用 `terminalController.ts`(单例)+`paneScrollbackCache.ts`(旁路缓存)+`resetForSwitch`，未接共享 `manager.ts` 多 kernel 保活 → 切换白屏、scrollback 脆弱、与桌面渲染核心割裂。

## 2. 本迭代方案

### 2.1 后端收敛（本迭代落地，全程 `cargo check` + 单测可验）

> 判断：桌面 `handle_ws` 深耦 `AppState`（client registry / 事件广播 / desync 与 lib.rs fan-out 协同 / global-ws / data-request 限流 / invoke JSON-RPC），全量抽取共享 select 循环风险过高、边际收益低（协议已统一、前端已经 `RemoteLink` 归一）。**故收口聚焦真正分叉且致 bug 的「pane 帧格式 + resync 策略」——把三腿手抄的这份下沉为一份 SSOT，各腿 I/O 管道（mpsc/broadcast/Tauri-event 差异，真实不可消）保留但只调同一份帧构造。** 同时补 rdg 的 modes 追踪能力，令其 resync 与桌面同调。

**2.1.1 新增 `ridge_remote::pane`（SSOT）**

- `ridge-remote` 增依赖 `ridge-term`（`default-features=false`）+ `uuid`。
- 常量（消「同名同值」手抄）：`RESYNC_MIN_INTERVAL=1s`、`RAW_CHAN_CAP=512`、`RESYNC_SCROLLBACK_LAN=64KiB`、`RESYNC_SCROLLBACK_CLOUD=256KiB`。
- `pub fn pane_frame(pane_id: Uuid, payload: &[u8]) -> Vec<u8>`：16B pane-id 前缀 + 载荷（LAN live 帧）。
- `pub fn pane_resync_frame(pane_id: Uuid, scrollback: &[u8], modes: &Modes, alt: bool) -> Vec<u8>`：前缀 + `build_resync_frame`（RIS + 模式前导 + scrollback）——LAN resync 帧。
- cloud 腿无 16B 前缀（Tauri 事件名 `pane-raw-{pane}` 已携 pane id），继续直调 `build_resync_frame`（body SSOT 本就在 ridge-term）。
- 单测：前缀长度/内容、resync 帧含 RIS + `?1002h` 等。

**2.1.2 桌面三处采用**

- `remote_host_impl.rs::handle_ws`：初次 subscribe 改发 `pane_resync_frame`（新连内核为空 → RIS 无副作用，模式重连修长 TUI 丢鼠标）；desync 分支与 live 帧改用 `pane_frame`；常量引 `ridge_remote::pane`。
- `cloud_pane.rs`：`RESYNC_MIN_INTERVAL`/`RAW_CHAN_CAP`/scrollback 尺寸改引 `ridge_remote::pane`，删本地副本。

**2.1.3 rdg 补 live modes 追踪 —— 复用一份 `ridge_term` 原语（不另写）**

> 用户要求 live modes 亦只用一份代码。解法：**唯一的 modes 追踪实现就是 `ridge_term`**（`Terminal` 解析 `CSI ? h/l` → `Modes`，已是一份）。为让「喂字节 → 取模式快照」这层也不在 rdg 手抄，在 ridge-term 内加薄共享适配，两端同走一个原语：

- **ridge-term（一份 SSOT）**：
  - `Terminal::mode_snapshot(&self) -> (Modes, bool)` = `(*self.modes(), self.is_alt_screen())`（单一定义）。
  - `pub struct ModeTracker`：`{ term: Terminal }` + `new()`(`Terminal::new(24,80,0)`，仅追踪模式、0 scrollback、grid 尺寸与模式无关、内存可忽略) + `feed(&[u8])`（委托 `term.feed`）+ `snapshot()`（委托 `mode_snapshot`）+ `resize`。**零重复解析**——只是「仅需模式」宿主的复用外壳。
- **桌面**：`PaneParser::modes()`/`AppState::get_pane_modes` 改经同一 `Terminal::mode_snapshot`（消桌面侧 `(modes(), is_alt())` 手抄），全走一个原语。
- **rdg**：`ridge-cli` 增依赖 `ridge-term`；`workspace.rs::SessionHandle` 加 `modes: Arc<Mutex<ridge_term::ModeTracker>>`；create_session 的 writer task 每 PTY chunk 除 append ring + broadcast 外，另 `feed` 该 tracker（锁不跨 await）；`resize` 同步 tracker。加 `pub fn modes_snapshot(&self) -> (Modes, bool)` → `tracker.snapshot()`。
- `lan_host_impl.rs::run_ws` 的 `subscribe-pane`：backlog 非空时改发 `pane_resync_frame(pane_id, &backlog, &modes, alt)`；live 帧改 `pane_frame`。→ rdg resync 与桌面/cloud 完全同调，**修 rdg TUI 鼠标丢失**，且 modes 追踪与桌面共用 `ridge_term` 一份。

> 为何需 live 追踪而非 subscribe 时反解 backlog：`ScrollbackRing` 仅 256KiB，长 TUI 启动时的 `?1002h/?1049h` 早滑出窗口；唯有 feed 全量字节的 live 追踪才不漏。桌面 PaneParser 本就 feed 全量——rdg 经同一 `ModeTracker` 对齐此语义。

### 2.2 前端 P4（视 app 可跑性）

手机 `MainApp` 单例 → 共享 `manager` 多 kernel 保活；抽 `mobile/input/` 触屏/软键盘/IME 适配层。**保活行为（白屏消除/scrollback 保真/弱网不断连）无 headless 测试，须跑真 app（`tauri:dev:cdp`，R5.3 载非提权计划任务法）方能验。** 本环境可跑则实施并 CDP 验；不可跑则诚实标注为唯一 app-gated 遗留 + 交接配方，不盲改渲染核心。

## 3. 分阶段 + 门禁

| 阶段 | 内容 | 门禁 |
|---|---|---|
| 0 | 桌面 mode-reattach 基础修复 | 已提交 cc457c1（cargo check 绿）|
| 1 | `ridge_remote::pane` SSOT + 单测 | `cargo check -p ridge-remote` + `cargo test -p ridge-remote` |
| 2 | 桌面 handle_ws + cloud_pane 采用 SSOT | `cargo check`（src-tauri）；LAN/cloud 回归不破 |
| 3 | rdg modes 追踪 + 接 SSOT | `cargo check -p ridge-cli` + `cargo test -p ridge-cli` |
| 4 | 前端 P4 保活（视 app）| `svelte-check` + `vitest` + CDP 白屏/scrollback |
| 5 | 版本 0.1.1 + Release CI | `gh release view v0.1.1` 资产齐（CLAUDE.md Releases 硬规矩）|

每阶段单独 commit，可独立回滚。

## 4. 非目标 / 留待后续

- **不**全量抽取桌面 `handle_ws` 1300 行 select 循环入共享层（深耦 AppState、风险高、协议已统一、前端已归一 `RemoteLink`——边际收益低）。真正的 `PaneProvider/InvokeDispatcher/EventBus` 完整 trait 化仍为独立后续（承接 host.rs 注释），本稿只收口其中真正分叉且致 bug 的帧+resync 子集。
- **不**把 LAN 控制协议转 JSON-RPC（§前端 R6 已判：低收益高风险，可能永不必）。
- P5（手机壳 `src/remote/*` → `packages/remote/mobile/`）/P6（清理死路径）随 P4 之后。

## 5. 风险

| 风险 | 缓解 |
|---|---|
| 桌面初次 subscribe 改发 RIS 影响 desktop-browser 多 pane | 新连内核为空,RIS 为 no-op;各 pane 独立内核,均为 fresh。若实测异常则退回仅 mobile 单 pane 路径 |
| rdg per-session Terminal 内存 | `Terminal::new(24,80,0)` 仅模式追踪,单实例 ~2K cells,可忽略 |
| ridge-remote 新增 ridge-term 依赖 | `default-features=false`(同 src-tauri),不引入 wgpu/web-sys |
| P4 盲改渲染核心 | 严守「不跑 app 不改保活」;不可验即交接,不硬上 |
