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

### Wave 3（下一步，并行 {S4,S5,S6}；本会话能推 S5 最实）
- **S5 headless ridge-cli 完整 IDE**（依赖 S1+S3 ✓）：ridge-cli 链 `ridge-core`、补 D11 领域模型、承接 S1 台账剩余 11 文件迁移（git.rs 最易=0 State/AppHandle）、落 D10 全量屏缓冲。**Rust，cargo check 可验，本会话最有产出。**
- **S4 cloud 桌面 host**：cloud-WebRTC 适配器 + onFrame 接通 + E2EE 密钥认证；现状是 scaffold（onFrame 空、host WebRTC 在 WebView），终态迁 Rust——大且部分需运行时/WebRTC e2e。
- **S6 cloud 入口**：跨仓库 `C:\code\ridge-cloud` + CDN/code-split——**跨 repo，超出本仓库范围**。
- **S8 安全/可观测**：能力数据化(✓S1)、fs 沙箱、tracing+相关 id、shim 全量审计——可增量推进。
