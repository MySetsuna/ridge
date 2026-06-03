# 统一远控架构 —— 多 Agent 执行编排日志（总经理记录）

> 本文件是「总经理（GM）」agent 的权威记录：记录派工、各执行 agent 的产出与计划的 gap、分歧点的拍板结论。
> 上游计划：[`unified-remote-architecture-handoff-final.md`](./unified-remote-architecture-handoff-final.md)（S0–S8）。
> 状态语言约定与上游一致：散文简体中文，标识符英文。

## 组织结构

| 角色 | 实体 | 职责 |
|---|---|---|
| **总经理 GM** | 主会话（Claude Opus） | 派工、记录、拍板分歧、把关 commit（验证证据后才提交） |
| **经理 Manager** | `manager` teammate | 居中调度协助；确认各执行 agent 的实现与计划验收标准的 gap，产出 gap 报告与分歧清单 |
| **执行 S0** | `s0-contract` teammate | S0 契约修订（纯文档，可立即并行，解锁 S3/S6） |
| **执行 S1** | `s1-core` teammate | S1 `ridge-core` 抽取（地基，最高回归风险） |
| **执行 S2** | `s2-transport` teammate | S2 客户端 Transport 分层抽象（L1 通道原语 + L2 共享 RPC） |

> **执行模型（2026-06-03 修订）**：本机无 tmux/WSL，swarm/后台 teammate 不可用。改为 **GM 居中调度的并发前台 subagent**：执行 agent 以并发前台 subagent 跑、文件改动落盘持久、回交结构化报告；**所有跨切面协调与分歧拍板经 GM 中转**（即"居中调度"）。Manager 在执行 agent 回报后由 GM 派发做 gap 复核。task 板由 GM 维护。

规则（来自用户指令）：
- 执行 agent **必须严格实现**计划；若无法实现，**先与其他 agent / GM 讨论出合理方案再继续**，**不中断问用户**。
- 跨切面 / 协议级分歧 → 上报 GM 拍板，GM 记录于本文件「分歧拍板」。
- 执行 agent **不自行 git commit**；改动留工作区，由 GM 验证后按"每功能点一 commit"（上游 §8.5 偏好）提交。

## 环境约束（影响验收方式 —— 必须诚实）

- 本机重新构建 Tauri 后端会**杀死当前会话**；`cargo test --lib`（ridge cdylib）本机以 `0xc0000139` 崩溃。
  ⇒ Rust 改动的可用验收上限是 **`cargo check`（编译通过）**；**完整桌面回归（运行 app）必须由用户在本机 rebuild 验证**，GM 据此对 S1 类后端改动做 commit 把关与标注。
- 前端（TS/Svelte）可用 `svelte-check` / `tsc` 验证，会话内可完成。

## 现状勘察（codegraph + grep，2026-06-03）

- 代码图谱：272 文件 / 5675 节点 / 9290 边。Rust 77、TS 108、Svelte 55。
- **S1 体量**：`src-tauri/src/commands/*` 共 **~8,282 行 / 12 文件 / ~135 个 `#[tauri::command]`**。
  - 最大：`git.rs`(2055, 32 cmd) · `terminal.rs`(1512, 15 cmd) · `project.rs`(1259, 25 cmd) · `pane.rs`(887, 10 cmd) · `ridge_file.rs`(855, 14 cmd)。
  - 最小/低耦合（迁移先行片）：`settings.rs`(24, 1 cmd) · `theme.rs`(301, 2 cmd, 6×AppHandle) · `process.rs`(348, 2 cmd, 2×State)。
  - Tauri 耦合面：`State<>` 出现于 10 文件、`AppHandle` 出现于 7 文件；`async_runtime` 仅 `lib.rs` 1 处。
  - 安全边界入口：`dispatch_invoke_request`（`src-tauri/src/remote/server.rs:2208`），白名单即统一项目要下沉的策略层（D8）。
- 契约 SSOT 现有章节齐备：§0 名词拓扑、§7 WebRTC/E2EE、§9 复用既有代码、§11 文件归属 —— 均为 S0 待修订项。
- 尚不存在 `ridge-core` crate；`packages/` 现有 `rg-split`/`ridge-cli`/`ridge-term`。

## 分波次执行计划

- **Wave 1（并行）**：S0 ∥ S1 ∥ S2 —— 本会话目标。
- Wave 2：S3（依赖 S0+S1+S2）。Wave 3：{S4, S5, S6}。S7/S8 横切。
- 本会话现实交付预期：
  - **S0**：可完整交付（纯文档修订 + 跨团队确认标注）。
  - **S2**：可交付主体（L1/L2 接口 + LAN-WS 适配器；`svelte-check` 验证）。
  - **S1**：交付**可编译的地基 + 首个垂直迁移片 + 余量迁移台账**（`cargo check` 绿）；完整 135 handler 迁移与桌面回归留后续会话 + 用户 rebuild 验证。

## 分歧拍板（GM 决策记录）

| 编号 | 议题 | 结论 | 依据 | 时间 |
|---|---|---|---|---|
| D-S1-1 | dispatch 类型化（上游 §5.1 要求 S1 拍板） | **边界 stringly-typed**（`dispatch(method, args: serde_json::Value, ctx)`），内部热路径/易错命令可后续逐步收敛 typed enum，二者共存 | 贴现状 invoke 形态、对"零行为变化"重构风险最低；与上游推荐一致 | 2026-06-03 |
| D-GM-1 | ridge-core crate 落点（S0 契约写 `crates/`，S1 实现落 `packages/`，冲突） | **`packages/ridge-core/`**（与 sibling `packages/ridge-cli`/`ridge-term` 平级）。GM 已改契约 §11 全部 `crates/ridge-core`→`packages/ridge-core`（grep 验证 0 残留） | S1 已在此编译通过，搬迁=纯返工；`packages/` 是既有 crate 根，更一致；改文档成本最低 | 2026-06-03 |
| D-GM-2 | 错误码端到端损耗（Manager HIGH：CoreError 码表经 LAN 腿被压成 message-only） | **S1 码表是为 S3 前置准备；LAN 腿在 S3 把 host 升级 JSON-RPC-native 前不透传 code/data**，属计划内。已在两损耗点（`server.rs::core_result_to_envelope`、`lanWsAdapter.handleInbound`）加 `TODO(S3)` 锚点；S7 conformance 对 code 的断言须等 S3 收口后开启 | 根因是 legacy WS 信封 message-only，非 S1/S2 实现缺陷；加锚点防遗忘 | 2026-06-04 |
| D-GM-3 | 新 workspace 根的 lock/target 卫生 | 提交新根 `Cargo.lock` + 根 `.gitignore` 增 `/target`；**暂不** `git rm` 三个旧 per-crate lock（`src-tauri`/`ridge-cli`/`ridge-term`，workspace 模式下被 cargo 忽略、无害）——待用户 `tauri build`/wasm-pack 验证 workspace 后再清理 | 删 tracked 文件是破坏性操作，且 workspace 方案本身待构建验证；保守留存 | 2026-06-04 |
| D-GM-4 | S3 协议落地策略（直接 flip vs 向后兼容） | **向后兼容加法式**：host 同时认 legacy 与 JSON-RPC 帧、按形态对称回复；adapter 默认 legacy 翻译，收到 host `$/hello` 回复后才升级原生 JSON-RPC。legacy `dispatch_invoke_request` 一字未动 | 现网 LAN 远控不可运行时验证，破坏性 flip 风险过高；加法式让老 web-remote-dist/移动端零改动仍工作 | 2026-06-04 |
| D-GM-5 | `$/bye`（D9 版本不匹配）是否强制关 socket | **不强制关**：host 发 `$/bye`，client 标记 rejected + 提示升级，由 client 决定 UX | 契约 §7.3「降级或明确拒绝并提示」；非破坏性 | 2026-06-04 |
| D-GM-6 | D10 全量 per-pane 屏幕缓冲归属 | S3 仅交付 `PaneSnapshotFrame` 消息类型 + subscribe 接入点 + 实现要点；**全量屏缓冲实现切到 S5**（与 pane 流改造、D11 共享尺寸耦合） | 量大且与 S5 领域模型咬合；避免 S3 膨胀 | 2026-06-04 |
| D-GM-7 | cloud `0x10` PANE_RAW 的 paneId 字节布局 | 采用 **`0x10 \|\| paneIdLen(u8) \|\| paneId(UTF-8) \|\| raw`**（S4 cloud 适配器已实现）；已登记契约 §7。ridge-cli `protocol.rs`(单 pane 无 paneId) + 桌面 host 编码器须按此对齐（对齐债，LAN 走 WS binary 不用此 mux 故不影响现网） | 多 pane 需自描述帧头；登记 SSOT 防三方漂移(R1) | 2026-06-04 |
| D-GM-8 | JSON-RPC 应用错误码空间 | ridge-core `CoreError` 用 **1000-1006**（避开 JSON-RPC 保留区 `-32768..-32000`）+ 标准 `-32601/-32602`；已登记契约 §7.0 码表（含 S8 新增 1006 outside_sandbox）。新增码须登记此表 | 防 S1/S3/S8 三方码值漂移(R1) | 2026-06-04 |
| D-GM-9 | headless 沙箱激活时点（Manager LOW gap，R10） | S5 MVP（只读 search/tree、fail-soft）**接受 headless_ctx 暂不 `.with_roots()`（沙箱 no-op）**；但把"`headless_ctx` 绑定工作区根 ⇒ `.with_roots()`"列为 **fs 写命令迁入 ridge-cli 的前置硬门**，避免写命令上线时整机 fs 裸奔 | 当前 exploit 路径未通（S4-host 未打通 + 无写命令）；硬门前置防回归 | 2026-06-04 |
| D-GM-10 | S4-host E2EE 公钥↔设备身份绑定（§5.5/R10，安全） | **本期不做完整绑定**：当前为 **relay-trust**（依赖 signaling 把双方撮合进同一 room，cloud 后端被攻陷理论可 MITM）。host 桥已留 `KeyBindingVerifier` 接入点（v1 默认 relay-trust、向后兼容）。完整方案=协议级跨仓库变更（契约 §7.1 握手增身份签名字段 + ridge-cloud 发布对端身份验证材料接口 + e2ee.ts 校验），列为独立跨仓库 track。**诚实标注：cloud E2EE 当前非 MITM-resistant** | 完整绑定跨 wind/ridge-cloud 两仓库 + 契约改，需专门 track；不在 S4-host 单切片内 | 2026-06-04 |
| D-GM-11 | S4-host cloud pane 流（PTY over WebRTC） | 本期 host 桥实现 invoke 往返 + $/hello + $/cancel + pane 编码器(D-GM-7)，但**真实 PTY 源未注入**（`paneOutputSource` 占位）。pane 流接入需 Tauri-event 桥（碰 src-tauri）或待 host WebRTC 迁 Rust（契约 §8 终态）——留后续 | host WebView/TS 期 PTY fan-out 需经 Tauri event 桥；与终态 Rust 迁移耦合；invoke 路径先行更稳 | 2026-06-04 |

## 执行进度

### Wave 1 完成（2026-06-04）—— Manager 复核全 PASS，无 CRITICAL/阻塞 HIGH

- [x] **S0 契约修订** —— 6 点全落地：§0 桌面 controller、§7 raw-byte、新增 §7.0 JSON-RPC 信封、§7.3 D9 握手、§7.4 D10 快照、§9 收口、§11 ridge-core 归属（GM D-GM-1 改 packages/）。商业化语义未弱化（20+ 关键词在）。`docs/contracts/ridge-cloud-protocol.md`。
- [x] **S1 ridge-core 地基** —— 新建 `packages/ridge-core/`（零 Tauri 依赖，`cargo tree` 实证）+ virtual workspace 根 + `core_bridge.rs`；Ctx 四抽象面齐；dispatch stringly-typed + 能力策略层（D8，~85 白名单数据化、host 特权命令排除有单测）；迁 settings/theme 垂直片，src-tauri 薄封装（Manager 逐行核查无行为漂移）。验收：`cargo check -p ridge-core` 0err、`cargo test -p ridge-core` 20 passed、`cargo check -p ridge`(src-tauri) 0err。台账 `docs/plans/s1-migration-ledger.md` 覆盖剩余 11 文件。
- [x] **S2 Transport L1/L2** —— `src/lib/transport/remote/{types,jsonRpc,rpcClient,lanWsAdapter}.ts`；bridge.ts 去 RemoteConnection 硬依赖；L2 RPC 超时/cancel/重连-reject（不重放）；JSON-RPC 字段与 S0 §7.0 逐字一致。验收：`pnpm check` 0err/0warn（=baseline）；自带 37 单测过（全量 5 既有失败与 S2 无关，stash 复验）。
- [x] **Manager gap 复核** —— 三子项 PASS；报告 `docs/plans/wave1-gap-report.md`；HIGH 发现（错误码损耗链）→ GM D-GM-2 处置 + TODO(S3) 锚点。

### 待用户/后续会话
- **S1 桌面运行回归**：本机 rebuild 杀会话 + cdylib `0xc0000139`，会话内只到 `cargo check`；settings/theme 三命令运行时回归（启动主题、默认 cwd、远控 invoke）须用户在本机 rebuild 验证。
- **workspace 构建验证**：`tauri build` / wasm-pack 产物布局（target 迁根、ridge-term release profile hoist）须用户确认；通过后再清理 3 个旧 per-crate Cargo.lock（D-GM-3）。

### Wave 2 完成（S3，2026-06-04）—— 向后兼容、GM 独立复跑 conformance 全绿

- [x] **S3 统一线协议骨干**（owner s3-protocol）：
  - server.rs **invoke 双形态收发**（legacy + JSON-RPC，对称回复，legacy 路由一字未动）。
  - **D-GM-2 解除**（JSON-RPC 腿透传 `CoreError.to_json_rpc()` 的 `{code,message,data}`；两处 TODO(S3) 锚点更新；legacy 腿仍 message-only）。
  - **D9 `$/hello`** 握手（host `negotiate_hello` + client `rpcClient.hello()`/reconnect 重握手/`hasCapability`）。
  - **`$/cancel`**（per-conn 取消登记）、**事件背压**（broadcast arm coalesce 同名取最新，防 §5.2/R8 OOM）。
  - **D10 scaffold**：`PaneSnapshotFrame` 类型 + subscribe 接入点 + S5 实现要点（全量屏缓冲切 S5，D-GM-6）。
  - **S7 conformance（LAN-WS arm）**：`conformance.test.ts`(17) + `lanWsAdapter.test.ts`(+5) + Rust `jsonrpc_tests`(6)。
  - 验收：`cargo check -p ridge` / `--tests` 0err/0warn、clippy 新增段 0 警告、`cargo test -p ridge-core` 20 passed、`pnpm exec vitest run transport/` **58 passed**（GM 独立复跑确认）、`pnpm check` 0/0。
  - 移交：桌面浏览器经 LAN 端到端运行回归（老客户端仍 invoke、握手后 error 带 code/data、事件风暴不卡、$/cancel 取消搜索）须用户 rebuild 验证。

### Wave 3 完成（S4-client / S5-MVP / S8 / R12，2026-06-04）—— Manager 合并复核全 PASS，零跨切面漂移

- [x] **S5 headless ridge-cli MVP（tree+search 切片）**：`fs::{search,tree}`(901行) 下沉 ridge-core 单一真源，src-tauri re-export（行为不变）；ridge-core dispatch 增只读 fs/search/tree 命令面 + D8 白名单；ridge-cli 接 `ridge_core::dispatch`、废除 `fs_reuse` 重复实现，**`cargo tree -p ridge-cli` 无 tauri（R3 实证消除）**。验收：cargo check ×3 crate 0err、`cargo test -p ridge-core` 40 / `-p ridge-cli` 29 passed。
- [x] **S4 cloud-WebRTC L1 适配器（客户端切片）**：`cloudWebrtcAdapter`+`cloudMux`（1 字节前缀 mux，paneId 帧头 D-GM-7），E2EE/auth 由 provider 负责；同一 bridge+RpcClient 可跑 WebRTC。验收：svelte-check 0、vitest transport/ 88 passed。
- [x] **S8 安全切片**：fs root-scoping 沙箱（`OutsideSandbox` code 1006，空根=不限制向后兼容）+ shim 全量审计报告（`s8-shim-audit.md`）。验收：cargo test -p ridge-core 57 passed。
- [x] **R12**：补 `@tauri-apps/plugin-opener` shim + vite alias + 修 `linkResolver` 远控外链静默失效。验收：pnpm check 0、opener.test 4 passed。
- [x] **契约 SSOT 登记**：D-GM-7 paneId 布局、D-GM-8 错误码表（防 R1 漂移）。
- [x] **Manager 合并复核**（`wave23-gap-report.md`）：S3/S4/S5/S8/R12 全 PASS，跨切面一致性矩阵（错误码 1000-1006 / D9 capabilities `[pane,invoke,fs,git,search,workspace,theme]` / D8 白名单 / paneId 债 / 向后兼容）**逐值零漂移**；唯一 LOW = D-GM-9（headless 沙箱 no-op，已记硬门）。

### 验证总账（本会话内可达上限）
- **静态/单元/构建全绿**：cargo check（ridge-core/ridge/ridge-cli）、cargo test（ridge-core 57 / ridge-cli 29）、vitest transport 88 + opener 4、svelte-check 0/0、**`pnpm build:desktop-web` 生产构建 exit 0**（1m44s，写出 web-remote-dist）。
- **chrome-devtools 前端 e2e 烟测**：本地静态服 web-remote-dist → 真浏览器加载 → 门面正确渲染（"Ridge Remote"+TOTP 输入）、**console 0 error/0 warn**；证据 `docs/plans/wave1-3-webremote-smoke.png`。说明：旧 ridge.exe=旧后端，无法反映新后端改动；本烟测验证前端 shipping bundle 加载/路由/别名运行时无误，**不替代后端运行时回归**。

## 本会话结束态（"结束"）：已驱动至自治可行边界

**已交付并提交（develop，本地未 push，共 ~12 笔 commit）**：S0 契约 · S1 ridge-core 地基 · S2 Transport 分层 · S3 协议骨干 · S4 cloud 客户端适配器 · S5 headless MVP · S8 安全 · R12 · 契约登记 · GM 编排/台账/gap 报告。**8 个子项均有已验证的实质增量。**

**必须用户在本机 rebuild 验证（本机 rebuild 杀会话 + cdylib 0xc0000139，无法在会话内运行）**：
1. 桌面 app 全功能回归（S1 settings/theme 薄封装 + S5 fs 下沉零行为变化）。
2. `tauri build` / wasm-pack 产物（workspace 根 + target 迁移 + ridge-term profile hoist）；通过后清理旧 per-crate Cargo.lock（D-GM-3）。
3. LAN 浏览器远控端到端（S3 双形态 invoke、$/hello 握手、error 带 code/data、事件背压、$/cancel、R12 外链）。

**真正的外部阻塞（非本仓库/需运行时基建，非本会话可完成）**：
- **S4-host**：onFrame 接通 + host 侧 paneId 编码器对齐 + Rust(webrtc-rs) 迁移 + **E2EE 密钥认证核实（尚无实现核实）**——需 live cloud relay + WebRTC e2e。
- **S6 cloud 入口**：跨仓库 `C:\code\ridge-cloud`（CDN/code-split/兜底下发）——超出 `C:\code\wind` 范围。
- **剩余 handler 迁移**：git/terminal/workspace/pane（绑 D11 领域模型）+ 全量 D10 屏缓冲 + fs **写**命令（前置 D-GM-9 沙箱硬门）——见 `s1-migration-ledger.md`，留后续会话（Rust，cargo check 可验）。

## Phase A 运行时验证已完成（2026-06-04，用户授权本机构建+实跑，已不再是 rebuild 门）

GM 在本机做了完整运行时验证（Claude 在 Windows Terminal 非 ridge 托管，构建/启动 ridge 不杀会话）：
- **release 构建**：先修复 S1 workspace 重锁导致的 tauri 生态版本漂移（pin 回 2.10.x 全族，commit `ce8c72a`），`pnpm tauri build --no-bundle` exit 0，产出内嵌前端 ridge.exe（19.5MB）。
- **桌面回归**（chrome-devtools 接 WebView2 CDP :9222）：新后端启动，文件树（S5 迁入 ridge-core 的 fs 命令）、git（develop ↑14）、终端全部正常 → **S1/S5 桌面零行为变化实证**。
- **LAN web-remote e2e**（新后端 :9527 + TOTP 鉴权 + node WSS 客户端）：
  - ✅ D9 `$/hello` 握手：host 回 `{protocolVersion:1, capabilities:[pane,invoke,fs,git,search,workspace,theme]}`。
  - ✅ JSON-RPC invoke 迁移命令 `path_exists` → `{result:true}`（S3 JSON-RPC-native leg + ridge_core::dispatch + S5 命令 全链路）。
  - ✅ **D-GM-2 结构化错误码实证**：迁移命令 `read_file` 带 `..` → `{error:{code:1003,data:{kind:"path_traversal"},message:"path traversal rejected"}}`；`get_remote_info`（非迁移、host 特权）被 legacy allowlist 早挡 → `-32603 internal`（符合预期）。
  - ✅ legacy `invoke-request` 向后兼容 → `{_result:true}`（D-GM-4 实证）。
  - ✅ 事件推送带 S3 `coalesced` 字段（背压路径活）。
  - `text_search` 参数名为 `root`（非 `path`），命令本身正常（与 project.rs 行为一致）。
⇒ **S1/S3/S5 在运行时验证通过**。剩 `tauri build` 完整 bundle（installer，非阻塞）+ S4-host/S6 见下。

## Phase D — 现存 e2e + perf 回归（用户追加，2026-06-04）

- **单元套件 `pnpm test`（vitest 全量）**：**577 passed / 5 failed**。5 个失败全在 `src/lib/terminal/{workerRendererSingleton,renderWorker,workerRendererBridge}.test.ts`（P4.9 worker-rendering）。
  - **研究结论**：`isWorkerRenderingEnabled()` 实现**故意 `return false`**（源码 NOTE：worker 渲染在其自带 wasm kernel 能产出真实像素前默认关闭）；这 5 个测试断言默认 `true`，是**独立的 P4 render-decouple 在制功能**（worktree `p4-ipc-render-decouple`）的测试**跑在实现之前**。
  - **与本次 unified-remote 改动无关**（S2/S3 executor 早已 stash 复验为既有失败）；**本次改动新增失败数 = 0**。
  - **不修复（正确判断）**：把默认翻成 `true` 以满足测试会**破坏终端渲染**（worker 还没 wasm kernel），是错误修复；正确解 = 落地完整 P4.9（独立多会话工作）或把超前测试标 skip（属 P4 团队 TDD，不应越界动）。故保留现状并诚实记录。
- **playwright e2e（`pnpm e2e`，tests/e2e 浏览器 smoke）**：首跑 8✓/1✗（test#1 左栏 `toBeVisible` 10s 超时，因 vite 在我构建后冷启动 re-optimize deps；test#3 用同一批 rail 按钮却通过）→ **暖跑全绿 9 passed**，确认是冷启动瞬时 flake，非回归（我的 `+layout.svelte` 改动只影响 WEB_REMOTE 分支、不碰普通 dev 渲染）。
- **wdio e2e:shell（tauri-driver 驱动原生 app）**：**首跑 0/10 全失败 = "never left about:blank"**。研究结论分两层：
  - **(我的回归，已修)** workspace 重定位把 target 移到根，但 `wdio.conf.ts` 仍指 `src-tauri/target/release/ridge.exe`（旧二进制）+ perf 脚本进程过滤仍按 `src-tauri\target\` → 已全部修复（commit `7be7381`：wdio binary path、perf-bench/compare 过滤、package clean、terminal.rs 调试 shim 走 ancestor walk、README）。
  - **(预存在、非我的回归)** 即便指向正确的新 release 二进制，仍 about:blank：根因是 wdio.conf §1.35 注释记录的 **WebView2 user-data 独占锁 / about:blank 导航挂起**（与已安装 `C:\Program Files\ridge\ridge.exe` 共享 identifier）。我**重新启用了被前人调试时移除的 `WEBVIEW2_USER_DATA_FOLDER` 隔离**，但 about:blank 仍在（139 次轮询）→ 确认是更深的 tauri-driver/WebView2(148) BiDi 自动化导航挂起，**前人正在调试、超出 unified-remote 范围、非本次回归**。
  - **app 本身已验证**：Phase A 手动启动 + chrome-devtools 全 UI + LAN WSS e2e 全绿，证明应用与我的改动在运行时正确；失败仅在 tauri-driver 自动化壳。
- **perf（perf-bench/compare/frame）**：`e2e:perf` 走同一 wdio 原生壳 → 同 about:blank 阻塞；我已修进程过滤使其能定位 app，但壳的导航挂起预存在。我的改动是后端协议层（非渲染热路径），perf 影响概率低。**诚实标注：perf 对比受 wdio 壳预存在问题阻塞，未取得新基线。**

### Phase D 结论
- **单元 + playwright e2e 证明本次 unified-remote 改动零回归**（577✓ / 9✓；5 个 vitest 失败=预存在 P4.9 WIP；1 个 playwright 失败=冷启动 flake，暖跑全绿）。
- **我的 workspace 重定位引入的 e2e/perf 路径回归已全部修复并提交**（`7be7381`）。
- **wdio/perf 原生壳的 about:blank 是预存在、在调试中的 WebView2/tauri-driver 工具链问题，非本次回归**；app 运行时正确性已由 Phase A 实证覆盖。

## Cloud 功能闭环（Wave 3 / S4-full + S6）——代码完成 + 集成验证至网络边界（2026-06-04）

补齐了 cloud 闭环缺失的全部代码并做了能做的 live 集成验证：
- **代码全部提交**（develop，19 commit ahead，未 push）：S4-client `cloudWebrtcAdapter`+`cloudMux`（`efd6706`）、S4-host `cloudHostBridge`（`fd28768`）、controller-side `controllerCloudProvider`(offerer)+`cloudControllerBoot`+`+layout ?cloudHost` 分支（`bade1cc`）。140+ vitest 全过，svelte-check 0/0。
- **device pairing 真打通**（live 云后端 API）：创建测试账号 `s4-test@ridge.test` → DB 升级 premium + username `s4test`（用户授权 SSH/DB）→ `/device/code`+`/device/activate`+`/device/poll` 拿到 device JWT（tenant `s4host-s4test.remo2ridge.duckdns.org`）。
- **host 集成验证至网络边界**：重建 ridge（内嵌 cloudHostBridge）→ chrome-devtools(WebView2 CDP) 注入 cloud auth(localStorage) → CloudPanel「官方公网加速」**正确识别已配对设备 + 显示专属公网入口** → 点「建立加速连接」→ **RidgeCloudProvider 代码真的跑起来并发起 fetch（带正确 device token/tenant）**。
- **唯一阻塞 = 本机 WebView2 无外网**（环境问题，非代码/架构）：WebView2 内 `fetch` 连 `example.com`/relay 全 `Failed to fetch`，而 shell `curl` 同域名 200；`--no-proxy-server` 也未解 → 本机 ShellCrash/Tailscale 代理/DNS/防火墙对 WebView2 进程的网络拦截。
  ⇒ **cloud 闭环代码完成 + 集成正确接线（host 识别设备并发起连接）；live WebRTC e2e 受本机 WebView2 外网阻塞**，需在 WebView2 有外网的机器/网络上跑，或修本机 WebView2 网络（环境项，超出 unified-remote 范围）。
- **仍为文档化的后续**：pane PTY 流真实接入（D-GM-11）、E2EE 公钥↔身份绑定（D-GM-10，跨仓库）。

### 🎉 Live cloud e2e 打通（2026-06-04）—— 根因是 CSP，非机器网络

补充发现纠正了之前的"WebView2 无外网"误判：根因是 **SPA `app.html` 的 CSP `connect-src` 只允许 `'self'/ipc/ws://localhost`**，拦掉了一切外网 fetch/WSS（连 example.com 都 fail）。修复 `connect-src` 加 cloud relay 域名（commit，已提交）后：
- **host WebView2 fetch relay → 200**（CSP 解禁实证）。
- **device pairing live 打通**：premium 升级 + `/device` 流 → device JWT（room `s4hostb-s4test`）。
- **host 经云连上信令**（建立加速连接 → 连接中，等待 controller）。
- **controller（浏览器，由无后端的静态服务器 :8810 托管，`?cloudHost=s4hostb&u=s4test` 进 cloud 模式）经 relay + WebRTC + E2EE 连上 host** → bridge `$/hello` 协商 → **`get_file_tree` invoke 经云往返，controller 渲染出 host 的真实仓库文件树**（`.baseline/.codegraph/.kiro/packages/src-tauri/web-remote-dist` 等 = C:/code/wind 实际内容）。
  - **铁证**：controller 由纯静态文件服务器托管（无 backend/WS/invoke 能力），其显示的 host 文件树只可能来自 cloud→host 的 WebRTC 连接。⇒ **cloud 功能闭环 invoke 往返 live 验证通过**。
- 证据截图：`docs/plans/cloud-e2e-controller.png`。
- **次要待办（非阻塞）**：① 展开 `docs` 子目录时 `get_directory_children`（懒加载）经云返回"空目录"——cloud 路径下子目录懒加载的小 bug（根树 `get_file_tree` 正常），待查参数传递；② D-GM-11 pane PTY 流、D-GM-10 E2EE 身份绑定仍为后续；③ 注意：live e2e 需 host+controller 同时在线（host 信令空闲会超时断），编排时序敏感。

### S6 公网下发部署 —— 代码完成 + 本地提交，部署受阻于 ridge-cloud 远端历史分叉
- ridge-cloud 实现 + 本地提交 `fff01da`：host-label 路由 `app.remo2ridge.duckdns.org` → `desktop-app/`（刷新为 CSP-fixed web-remote-dist）；主域名/`/api`/`/ws` 不变；指纹缓存；Dockerfile 纳入 desktop-app。`cargo check`(ridge-cloud) 0err。
- **app.* DNS+TLS 已就绪**（duckdns 通配 + `*.remo2ridge.duckdns.org` 证书；curl app.* = 200/cert valid）——无需额外 ops。
- **部署受阻（非本次代码问题）**：`git push dokku main` 被拒（non-fast-forward）；fetch 后发现**本地 clone 与已部署 dokku/main 历史分叉**（ahead 4 / behind 1、根 commit 不同 = E 组曾 force-push/re-init）。生产仓强行 reconcile 风险高（可能破坏在线服务或丢 E 组已部署 web/）——**未强推**，已恢复 E 组 stash、服务 health 200 完好。S6 部署 = 待与 E 组对齐 ridge-cloud 历史后 `git push dokku main`（代码 + DNS/cert 均就绪）。
