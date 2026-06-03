# 统一远控架构 + Cloud 桌面控制 —— 现状与规划（Handoff Plan）

> 状态：**规划中（未开工）**。本文件是给后续接手 agent 的单一交接文档：先讲清**现状**，再讲清**要做什么、按什么顺序做**。
> 作者轮次：2026-06-03 brainstorming 产出。语言约定：散文简体中文，标识符/字段名英文（与代码库一致）。
> 上游权威契约：[`docs/contracts/ridge-cloud-protocol.md`](../contracts/ridge-cloud-protocol.md)（Ridge Cloud 商业化协议 SSOT）。本文件**不覆盖**该契约；凡与之冲突处，以"改契约在先"为准（见 §6 风险 R1）。

---

## 0. 一句话目标

把今天**三套各自为政的远控数据面**（LAN / cloud-桌面host / cloud-headless）收敛成**一套**：一份命令实现（`ridge-core`）、一套线协议、一个可插拔传输，喂同一个**复用桌面组件、提供完整 IDE 面板**的控制端；并补上 cloud 下"桌面浏览器完整控制"这个缺口（LAN + cloud，cloud 含桌面 app host 与 headless ridge-cli host，两者都给完整 IDE）。

```
控制端浏览器  ——  复用桌面 SPA（web-remote-dist），完整 IDE 面板
   └─ tauriShim/bridge  →  Transport 接口（可插拔）
                            ├─ LAN-WS 适配器 ───── WebSocket（原生 text/binary）──┐
                            └─ cloud-WebRTC 适配器 ─ E2EE + 1字节通道前缀 mux ───┐
                                                                                 ▼
   host 侧统一数据面: {pane raw-byte 0x10}  {控制 JSON}  {invoke 请求/响应 0x11}
       ├─ 桌面 app host : src-tauri 薄封装  ─┐
       └─ headless host : ridge-cli         ─┴─→  ridge-core::dispatch(cmd, args, ctx)
                                                   └ 单一命令实现 + 工作区/分屏领域模型
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

### 2.3 Cloud · headless ridge-cli（独立 Rust 主机）

- **位置**：`packages/ridge-cli/`（独立二进制：`main.rs`/`daemon.rs`/`config.rs`/`device_flow.rs` + 信令 `signaling.rs` + WebRTC `rtc.rs`/`ice.rs` + `e2ee.rs` + `protocol.rs` + `pty.rs`/`session.rs` + `fs_reuse.rs`）。
- **协议（已选对方向）**：`protocol.rs` 用 **raw 字节 + 1 字节通道前缀 mux**：`0x10 = PTY_OUTPUT`（裸字节）/ `0x11 = JSON`（带外，如搜索结果）。注释明确要与 LAN 的 `RemotePtyEvent::RawBytes` + `kernel.feed()` 对齐。
- **控制面（很窄）**：`ControlMsg`（controller→host）只有 `Input / Resize / Search / Tree`；`HostMsg`（host→controller）只有 `SearchResult / Tree / Error`。复用 `fs_reuse` 的 search/tree。**没有 git/编辑器/工作区，没有 invoke-RPC**。
- **当前耦合**：契约 §11 规定 ridge-cli **path-依赖 `src-tauri` 的 lib**，复用 `engine::pty` / `fs::search` / `fs::tree`（不复制，最多把它们设 `pub`）。⚠️ 这条 path-依赖会把 **Tauri 依赖拖进 headless 二进制**——正是 §4 决策 D4 要解决的。

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

## 4. 已锁定的设计决策（brainstorming 产出）

| # | 决策 | 理由 |
|---|---|---|
| **D1** | 桌面浏览器远控 = remote 同一入口的 UA 分流变体，**不是新通道** | 与现状一致；统一入口、统一鉴权 |
| **D2** | 控制端**复用桌面 SPA 组件**，提供**完整 IDE 面板**（终端+文件树+git+搜索+编辑器+工作区） | 用户明确要完整 IDE；范围=完整 IDE 时"复用组件"≈复用整个桌面 SPA |
| **D3** | LAN + cloud 都支持；cloud 含**桌面 app host** 与 **headless ridge-cli host**，**两者都给完整 IDE** | 用户选最全档 |
| **D4** | 抽 **`ridge-core` 运行时无关 crate**（命令 handler + 工作区/分屏领域模型），desktop 与 ridge-cli 共用 | 完整 IDE 跨两种 host 的唯一干净解；且能砍掉 ridge-cli 现状对 src-tauri/Tauri 的脏依赖（见 §2.3 ⚠️） |
| **D5** | pane 数据统一为 **raw-byte**（放弃 per-sub delta）；其上 **invoke-RPC**（请求/响应）+ **控制 JSON**；字节流传输用 **1 字节通道前缀 mux** | raw-byte 已被 LAN 验证（避免 delta 的 OOM/脱节）；req/resp 语义（read_file/get_scm_status…）天然不能塞进单向推流，故 RPC 通道是必需品而非累赘 |
| **D6** | 客户端 `bridge` 依赖**可插拔 `Transport` 接口**；LAN-WS 与 cloud-WebRTC 各一适配器 | 桌面 SPA 在两种传输上行为一致，无需改业务组件 |

### 完整 IDE 跨 host 的可行性结论（D3 的评估）
headless 机器有 fs/git/PTY，所以文件树/git/搜索/编辑器（文件读写）**结构上可移植**，无架构阻塞；唯一硬骨头是**命令层与 Tauri 死绑**（handler 吃 `tauri::State`/`AppHandle`，事件靠 `AppHandle` 发）。剥成 `ridge-core`（D4）即解，且该剥离恰是"统一"本身。headless 另需补一个**工作区/分屏领域模型**（桌面端在 `AppState`，headless 要等价物——也归 `ridge-core`）。

---

## 5. 统一目标架构（组件职责）

### 5.1 `ridge-core` crate（地基）
- 把 `src-tauri/src/commands/*` 的 handler + 工作区/分屏领域模型抽出，**运行时无关**：handler 改吃普通 `Ctx`（持有 `AppState` 等价物 + 一个事件发射 trait），**不再吃** `tauri::State`/`AppHandle`。
- `src-tauri` 改为薄封装：Tauri command → 构造 `Ctx` → 调 `ridge_core::dispatch`。**桌面行为零变化**。
- `ridge-cli` 链接 `ridge-core`（替换现状对 src-tauri lib 的 path-依赖），获得同一套命令 + 领域模型。
- 事件（fs-changed / scm refresh / pty metadata）经 `Ctx` 的发射 trait 抽象：Tauri 侧实现成 `AppHandle::emit`，传输侧实现成"打成控制 JSON 帧下发"。

### 5.2 统一线协议
- 逻辑通道：`{pane raw-byte}`（高频单向）、`{控制 JSON}`（订阅/切换/元数据/事件）、`{invoke 请求-响应 JSON}`（按需双向）。
- 物理承载：
  - **字节流传输（WebRTC DataChannel）**：1 字节通道前缀 mux（沿用 ridge-cli `protocol.rs`：`0x10` PTY、`0x11` JSON；invoke 与控制都走 JSON 通道，靠 JSON 内 `type`/`_reqId` 区分）。
  - **WebSocket**：用原生 text/binary 帧，无需前缀（保持 LAN 现状）。
- 与契约 §7/§9 对齐：**payload 改为 raw-byte**——需先改契约（R1）。

### 5.3 客户端 `Transport` 抽象
- 定义接口（暂名）：`sendJson(msg)` / `onJson(cb)` / `sendBytes(paneId, bytes)`? / `onPaneBytes(cb)` / `request(cmd,args)→Promise` / 连接生命周期。
- `tauriShim/bridge.ts` 改为依赖此接口（去掉对 `RemoteConnection` 的硬 import）。
- 适配器：**LAN-WS**（包住现有 `RemoteConnection`，行为不变）；**cloud-WebRTC**（包住 `RidgeCloudProvider`，内部做 1 字节 mux + E2EE，并完成 JWT 握手）。

### 5.4 两种 host 接入
- **桌面 app host**：终态把信令/WebRTC/E2EE/PTY 桥迁到 Rust（契约 §8 终态，AppState 托管），消费 mux 帧 → 进程内 `ridge_core::dispatch` + raw-byte pane 广播。（过渡期可先在 WebView 把 `onFrame` 接通验证，但终态在 Rust。）
- **headless host（ridge-cli）**：用统一协议替换 bespoke `ControlMsg`/`HostMsg`，所有命令走 `ridge_core::dispatch`。

### 5.5 cloud 入口（取页 + 鉴权）
- **取页**：cloud 下控制端浏览器够不到主机 LAN server → 桌面 SPA（`web-remote-dist`）需从公网源下发（候选：ridge-cloud 后端在主域名兜底返回，参契约 §10 对 Web 面板的做法；或 CDN）。**这是新需求：契约 §0 当前规定 controller 仅复用移动端 SPA。**
- **鉴权**：cloud 用 user JWT（controller）/ device JWT（host），见契约 §3；与 LAN 的 TOTP/token 不同。统一做法 = 各 `Transport` 适配器各自完成鉴权握手，`bridge` 只要一个"已鉴权的传输"，不强行合并两套鉴权。

---

## 6. 子项目拆分、依赖、验收

> 体量过大，**一个 spec 装不下**，按依赖拆 6 个子项目，各自独立推进（建议各落一个 `.kiro/spec` 或独立 plan）。

| 子项 | 内容 | 依赖 | 验收标准 | 性质/风险 |
|---|---|---|---|---|
| **S1 `ridge-core` 抽取** | commands/* + 工作区/分屏模型剥成运行时无关 crate；handler 去 Tauri 化；src-tauri 薄封装 | 无 | 桌面 app 全功能回归通过、**零行为变化**；`ridge-core` 无 Tauri 依赖 | 纯重构，**动桌面路径=最高回归风险**，地基 |
| **S2 客户端 `Transport` 抽象** | `bridge.ts` 依赖接口；LAN-WS 适配器包 `RemoteConnection` | 无（可与 S1 并行） | LAN desktop-web 行为不变（回归） | 纯重构，低风险 |
| **S3 统一线协议** | raw-byte + JSON 控制 + invoke-RPC，字节流 1 字节 mux；invoke-RPC 定为唯一命令面；**先改契约 §7/§9** | S1, S2 | 协议文档化并与契约一致；LAN 在新协议下回归 | 协议设计 + 契约修订 |
| **S4 cloud·桌面 app host 完整 IDE** | cloud-WebRTC 适配器；桌面 host 消费 mux 帧→进程内 dispatch；（终态迁 Rust，见契约 §8） | S1,S2,S3,(S6) | 桌面浏览器经 cloud 看到完整 IDE 且终端流通畅、E2EE 生效 | 新能力 + 终态 Rust 迁移 |
| **S5 headless·ridge-cli 完整 IDE** | ridge-cli 链 `ridge-core` + 补领域模型；统一协议替换 ControlMsg | S1, S3 | 在无 GUI 机器上经 cloud 提供完整 IDE（含 git/编辑器/工作区） | 工作量大 |
| **S6 cloud 入口** | 公网下发桌面 SPA + 鉴权（user/device JWT）；扩契约 §0/§10 接纳桌面 controller | S2, S3 | 控制端能在公网加载桌面 SPA 并鉴权连通 | 横切，**跨 repo（ridge-cloud）** |

**推荐顺序**：`S1 ∥ S2` → `S3` → `{S4, S5, S6}`。先 **S1**（解锁最多 + 风险最高，趁早单独做、配桌面回归）。

---

## 7. 风险与必须先解决的矛盾

- **R1 契约 SSOT 部分过时**：`docs/contracts/ridge-cloud-protocol.md` §7.2/§9 仍写 "postcard 增量帧 schema 不改"，但 LAN 与 ridge-cli 实际已是 raw-byte；§0 规定 controller 仅复用移动端 SPA（无桌面 controller 概念）。契约自有规则"**改契约在先**"——S3/S6 开工前先修订契约相应条款，否则跨组件实现会再次发散。
- **R2 桌面回归风险（S1）**：动 `commands/*` 与 `AppState`，必须有桌面端全功能回归手段。历史记忆提醒：动桌面路径要慎重（见 fileExplorer 迁移待办）。
- **R3 ridge-cli 当前脏依赖**：现状 path-依赖 src-tauri lib（带 Tauri）。`ridge-core` 抽取须确保 ridge-core **零 Tauri 依赖**，否则 headless 二进制被 Tauri 污染。
- **R4 headless 领域模型缺口**：工作区/分屏状态今天只在桌面 `AppState`，headless 要等价物（归 `ridge-core`），注意与桌面端"全局活动工作区"语义对齐。
- **R5 cloud 仍 scaffold**：`onFrame` 空、host WebRTC 在 WebView、deep-root 是 hide 非 destroy。终态须把 host WebRTC 迁 Rust（契约 §8）才能真正保活/降内存。S4 要规划这条迁移，不要在 WebView 上叠太多终态逻辑。
- **R6 跨 repo / 文件归属**：cloud 后端在独立仓库 `C:\code\ridge-cloud`；契约 §11 有严格文件归属（B=src/、C=src-tauri/、D=packages/ridge-cli/）。`ridge-core` 抽取会**打破 §11 现有边界**（它要从 src-tauri 切出新 crate 并被 packages 依赖）——需在契约 §11 增补 `ridge-core` 的归属。

---

## 8. 给接手 agent 的起步指引

1. **先读**：本文件 → 契约 `docs/contracts/ridge-cloud-protocol.md` → `.kiro/specs/remote-raw-byte/`（理解为什么是 raw-byte 不是 delta）。
2. **用 codegraph** 摸 `dispatch_invoke_request` 的 callees 与 `commands/*` 对 `tauri::State`/`AppHandle` 的依赖面，量化 S1 的去 Tauri 化工作量（`codegraph_impact` / `codegraph_callees`）。
3. **从 S1 起**：先列出 `commands/*` 里哪些 handler 是纯 fs/git/无状态（易迁）、哪些吃 AppState/AppHandle（需抽 `Ctx` + 事件 trait）。建议每个功能点单独 commit（项目偏好）。
4. **验收用真实证据**：S1 完成必须跑桌面端回归（不是"应该没问题"）。
5. **改协议前先改契约**（R1）。
6. **保持 LAN 生产路径绿色**：S1/S2/S3 都以"LAN 行为不变"为硬验收。

---

## 9. 仍待用户拍板的开放点（不阻塞 S1/S2）

- S6 取页：桌面 SPA 经 cloud 下发，走 ridge-cloud 后端兜底 vs 独立 CDN？
- headless 完整 IDE 的"工作区"语义：headless 多客户端时的工作区归属（全局 vs 每连接）？
- cloud 桌面 host 终态 Rust 迁移（契约 §8）与本统一是否同批做，还是 S4 先 WebView 过渡、Rust 迁移另立子项？
