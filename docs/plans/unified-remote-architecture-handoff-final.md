# 统一远控架构 + Cloud 桌面控制 —— 现状与规划（Handoff Plan · 最终稿）

> 状态：**规划中（未开工）**。本文件是给后续接手 agent 的单一交接文档：先讲清**现状**，再讲清**要做什么、按什么顺序做**。
> 作者轮次：2026-06-03 brainstorming 产出 → 2026-06-03 架构评审并入 → 2026-06-03 工作区多客户端模型定稿（D11）。语言约定：散文简体中文，标识符/字段名英文（与代码库一致）。
> 上游权威契约：[`docs/contracts/ridge-cloud-protocol.md`](../contracts/ridge-cloud-protocol.md)（Ridge Cloud 商业化协议 SSOT）。本文件**不覆盖**该契约；凡与之冲突处，以"改契约在先"为准（见 §6 风险 R1，以及新增的前置子项 **S0**）。

---

## 0.1 本稿说明：评审并入了什么（按功能点）

本次评审认为原 brainstorming 稿的主干（`ridge-core` 抽取、raw-byte 收敛、可插拔 Transport、S1 先行 + 桌面回归、单一 SSOT 契约 + "改契约在先"）是正确且符合最佳实践的，**未推翻任何一条已锁定决策**，只做加固与补缺。新增/强化清单如下，正文已就地整合：

| 触及功能点 | 变更摘要 | 落在 |
|---|---|---|
| **契约前置** | 新增 **S0：契约修订独立前置 track**，可立即并行启动，不被 S1/S2 阻塞（避免 S3/S6 被跨团队契约卡住成为关键路径瓶颈） | §6 S0、§8 |
| **协议 · RPC 信封** | 建议 invoke 通道改用 **JSON-RPC 2.0** 信封，取代自定义 `type`/`_reqId`（标准化、现成的错误/通知/批量语义） | §5.2、§6 S3 |
| **协议 · 版本握手** | 新增 **D9**：连接建立时协商 `protocolVersion + capabilities`（controller SPA 经公网下发可独立更新，必然与 host 版本漂移） | §4 D9、§5.2、§6 S3 |
| **协议 · 重连/迟到订阅** | 新增 **D10**：attach/reattach 必须先下发**当前屏幕快照**（raw-byte 不可重放 → host 侧维护 per-pane 屏幕缓冲）；这也是后续 cloud 腿上选配"屏幕状态同步"的地基 | §4 D10、§5.2、§6 S4、§7 R11 |
| **协议 · RPC 生命周期** | 补 request 超时 / `cancel(id)` / 重连后 in-flight 一律 reject + 重订阅 + 重拉 snapshot | §5.2、§6 S3 |
| **协议 · 背压** | 补高频事件（fs-changed/scm refresh）host 侧 **bounded + coalesce**，防止再次无界缓冲 OOM（与 raw-byte 当初要解决的同类问题） | §5.2、§7 R8 |
| **协议 · 物理通道** | D5 加注：单通道 1-byte mux 是 v1 选择，代价是 PTY 洪峰 **head-of-line 阻塞**同通道 JSON（cloud 高延迟放大）；备选拆多 DataChannel，列为可延后决策 | §4 D5 注、§7 R7、§9 |
| **Transport 分层** | 新增 **D7**：把 reqId 关联/超时/取消/重连**收敛到 Transport 之上的共享 RPC 层写一次**，适配器只实现通道原语，杜绝按适配器各写一遍导致漂移 | §4 D7、§5.3、§6 S2 |
| **安全 · 白名单下沉** | 新增 **D8**：命令准入白名单**作为数据下沉进 `ridge-core::dispatch` 策略层**，三 host 共用同一份执行，杜绝按 host 重写白名单造成的提权 | §4 D8、§5.4、§5.6、§6 S1 |
| **安全 · fs 沙箱** | 补工作区根沙箱 / root-scoping，尤其 cloud headless 暴露整机 fs；deep-root（🌱 扩权）需独立强鉴权 + 审计 | §5.6、§7 R10 |
| **安全 · E2EE 密钥认证** | 强调 X25519 公钥必须与设备配对身份绑定校验，否则 signaling/cloud 后端可 MITM，"E2EE"不成立 | §5.5、§5.6、§7 R10 |
| **安全 · shim 全量审计** | 复用整个桌面 SPA ⇒ 必须审计 SPA 触达的**全部** `@tauri-apps/api` 调用点，逐一隧道/桩/远控降级，否则远控模式运行时报错 | §5.5、§7 R12 |
| **headless 专有缺口** | 补 git 凭据来源（无 GUI 机器）与 PTY 环境（shell/env/cwd/TERM）两处现实缺口 | §5.4、§7 R13 |
| **测试与一致性** | 新增 **S7（横切）**：S1 的 characterization/golden 套件 + S3 起的 protocol conformance 套件（同一套件跨 LAN-WS 与 cloud-WebRTC 跑）+ 跨 host（desktop vs ridge-cli）parity；这是"统一项目防静默漂移"的核心投资 | §6 S7、§8 |
| **可观测性** | 新增 **S8（横切，与安全合并）**：结构化 tracing + 相关 id（connectionId/paneId/reqId）+ frame 级 debug 模式 | §5.7、§6 S8 |
| **S5 收敛范围** | 给 headless 完整 IDE 一个明确 **MVP 切法**（先 terminal+tree+search+只读编辑器，git/写编辑器/工作区后置），避免最重子项膨胀且把统一价值更早交付 | §6 S5、§9 |
| **多客户端语义（已定 D11）** | headless 工作区模型定为**共享实体图谱 + 每连接视图**：CRUD / PTY 输出 / 落盘内容 / 每 pane 锁定尺寸 = 共享广播，当前 workspace / 聚焦 pane / 滚动 / 选区 / 未落盘 buffer / theme = 每连接 | §4 D11、§5.1、§5.2、§6 S5、§7 R4、§9 |
| **dispatch 类型化** | S1 增决策点：`dispatch(cmd,args,ctx)` 用 stringly-typed（贴线协议、灵活）还是 typed command enum（编译期穷尽/类型安全），需在 S1 拍板 | §5.1、§6 S1 |
| **Ctx 抽象面** | 明确 `Ctx` 不只抽事件，还要抽**后台任务派发（直依 tokio，不依 `tauri::async_runtime`）与错误映射** | §5.1、§6 S1 |

---

## 0. 一句话目标

把今天**三套各自为政的远控数据面**（LAN / cloud-桌面host / cloud-headless）收敛成**一套**：一份命令实现（`ridge-core`）、一套线协议、一个可插拔传输，喂同一个**复用桌面组件、提供完整 IDE 面板**的控制端；并补上 cloud 下"桌面浏览器完整控制"这个缺口（LAN + cloud，cloud 含桌面 app host 与 headless ridge-cli host，两者都给完整 IDE）。同时建立两条**贯穿全程的横切 track**：测试与一致性（S7）、安全与可观测（S8）。

```
控制端浏览器  ——  复用桌面 SPA（web-remote-dist），完整 IDE 面板
   └─ tauriShim/bridge  →  Transport 接口（可插拔，分两层：通道原语 + 共享 RPC 层）
                            ├─ LAN-WS 适配器 ───── WebSocket（原生 text/binary）──┐
                            └─ cloud-WebRTC 适配器 ─ E2EE + 1字节通道前缀 mux ───┐
                                                                                 ▼
   host 侧统一数据面: {pane raw-byte 0x10}  {控制 JSON}  {invoke JSON-RPC 2.0 0x11}
       ├─ 桌面 app host : src-tauri 薄封装  ─┐
       └─ headless host : ridge-cli         ─┴─→  ridge-core::dispatch(cmd, args, ctx)
                                                   └ 单一命令实现 + 工作区/分屏领域模型
                                                     + 能力白名单策略层(数据)
                                                     (fs/git/search/workspace/theme/pane)
```

---

## 1. 关键概念澄清：三个"remote"

代码里 "remote" 一词被复用了三处，是认知歧义的主要来源，接手前务必区分：

| 名称 | 实体 | 谁用 | 代码位置 |
|---|---|---|---|
| **主机端 / 被控端** | 起 LAN 服务器、出二维码/验证码、列会话 | 桌面 Ridge app 自己 | `src-tauri/src/remote/`、`src/lib/remote/RemotePanel.svelte` |
| **移动端控制 UI** | 轻量 SPA，手机浏览器打开 | 手机 | `src/remote/`（独立 Vite app）→ 构建到 `static/remote/` |
| **桌面端控制 UI** | 完整主程序跑在浏览器里 | 桌面浏览器 | 主 SvelteKit app + `RIDGE_WEB_REMOTE=1` → `web-remote-dist/` |

**桌面浏览器远控不是一种新控制方式**，而是 remote 同一入口（LAN 服务器根路由 `/`）按 **User-Agent 分流**出来的变体：识别到桌面浏览器就发完整桌面 SPA，否则发移动 SPA。

---

## 2. 现状：三套数据面（连同"完成度"）

### 2.1 LAN · WebSocket（最成熟，生产可用）

- **入口**：LAN HTTP/HTTPS 服务器，固定端口 9527，根路由 `/`。见 `src-tauri/src/remote/server.rs`。
- **UA 分流**：`wants_desktop_ui()`（`server.rs` 内）—— `?ui=desktop|mobile` 覆盖 > UA 关键字黑名单（`android/iphone/ipad/ipod/mobile/windows phone`）否则判桌面 > 且 `web-remote-dist/index.html` 必须存在否则桌面 UA 回退移动 SPA。`ui_dir()` 据此选 `static_dir`（移动）/ `desktop_dir`（桌面）发文件。
- **pane 数据**：**raw PTY 字节**（二进制 WS 帧，paneId 前缀）→ 客户端 wasm 终端内核 `kernel.feed()` 自行解析。**注意：这是刻意从 per-sub postcard delta 重构来的**（见 `.kiro/specs/remote-raw-byte/`：旧 delta 方案每 sub 一个 ~11MB `PaneParser`→OOM；丢帧→状态脱节→空屏/崩溃）。
  - 评审补注：raw-byte 解决了 OOM，但带来一个必须正视的后果——**字节流不可重放**。迟到订阅/重连的客户端拿不到历史，需要 host 在 attach 时下发**当前屏幕快照**（见 §4 D10、§5.2、§7 R11）。这一点 LAN 现状是否已处理需核实；统一协议里必须显式支持。
- **元数据/控制**：JSON 文本 WS 消息（title/cwd/bell、`subscribe-pane`、`switch-workspace`、`use-global-workspace` 等）。
- **完整 IDE（仅 desktop-web 有）**：`invoke-request`/`invoke-result` JSON over WS → `dispatch_invoke_request()`（`server.rs`，**白名单**：fs/git/search/pane/terminal/workspace/theme；刻意排除 host 特权命令如 `get_remote_info`/`set_remote_enabled`/`enter_deep_root_mode`）。客户端侧由 `src/lib/transport/tauriShim/{bridge,core,event,window}.ts` 把桌面 app 的 `@tauri-apps/api/*` 调用隧道过来（vite alias，`RIDGE_WEB_REMOTE` 标志，见 `vite.config.js`）。`bridge.ts` 当前**写死依赖** `RemoteConnection`（`src/remote/lib/wsRemote.ts`）。
- **桌面浏览器启动握手**：`src/routes/+layout.svelte` 的 `startWebRemoteBoot()` —— TOTP 6 位验证码（或 localStorage token）→ `POST /verify` 拿 token → 连 WS → `bridge.attach(conn)` → 渲染。
- **工作区语义**：桌面浏览器连上发 `use-global-workspace`，被当"对等桌面"（全局活动工作区 + 多分屏 pane）；移动端是每客户端独立视图 + 单 pane。

### 2.2 Cloud · 桌面 app 作 host（**v1 scaffold，未端到端打通**）

- **传输**：WebRTC DataChannel（`label="ridge"`, ordered）+ 在其上叠的 E2EE（X25519 + ChaCha20-Poly1305，按方向分离 nonce）。见 `src/lib/remote/cloud/ridgeCloudProvider.ts`（host = answerer）、`e2ee.ts`、`apiClient.ts`、`auth.ts`。
- **激活/UI**：`src/lib/remote/cloud/CloudPanel.svelte`（设备激活、连接控制、"进入深根模式 🌱"）。
- **完成度（必须诚实）**：
  - `CloudPanel.svelte` 里 provider 的 **`onFrame` 是空 stub**（注释："留空，集成者把 onFrame 接到既有 delta 解析管线"）。即帧能收到但没接到渲染/PTY。**cloud 桌面 host 路径并未端到端工作**。
  - host WebRTC+E2EE 现在跑在 **WebView/TS**；Deep Root Mode 用 `window.hide()`（隐藏不销毁），契约 §8 终态要把 host WebRTC 迁到 **Rust（webrtc-rs）由 AppState 托管**才能销毁 WebView 仍保活。
  - `set_cloud_remote_active` / `enter_deep_root_mode` 等命令前端用 try/catch 容错（"命令可能尚未实现"）。
  - 评审补注：E2EE 的 X25519 公钥**如何被认证**（防 signaling/cloud 后端 MITM）需核实——见 §5.5、§5.6、§7 R10。

### 2.3 Cloud · headless ridge-cli（独立 Rust 主机）

- **位置**：`packages/ridge-cli/`（独立二进制：`main.rs`/`daemon.rs`/`config.rs`/`device_flow.rs` + 信令 `signaling.rs` + WebRTC `rtc.rs`/`ice.rs` + `e2ee.rs` + `protocol.rs` + `pty.rs`/`session.rs` + `fs_reuse.rs`）。
- **协议（已选对方向）**：`protocol.rs` 用 **raw 字节 + 1 字节通道前缀 mux**：`0x10 = PTY_OUTPUT`（裸字节）/ `0x11 = JSON`（带外，如搜索结果）。注释明确要与 LAN 的 `RemotePtyEvent::RawBytes` + `kernel.feed()` 对齐。
- **控制面（很窄）**：`ControlMsg`（controller→host）只有 `Input / Resize / Search / Tree`；`HostMsg`（host→controller）只有 `SearchResult / Tree / Error`。复用 `fs_reuse` 的 search/tree。**没有 git/编辑器/工作区，没有 invoke-RPC**。
- **当前耦合**：契约 §11 规定 ridge-cli **path-依赖 `src-tauri` 的 lib**，复用 `engine::pty` / `fs::search` / `fs::tree`（不复制，最多把它们设 `pub`）。⚠️ 这条 path-依赖会把 **Tauri 依赖拖进 headless 二进制**——正是 §4 决策 D4 要解决的。
- **评审补注（现实缺口）**：headless 机器**无 GUI**，git 远程操作（push/pull/fetch）的凭据来源、以及 PTY 的 shell/env/cwd/TERM，与桌面会话不同，须在 S5 显式定义（§5.4、§7 R13）。

### 2.4 关键文件索引

| 关注点 | 文件 |
|---|---|
| LAN 服务器 / UA 分流 / invoke 白名单 | `src-tauri/src/remote/server.rs` |
| LAN 鉴权 / TLS / mDNS | `src-tauri/src/remote/{auth,tls,mdns,mod}.rs` |
| remote 启动命令、会话列表 | `src-tauri/src/commands/remote.rs` |
| 移动端控制 SPA | `src/remote/`（`main.ts`/`MainApp.svelte`/`lib/wsRemote.ts`/`lib/terminalController.ts`） |
| 桌面 web-remote shim（隧道层） | `src/lib/transport/tauriShim/{bridge,core,event,window}.ts` |
| 桌面 web-remote 启动 | `src/routes/+layout.svelte`（`startWebRemoteBoot`） |
| web-remote 构建 | `vite.config.js`（alias/标志）、`svelte.config.js`、`scripts/build-desktop-web.mjs`、`package.json`（`build:desktop-web`） |
| cloud 传输 provider（桌面 host） | `src/lib/remote/cloud/{connectionProvider,ridgeCloudProvider,e2ee,apiClient,auth}.ts`、`CloudPanel.svelte` |
| cloud headless host | `packages/ridge-cli/src/*` |
| **协议 SSOT** | `docs/contracts/ridge-cloud-protocol.md` |
| pane raw-byte 重构来由 | `.kiro/specs/remote-raw-byte/` |
| cloud 后端（**独立仓库**） | `C:\code\ridge-cloud`（契约 §10/§11） |

---

## 3. 现状归纳：分叉在哪

| 维度 | LAN WS | Cloud 桌面 host | Cloud headless |
|---|---|---|---|
| 传输 | WebSocket（TLS） | WebRTC + E2EE | WebRTC + E2EE |
| pane 编码 | **raw-byte** | 帧未消费（onFrame 空，注释仍说 delta） | **raw-byte**（0x10） |
| 控制/带外 | JSON 文本帧 | — | JSON（0x11） |
| 完整 IDE | **有**（invoke-RPC 白名单） | 无 | 无（仅 search/tree） |
| 命令实现 | `src-tauri` 内、绑 Tauri | 同 LAN（进程内） | ridge-cli 自写一小撮 |
| 鉴权 | TOTP → token | device/user JWT（契约 §3） | device JWT（device flow，契约 §4.4） |
| 完成度 | 生产可用 | **v1 scaffold，未打通** | 部分（终端+搜索+目录） |

收敛信号：**pane 已在向 raw-byte 收敛**（LAN + ridge-cli 都是），cloud 桌面 host 的 delta 措辞是历史遗留；**mux 方案 ridge-cli 已发明**（1 字节前缀）。所以统一目标不是空想，是把已有趋势收口。

---

## 4. 已锁定的设计决策（brainstorming 产出 + 评审加固）

| # | 决策 | 理由 |
|---|---|---|
| **D1** | 桌面浏览器远控 = remote 同一入口的 UA 分流变体，**不是新通道** | 与现状一致；统一入口、统一鉴权 |
| **D2** | 控制端**复用桌面 SPA 组件**，提供**完整 IDE 面板**（终端+文件树+git+搜索+编辑器+工作区） | 用户明确要完整 IDE；范围=完整 IDE 时"复用组件"≈复用整个桌面 SPA。**代价（评审）**：shim 必须覆盖 SPA 触达的全部 Tauri API 面，见 R12 |
| **D3** | LAN + cloud 都支持；cloud 含**桌面 app host** 与 **headless ridge-cli host**，**两者都给完整 IDE** | 用户选最全档（headless 完整 IDE 给 MVP 切法分期，见 S5） |
| **D4** | 抽 **`ridge-core` 运行时无关 crate**（命令 handler + 工作区/分屏领域模型），desktop 与 ridge-cli 共用 | 完整 IDE 跨两种 host 的唯一干净解；且能砍掉 ridge-cli 现状对 src-tauri/Tauri 的脏依赖（见 §2.3 ⚠️） |
| **D5** | pane 数据统一为 **raw-byte**（放弃 per-sub delta）；其上 **invoke-RPC**（请求/响应）+ **控制 JSON**；字节流传输用 **1 字节通道前缀 mux** | raw-byte 已被 LAN 验证（避免 delta 的 OOM/脱节）；req/resp 语义（read_file/get_scm_status…）天然不能塞进单向推流，故 RPC 通道是必需品而非累赘 |
| **D6** | 客户端 `bridge` 依赖**可插拔 `Transport` 接口**；LAN-WS 与 cloud-WebRTC 各一适配器 | 桌面 SPA 在两种传输上行为一致，无需改业务组件 |
| **D7（新）** | RPC 关联/超时/取消/重连语义在 **Transport 之上的共享 RPC 层**实现一次；适配器只实现**通道原语**（control 收发 / pane-bytes 收发 / 生命周期） | 每个适配器各写一遍 reqId 关联必然漂移、重复 bug；`request()` 不是传输细节 |
| **D8（新）** | 命令准入**白名单作为数据**下沉进 `ridge_core::dispatch` 的**策略层**（`Ctx` 持有 capability set），LAN/cloud/headless 三 host 共用同一份执行 | 按 host 各自重写白名单 = 提权事故源；统一项目的核心正是消除这种重复 |
| **D9（新）** | 连接建立时做**协议版本 + 能力协商握手**（交换 `protocolVersion` + `capabilities`），不匹配时降级或明确拒绝并提示 | controller SPA（cloud 经公网下发、可独立更新）与 host（随桌面/CLI 版本走）必然版本漂移 |
| **D10（新）** | pane **attach/reattach 必须先下发当前屏幕快照**（host 侧为每 pane 维护屏幕缓冲，或复用终端 alt-screen/repaint），再续 raw 流 | raw-byte 不可重放；这同时是后续在 cloud 高延迟腿上引入"屏幕状态同步"的地基（见 §9） |
| **D11（新·已定）** | 工作区多客户端模型 = **共享实体图谱 + 每连接视图**。**共享(广播)**：workspace/pane 集合、分屏布局与比例、PTY 进程及其 raw-byte 输出、落盘文件内容、**每 pane 锁定渲染尺寸**（任意 controller 可改→广播、last-write-wins）。**每连接(不广播)**：当前 workspace、聚焦 pane、滚动/scrollback、编辑器/文件树选区与光标、**未落盘 buffer**、theme（连接时以 host 默认为种子）。输入路由到本连接聚焦 pane；尺寸锁定 ⇒ viewport 不驱动 PTY resize | 协作编辑器"共享文档 + 本地光标"范式，优于 tmux（tmux 连焦点都共享）；锁定尺寸消除多 viewport 的 PTY resize 冲突、与 D10 快照天然咬合；落实 R4 领域模型缺口（详 §5.1） |

> **D5 评审加注**：单通道 1-byte mux 是 **v1 选择**（与 WS 单连接现状对齐、运维简单）。其代价是 PTY 字节洪峰会 **head-of-line 阻塞**同通道的 JSON 控制/RPC（cloud 高延迟下被放大；注意 LAN 的单 TCP 连接 text/binary 也有同样性质，只是延迟低不痛）。若实测有影响，WebRTC 可拆**多 DataChannel**（PTY 一条、JSON 一条）解耦——列为 §9 可延后决策，**不阻塞 v1**。

### 完整 IDE 跨 host 的可行性结论（D3 的评估）
headless 机器有 fs/git/PTY，所以文件树/git/搜索/编辑器（文件读写）**结构上可移植**，无架构阻塞；唯一硬骨头是**命令层与 Tauri 死绑**（handler 吃 `tauri::State`/`AppHandle`，事件靠 `AppHandle` 发）。剥成 `ridge-core`（D4）即解，且该剥离恰是"统一"本身。headless 另需补一个**工作区/分屏领域模型**（桌面端在 `AppState`，headless 要等价物——也归 `ridge-core`）。
评审补：headless 的 git 凭据、PTY 环境是**现实工程缺口**而非架构阻塞（§5.4、R13）；"完整 IDE on headless" 工作量最大，建议分期（S5 MVP 切法）。

---

## 5. 统一目标架构（组件职责）

### 5.1 `ridge-core` crate（地基）
- 把 `src-tauri/src/commands/*` 的 handler + 工作区/分屏领域模型抽出，**运行时无关**：handler 改吃普通 `Ctx`（持有 `AppState` 等价物 + 一个事件发射 trait），**不再吃** `tauri::State`/`AppHandle`。
- `src-tauri` 改为薄封装：Tauri command → 构造 `Ctx` → 调 `ridge_core::dispatch`。**桌面行为零变化**。
- `ridge-cli` 链接 `ridge-core`（替换现状对 src-tauri lib 的 path-依赖），获得同一套命令 + 领域模型。
- 事件（fs-changed / scm refresh / pty metadata）经 `Ctx` 的发射 trait 抽象：Tauri 侧实现成 `AppHandle::emit`，传输侧实现成"打成控制 JSON 帧下发"。
- **评审补 · `Ctx` 抽象面不止事件**：还要抽
  - **后台任务派发**：file watcher / git 轮询等需要 spawn。`ridge-core` 直依赖 **`tokio`**，**不要**经 `tauri::async_runtime`（否则 Tauri 又被拖进 headless）。
  - **错误映射边界**：`ridge-core` 定义自己的错误类型（不依赖 Tauri 的序列化）；薄封装层把它映射成 Tauri command 错误，传输层把它映射成 JSON-RPC error 对象。
  - **状态所有权**：`AppState` 等价物以 `Arc<...>` 由宿主持有（桌面=Tauri manage，headless=daemon）；明确 `Ctx` 是每请求还是每连接构造，且 handler 必须 `Send + Sync`（支撑并发 invoke）。
- **评审补 · dispatch 类型化（S1 须拍板）**：`dispatch(cmd, args, ctx)` 有两条路线——
  - **stringly-typed**（`args: serde_json::Value`）：贴线协议、灵活、最贴近现状 invoke 形态；丢编译期类型安全。
  - **typed command enum**（serde 派生）：编译期穷尽 + 类型安全 + 重构友好；与线协议之间多一层映射。
  推荐：对外仍按 method 名分发，内部对热路径/易错命令逐步收敛到 typed，二者可共存。
- **能力策略层（D8）**：`dispatch` 入口按 `Ctx` 的 capability set 做准入，白名单是**数据**而非每 host 复制的代码（§5.6）。
- **领域模型（D11：共享实体图谱 + 每连接视图）**：`ridge-core` 持有**一份**权威 `WorkspaceGraph`（workspaces / panes / 布局 + 比例 / **每 pane 锁定尺寸** / PTY 句柄）+ 一张 `HashMap<ConnectionId, ViewState>`（`activeWorkspaceId` / `focusedPaneId` / scroll / selection / 未落盘 buffer / theme）。状态归属：

  | 状态 | 归属 | 说明 |
  |---|---|---|
  | workspace 集合（增/删/改名）、pane 集合、分屏布局 + 比例 | **共享** | 任一连接动 → 广播给所有连接 |
  | PTY 进程 + 其 raw-byte 输出 | **共享** | 一个 shell，输出广播给该 pane 所有观察者 |
  | 文件内容（**落盘**）、每 pane **锁定渲染尺寸** | **共享** | 改尺寸 = 任意 controller 可发的显式命令，广播 + last-write-wins；客户端 viewport 不驱动 resize |
  | 当前 workspace、聚焦 pane、scroll/scrollback、编辑器/树选区 + 光标、未落盘 buffer | **每连接** | 仅动发起者、不广播 |
  | theme | **每连接** | 连接时以 host 默认值为种子，之后各调各的 |

  - 事件发射 trait 须区分**广播**（CRUD 与 PTY 输出 → 所有连接）与**单连接**（焦点/选区/滚动 → 仅发起者）；焦点变更广播给所有人 = 噪音。
  - **悬空引用规则**：共享删除（删 workspace / 关 pane）时，任何 `ViewState` 正指向被删实体的连接须自动回退（兄弟 / 默认）+ 通知。
  - **并发输入收窄**：输入按连接路由到各自 `focusedPaneId`，只有两连接**恰好聚焦同一 pane** 才字节交错；v1 选最省事的（接受 / 该 pane 单写者）。
  - **编辑器内容传播**：共享 = 落盘 + 实体层，**不含**未落盘 buffer 的实时协同；A 存盘 → `fs-changed` → B "磁盘已变更"提示，故保存须做 mtime / 乐观并发检查。

### 5.2 统一线协议
- 逻辑通道：`{pane raw-byte}`（高频单向）、`{控制 JSON}`（订阅/切换/元数据/事件）、`{invoke 请求-响应}`（按需双向）。
- **invoke 信封：采用 JSON-RPC 2.0**（评审建议，取代自定义 `type`/`_reqId`）：
  - 请求 `{ "jsonrpc":"2.0", "id":…, "method":…, "params":… }`，响应 `{ "jsonrpc":"2.0", "id":…, "result"|"error":… }`。
  - 单向控制消息（订阅/事件下发）用 **notification**（无 `id`），错误用标准 `error` 对象。
  - 收益：标准化、现成的错误/通知/批量语义、生态工具、少自造少 bikeshedding。仍跑在同一 JSON 通道上。
- **版本/能力握手（D9）**：连接首帧交换 `{ protocolVersion, capabilities[] }`；host 与 controller 取交集，缺失能力对应的 UI 在 controller 侧灰掉/隐藏。
- **attach 快照（D10）**：`subscribe-pane` 的首个响应为**屏幕快照**（screen snapshot），随后才是 raw 续流；重连同理。
- **控制 JSON 分两类（D11）**：**广播类**（workspace/pane CRUD、分屏、`set_pane_size`/锁定尺寸变更、PTY 输出元数据）发给所有连接；**非广播类**（`set_active_workspace`、`focus_pane`、scroll、`set_theme`）只回发起连接。每 pane 锁定渲染尺寸是 pane 的**共享属性**，随 attach 快照（D10）一并下发；`resize` 是任意 controller 可发的显式共享命令（last-write-wins），不再由 viewport 触发。
- **RPC 生命周期**：
  - 每个 request **必带超时**，超时由 client 侧 reject。
  - 提供 **`cancel(id)`** 控制消息，支持取消长任务（大搜索等）。
  - **重连**：所有 in-flight request **一律 reject**（不静默重放，交由上层幂等重试）；重连后 bridge 负责**重订阅 panes + 重拉 snapshot**。
- **事件背压（防再次 OOM）**：fs-changed / scm refresh 等高频事件在 host 侧用 **bounded queue + 合并(coalesce) / 丢弃最旧**；`git checkout`、依赖安装等会引发事件风暴，不能无界缓冲（与 raw-byte 当初要解决的同类问题）。
- 物理承载：
  - **字节流传输（WebRTC DataChannel）**：1 字节通道前缀 mux（沿用 ridge-cli `protocol.rs`：`0x10` PTY、`0x11` JSON；invoke 与控制都走 JSON 通道，靠 JSON-RPC 的 `id`/`method` 区分）。HOL 取舍与多通道备选见 §4 D5 注 / §9。
  - **WebSocket**：用原生 text/binary 帧，无需前缀（保持 LAN 现状）。
- 与契约 §7/§9 对齐：**payload 改为 raw-byte + JSON-RPC 信封 + 版本握手**——需先改契约（R1 / **S0**）。

### 5.3 客户端 `Transport` 抽象（分两层）
评审建议把"通道原语"与"RPC 客户端"分层，避免每个适配器各写一遍关联逻辑（D7）：

- **L1 通道原语（每个适配器实现）**：
  - `sendControl(json)` / `onControl(cb)`
  - `sendPaneBytes(paneId, bytes)` / `onPaneBytes(cb)`
  - 生命周期：`connect()` / `close()` / `onStateChange(cb)`（含重连状态）
- **L2 共享 RPC 客户端（只写一次，跑在 L1 的 control 通道上）**：
  - `request(method, params) → Promise`（JSON-RPC `id` 关联、超时、`cancel`）
  - 订阅管理；重连后重订阅 + 重拉 snapshot（消费 D9 握手、D10 快照）。
- `tauriShim/bridge.ts` 改为依赖 **L2 + L1.onPaneBytes**，彻底去掉对 `RemoteConnection` 的硬 import。
- 适配器：
  - **LAN-WS**：包住现有 `RemoteConnection`（text→`onControl`、binary→`onPaneBytes`），行为不变。
  - **cloud-WebRTC**：包住 `RidgeCloudProvider`，内部做 1 字节 mux 解复用（`0x10`→pane-bytes、`0x11`→control）+ E2EE + 完成 JWT 握手。

### 5.4 两种 host 接入
- **桌面 app host**：终态把信令/WebRTC/E2EE/PTY 桥迁到 Rust（契约 §8 终态，AppState 托管），消费 mux 帧 → 进程内 `ridge_core::dispatch` + raw-byte pane 广播。（过渡期可先在 WebView 把 `onFrame` 接通验证，但终态在 Rust。）
- **headless host（ridge-cli）**：用统一协议替换 bespoke `ControlMsg`/`HostMsg`，所有命令走 `ridge_core::dispatch`；领域模型即 D11 的共享图谱 + 每连接视图（落实 R4），PTY 锁定尺寸为共享属性、viewport 不驱动 resize。
- **能力执行（D8）一致性**：两 host 都把 dispatch 的 capability set 设为"remote 白名单"（fs/git/search/pane/terminal/workspace/theme），host 特权命令（`get_remote_info`/`set_remote_enabled`/`enter_deep_root_mode` 等）**不在内**——同一份数据、同一份执行，不按 host 复制。
- **headless 专有缺口（评审）**：
  - **git 凭据**：无 GUI 机器上 push/pull/fetch 需凭据来源（SSH agent / token / credential helper）。建议先定 **本地-only git 能力档**（status/diff/stage/commit/log/branch），远程操作单独评估凭据方案，不在 S5 MVP 内。
  - **PTY 环境**：daemon 环境的 shell / env / cwd / TERM 与桌面会话不同，需显式确定（默认 shell、是否继承登录 env、起始目录策略）。

### 5.5 cloud 入口（取页 + 鉴权）
- **取页**：cloud 下控制端浏览器够不到主机 LAN server → 桌面 SPA（`web-remote-dist`）需从公网源下发（候选：ridge-cloud 后端在主域名兜底返回，参契约 §10 对 Web 面板的做法；或 CDN）。**这是新需求：契约 §0 当前规定 controller 仅复用移动端 SPA。**
  - **评审补 · 交付工程**：桌面 SPA 体量大，WAN 冷启动慢 → **code-split / lazy-load** 各 IDE 面板；CDN + **内容指纹版本化缓存**；与 host 版本经 **D9 握手对账**（避免新 SPA 连旧 host 的协议/命令错配）。
- **鉴权**：cloud 用 user JWT（controller）/ device JWT（host），见契约 §3；与 LAN 的 TOTP/token 不同。统一做法 = 各 `Transport` 适配器各自完成鉴权握手，`bridge` 只要一个"已鉴权的传输"，不强行合并两套鉴权。
- **评审补 · E2EE 密钥认证（安全关键）**：X25519 公钥必须与**设备配对身份 / JWT 身份**绑定并校验，否则中转方（signaling / cloud 后端）可替换公钥做 MITM，"E2EE"就不成立。S4/S6 须把"密钥认证绑定"列为安全评审硬项，对照契约 §3/§4.4 核实当前实现。

### 5.6 安全模型（新增 · 横切，归 S8）
- **白名单作为数据集中执行（D8）**：避免按 host 重写白名单导致 cloud 误暴露特权命令。
- **文件系统沙箱 / root-scoping**：remote 控制端经 fs 命令读写——尤其 **cloud headless 暴露的是整机 fs**，攻击面远大于 LAN 可信设备。须有**工作区根沙箱 / 路径白名单**策略，禁止越界读 `~/.ssh`、`/etc` 等。`deep-root mode`（🌱，扩权）必须**独立强鉴权 + 审计日志**，且能力位走 D9/D8 显式声明。
- **E2EE 密钥认证绑定**（见 §5.5）。
- **shim 全量审计（R12）**：复用整个桌面 SPA ⇒ shim 必须覆盖 SPA 触达的**全部** `@tauri-apps/api` 调用点，逐一处理为：隧道 / 桩(stub) / 远控模式下灰掉。

### 5.7 可观测性（新增 · 横切，归 S8）
- 结构化 **tracing**，相关 id 贯穿：`connectionId` / `paneId` / `reqId`（JSON-RPC `id`）流经 `ridge-core` 与传输层。
- **frame 级 debug 模式**（可开关）：打印通道流量（0x10/0x11、JSON-RPC method/id），集成期（S4/S5）排障神器。
- 早做、贯穿：本项是 3 路 × 2 host 统一的"调试地基"，越早接成本越低。

---

## 6. 子项目拆分、依赖、验收

> 体量过大，**一个 spec 装不下**，按依赖拆子项目，各自独立推进（建议各落一个 `.kiro/spec` 或独立 plan）。评审新增 **S0（契约前置）**、**S7（测试与一致性，横切）**、**S8（安全与可观测，横切）**。

| 子项 | 内容 | 依赖 | 验收标准 | 性质/风险 |
|---|---|---|---|---|
| **S0 契约修订前置（新）** | 修订契约 §7/§9（raw-byte + JSON-RPC 信封 + 版本握手）、§0（接纳桌面 controller）、§11（增补 `ridge-core` 文件归属）；落"改契约在先" | 无 | 契约相应条款更新并经跨团队确认；S3/S6 据此实现 | 文档+跨团队协商，**可立即并行，不被代码阻塞** |
| **S1 `ridge-core` 抽取** | commands/* + 工作区/分屏模型剥成运行时无关 crate；handler 去 Tauri 化（吃 `Ctx`：状态+事件 trait+`tokio` spawn+错误映射）；dispatch 类型化拍板；能力策略层(数据)；src-tauri 薄封装 | 无 | 桌面 app 全功能回归通过、**零行为变化**；`ridge-core` **无 Tauri 依赖**；**characterization 套件（S7）绿**；ridge-core 错误/能力边界单测 | 纯重构，**动桌面路径=最高回归风险**，地基 |
| **S2 客户端 `Transport` 抽象** | 分两层：L1 通道原语 + L2 共享 RPC 客户端（D7）；`bridge.ts` 依赖接口；LAN-WS 适配器包 `RemoteConnection` | 无（可与 S1 并行） | LAN desktop-web 行为不变（回归）；L2 RPC 单测（超时/取消/重连 reject） | 纯重构，低风险 |
| **S3 统一线协议** | raw-byte + **JSON-RPC 2.0** 控制/invoke + 字节流 1 字节 mux；**版本握手(D9)**、**attach 快照(D10)**、**RPC 超时/取消/重连**、**事件背压**；invoke-RPC 定为唯一命令面 | **S0**, S1, S2 | 协议文档化并与契约一致；LAN 在新协议下回归；**protocol conformance 套件（S7）跨 LAN-WS 与 cloud-WebRTC 同套通过** | 协议设计 + 契约修订 |
| **S4 cloud·桌面 app host 完整 IDE** | cloud-WebRTC 适配器；桌面 host 消费 mux 帧→进程内 dispatch；reconnect 重绘(D10)；E2EE 密钥认证核实；（终态迁 Rust，见契约 §8） | S1,S2,S3,(S6),S8 | 桌面浏览器经 cloud 看到完整 IDE 且终端流通畅、E2EE 生效、断连重连屏幕正确重绘 | 新能力 + 终态 Rust 迁移 |
| **S5 headless·ridge-cli 完整 IDE** | ridge-cli 链 `ridge-core` + 补领域模型（按 **D11**：共享图谱 + 每连接视图 + 每 pane 锁定尺寸）；统一协议替换 ControlMsg；git 本地能力档 + PTY 环境定义 | S1, S3 | 在无 GUI 机器上经 cloud 提供完整 IDE（MVP：terminal+tree+search+只读编辑器；P2：git 本地+写编辑器+工作区） | 工作量大，**先 MVP 切法分期** |
| **S6 cloud 入口** | 公网下发桌面 SPA（code-split + CDN 版本化缓存）+ 鉴权（user/device JWT）；扩契约 §0/§10 接纳桌面 controller；版本握手对账 | **S0**, S2, S3 | 控制端能在公网加载桌面 SPA 并鉴权连通；新 SPA 连旧 host 经 D9 正确降级/提示 | 横切，**跨 repo（ridge-cloud）** |
| **S7 测试与一致性（新 · 横切）** | ① S1 的 **characterization/golden 套件**（采集现状命令 req/resp 语料，回放打 ridge-core）② S3 起的 **protocol conformance 套件**（同一套件跨两传输跑）③ 跨 host **parity 套件**（desktop vs ridge-cli 同命令同结果） | 随 S1/S3 | 套件纳入 CI，作为 S1/S3/S4/S5 的硬验收门 | **防静默漂移的核心投资** |
| **S8 安全与可观测（新 · 横切）** | 能力白名单数据化(D8) 落地；fs 沙箱/root-scoping；deep-root 强鉴权+审计；E2EE 密钥认证核实；shim 全量审计；tracing + 相关 id + frame debug | 贯穿 | 安全评审 checklist 全过；关键路径有结构化 trace 与 debug 开关 | 横切，重点在 S4/S5/S6 |

**推荐顺序**：`S0 ∥ S1 ∥ S2`（S0 文档/协商，S1 含 S7 characterization，S2 含 L2 RPC 单测）→ `S3`（含 S7 conformance）→ `{S4, S5, S6}`；**S8 贯穿全程，重点在 S4/S5/S6**。先 **S1**（解锁最多 + 风险最高，趁早单独做、配桌面回归 + characterization 网）；**S0 立刻并行起跑**以免成为 S3/S6 的关键路径瓶颈。

---

## 7. 风险与必须先解决的矛盾

- **R1 契约 SSOT 部分过时**：`docs/contracts/ridge-cloud-protocol.md` §7.2/§9 仍写 "postcard 增量帧 schema 不改"，但 LAN 与 ridge-cli 实际已是 raw-byte；§0 规定 controller 仅复用移动端 SPA（无桌面 controller 概念）。契约自有规则"**改契约在先**"——**已抽成独立前置子项 S0**，S3/S6 开工前先就绪，否则跨组件实现会再次发散。
- **R2 桌面回归风险（S1）**：动 `commands/*` 与 `AppState`，必须有桌面端全功能回归手段。历史记忆提醒：动桌面路径要慎重（见 fileExplorer 迁移待办）。**缓解**：S7 characterization/golden 套件作为安全网，不靠"应该没问题"。
- **R3 ridge-cli 当前脏依赖**：现状 path-依赖 src-tauri lib（带 Tauri）。`ridge-core` 抽取须确保 ridge-core **零 Tauri 依赖**（含**不经 `tauri::async_runtime`**，直依 `tokio`），否则 headless 二进制被 Tauri 污染。
- **R4 headless 领域模型缺口**：工作区/分屏状态今天只在桌面 `AppState`，headless 要等价物（归 `ridge-core`）。**已决（D11）**：共享实体图谱 + 每连接视图 + 每 pane 锁定尺寸（详 §5.1）；剩余实现风险集中在悬空引用回退与同 pane 并发输入两条边角。
- **R5 cloud 仍 scaffold**：`onFrame` 空、host WebRTC 在 WebView、deep-root 是 hide 非 destroy。终态须把 host WebRTC 迁 Rust（契约 §8）才能真正保活/降内存。S4 要规划这条迁移，不要在 WebView 上叠太多终态逻辑。
- **R6 跨 repo / 文件归属**：cloud 后端在独立仓库 `C:\code\ridge-cloud`；契约 §11 有严格文件归属（B=src/、C=src-tauri/、D=packages/ridge-cli/）。`ridge-core` 抽取会**打破 §11 现有边界**（从 src-tauri 切出新 crate 并被 packages 依赖）——**在 S0 一并增补 `ridge-core` 归属**。
- **R7 单通道 mux 的 HOL 阻塞（新）**：PTY 字节洪峰会 head-of-line 阻塞同通道的 JSON 控制/RPC，cloud 高延迟放大。**缓解**：v1 接受（与 WS 同性质），实测有影响则拆多 DataChannel（§9）。
- **R8 事件风暴再次 OOM（新）**：高频 fs/scm 事件若无界缓冲下发，重蹈 delta-OOM 覆辙。**缓解**：host 侧 bounded + coalesce + drop policy（§5.2）。
- **R9 controller / host 版本漂移（新）**：cloud SPA 经公网独立更新，与 host 版本不同步导致协议/命令错配。**缓解**：D9 版本/能力握手 + 降级提示。
- **R10 cloud 全 fs 暴露 + deep-root 提权（新）**：headless 把整机 fs 暴露给远程控制端。**缓解**：fs 根沙箱/路径白名单、白名单数据化(D8)、deep-root 强鉴权+审计、E2EE 密钥认证绑定（§5.6）。
- **R11 raw-byte 重连/迟到订阅空屏（新）**：字节流不可重放，重连/第二控制端 attach 会看到空屏/错乱。**缓解**：attach 快照(D10)，host 维护 per-pane 屏幕缓冲。
- **R12 复用整个 SPA 的隐藏 shim 范围（新）**：SPA 任何未被 shim 隧道的 `@tauri-apps/api` 调用都会在远控模式运行时报错。**缓解**：审计 SPA 全部 Tauri API 调用点，逐一隧道/桩/灰掉（§5.6 / S8）。
- **R13 headless git 凭据 / PTY 环境缺口（新）**：无 GUI 机器的 git 远程操作凭据与 PTY 环境与桌面不同。**缓解**：S5 先定本地-only git 能力档 + 显式 PTY 环境（§5.4）。

---

## 8. 给接手 agent 的起步指引

1. **先读**：本文件 → 契约 `docs/contracts/ridge-cloud-protocol.md` → `.kiro/specs/remote-raw-byte/`（理解为什么是 raw-byte 不是 delta，以及由此引出的 attach-快照需求）。
2. **立刻并行起 S0（契约修订）**：它只是文档+协商，但 S3/S6 都等它；越早起跑越不堵关键路径。同批在契约 §11 增补 `ridge-core` 归属。
3. **用 codegraph** 摸 `dispatch_invoke_request` 的 callees 与 `commands/*` 对 `tauri::State`/`AppHandle`/`async_runtime` 的依赖面，量化 S1 的去 Tauri 化工作量（`codegraph_impact` / `codegraph_callees`）。
4. **建 characterization/golden 套件（S7）作为 S1 的安全网**：先采集现状命令的 req/resp 语料，再回放打 `ridge-core`，把"零行为变化"变成可验证而非口头承诺。
5. **从 S1 起**：先列出 `commands/*` 里哪些 handler 是纯 fs/git/无状态（易迁）、哪些吃 AppState/AppHandle（需抽 `Ctx` + 事件 trait + spawn + 错误映射）。**在 S1 拍板 dispatch 类型化路线**。建议每个功能点单独 commit（项目偏好）。
6. **S3 起把 protocol conformance 套件（S7）跨两传输跑**：同一套件同时打 LAN-WS 与 cloud-WebRTC，从机制上防 D6"两传输行为一致"退化成漂移。
7. **验收用真实证据**：S1 完成必须跑桌面端回归（不是"应该没问题"）。
8. **改协议前先改契约**（R1 / S0）。
9. **保持 LAN 生产路径绿色**：S1/S2/S3 都以"LAN 行为不变"为硬验收。
10. **安全与可观测（S8）尽早接**：白名单数据化、fs 沙箱、tracing/相关 id、frame debug 在 S4/S5 之前就位，集成期省大量排障。

---

## 9. 仍待用户拍板的开放点

**已决（原阻塞，现关闭）：**
- **headless 多客户端工作区语义** → **D11**：共享实体图谱 + 每连接视图；每 pane 锁定渲染尺寸（任意 controller 可改、广播、last-write-wins）；编辑器共享 = 落盘 + 实体层、不含未落盘 buffer 实时协同；theme 每连接、host 默认为种子。详 §5.1。

**阻塞性（需在对应子项开工前定）：**
- **S5 MVP 切法**：headless 完整 IDE 是否按"MVP（terminal+tree+search+只读编辑器）→ P2（git 本地+写编辑器+工作区）"分期？建议是，以更早交付统一价值、不让最重子项膨胀。
- **编辑器 pane 的"打开文档"归属（D11 残留小项）**：pane 本身共享，但"该 editor pane 当前打开哪个文档"算共享实体还是每连接视图？倾向每连接（与选区一致），需在 S5 定。

**非阻塞（不影响 S0/S1/S2，可后定）：**
- S6 取页：桌面 SPA 经 cloud 下发，走 ridge-cloud 后端兜底 vs 独立 CDN？（两者都要 code-split + 版本化缓存 + D9 对账）
- cloud 桌面 host 终态 Rust 迁移（契约 §8）与本统一是否同批做，还是 S4 先 WebView 过渡、Rust 迁移另立子项？
- **物理通道（评审新增）**：cloud 实测若 PTY 洪峰阻塞 RPC，是否从单通道 1-byte mux 升级为**多 DataChannel**（PTY/JSON 分离）？v1 不做，留观测后决定（R7）。
- **cloud 腿的终端语义（评审新增 · 远期）**：cloud 高延迟下，是否在 raw-byte 之上为终端引入"**屏幕状态同步**"（类 mosh：只同步到最新屏幕、可跳过中间态、天然解决迟到订阅与背压）？D10 的 per-pane 屏幕缓冲已是其地基。注意：当初弃用 delta 是因为**实现**（每 sub 11MB PaneParser）而非状态同步思想本身有错——若将来要做，须用共享/服务端单份屏幕模型 + 轻量 diff，不重蹈 per-sub 重解析。**LAN 保持 raw-byte 不动。**
