# Wave 1 Gap 复核报告（Manager → GM）

> 复核者：Manager agent（只读复核，未改业务代码）。日期：2026-06-03。
> 判据：[`unified-remote-architecture-handoff-final.md`](./unified-remote-architecture-handoff-final.md) §4 D6–D11、§5.1–§5.3、§6 S0/S1/S2 验收标准；GM 决策 D-S1-1 / D-GM-1（[`orchestration-log.md`](./orchestration-log.md)）。
> 复核手段：`cargo check -p ridge-core`（exit 0）、`cargo test -p ridge-core`（20 passed）、`pnpm check`（svelte-check 0 error / 0 warning / 4464 files）、`cargo tree -p ridge-core`（无 tauri/webview/wry/tao）、逐文件 diff 阅读、契约关键词 grep。
> 环境约束（诚实标注）：src-tauri `cargo check` ~10min 未重跑（依 GM 授权只读 S1 报告 + 抽查 diff）；`cargo test --lib`（ridge cdylib）本机 0xc0000139 跑不起来；**完整桌面运行回归须用户本机 rebuild 验证**。

---

## 总览结论

| 子项 | 结论 | 一句话 |
|---|---|---|
| **S0 契约修订** | **PASS**（1 项 LOW） | 6 改动点全部落地，§7.0 信封与 S2 代码逐字一致，商业化语义未弱化，无残留 `crates/`/postcard 过时硬声明。 |
| **S1 ridge-core 抽取** | **PASS（地基范围内）**（2 HIGH 跨子项 + 2 MEDIUM） | crate 零 Tauri 依赖（`cargo tree` 实证），4 抽象面齐，dispatch stringly-typed + 能力策略层到位，垂直片薄封装无行为漂移；HIGH 项是**跨子项语义损耗链**与**桌面运行回归未验**，非 S1 内部缺陷。 |
| **S2 Transport 抽象** | **PASS**（1 HIGH 跨子项 + 1 MEDIUM + 1 LOW） | bridge.ts 已彻底去 `RemoteConnection` 硬依赖，L1/L2 分层到位，RPC 超时/cancel/重连 reject 单测齐全，`pnpm check` 绿。 |

**放行建议速览**：S0、S1、S2 三者主体**均可 commit**（按"每功能点一 commit"）。无 CRITICAL。两条 HIGH 均为**跨子项协调项**（错误码端到端损耗、桌面运行回归），不阻塞 commit，但须 GM 记录并在 S3 强制收口 + 用户 rebuild 验证后才能宣称"S1 零行为变化"。

---

## S0 契约修订 —— 逐项

| 子项 | 判据 | 结论 | 证据 / 备注 |
|---|---|---|---|
| §0 桌面 controller | 接纳桌面浏览器完整控制形态 | **PASS** | §0 第 17-24 行：拆"移动 controller / 桌面 controller"，注明 LAN UA 分流 vs cloud 公网下发 + D9 对账。原"复用现有移动端 SPA"已扩写，无残留单一表述。 |
| §7 raw-byte 引子 | postcard delta → raw-byte | **PASS** | §7 第 204-211 行：`0x10 PANE_RAW` / `0x11 JSON` 1 字节 mux，引 `.kiro/specs/remote-raw-byte/` OOM 来由。 |
| §7.0 JSON-RPC 信封 | 新增成节，字段与 S2 逐字一致 | **PASS** | §7.0 第 219-224 行 vs S2 `types.ts` 第 19-24 行：`{jsonrpc,id,method,params}` / `{...,result}` / `{...,error:{code,message,data?}}` / notification 无 id / `$/cancel` params `{id}` —— **逐字一致**。 |
| §7.3 D9 握手 | 成节 | **PASS** | §7.3：`$/hello` notification + `protocolVersion` 取最高公共版本 + `capabilities` 取交集 + `$/bye` 拒绝；明确与 D8 准入正交。 |
| §7.4 D10 快照 | 成节 | **PASS** | §7.4：`subscribe-pane` 首响应为屏幕快照、per-pane 缓冲、重连重发、锁定渲染尺寸为共享属性随快照下发。 |
| §9 raw-byte 收口 | 去 postcard schema 硬声明 | **PASS** | §9 第 322-323 行："不再使用 postcard 增量协议帧"，旧"schema 不改"已删并加评审说明。 |
| §11 ridge-core 归属 | 增 F 行 + packages/ 而非 crates/ | **PASS** | §11 表新增 **F. ridge-core crate** = `packages/ridge-core/`；D-GM-1 明确**不**新增 `crates/` 源码根；新增 5 条归属规则 + C/F 共享接触点（根 Cargo.toml + src-tauri/Cargo.toml）约定。grep 确认全文 `crates/` 仅出现在"不新增 crates/"语境，无遗漏旧归属。 |
| 商业化语义未弱化 | §3 JWT / §4 激活 / §10 部署 | **PASS** | grep `device JWT`/`user JWT`/`premium`/`scope=`/`激活`/`Dokku`/`部署` 等 20 处全部保留；S0 改动集中在 §0/§7/§9/§11，未触及 §1-§6 身份鉴权与 §10 部署条款。 |
| "改契约在先"标注 | 各改动点标注 | **PASS** | 每个评审块均带"（待跨团队确认）"，与计划 §6 S0 验收"经跨团队确认"对齐。 |

**S0-LOW-1（LOW）**：§7.0 说明段称"业务错误码映射进 JSON-RPC `error.data`，`error.code` 按 JSON-RPC 规范用于协议级错误"，但 S1 `error.rs` 实际把应用错误码放在**正整数 1000-1005**（`error.code` 直接承载业务语义，`data.kind` 才是 tag）。契约文字与 S1 实现的 code 语义分工**措辞略有出入**（契约暗示 code 仅协议级，S1 让 code 也带业务级）。不影响互通（两侧都是合法 JSON-RPC），但 S3 定稿协议时应让契约 §7.0 与 S1 error.rs 的码表对齐措辞。文件：`docs/contracts/ridge-cloud-protocol.md:227-228` vs `packages/ridge-core/src/error.rs:20-33`。

---

## S1 ridge-core 抽取 —— 逐项

| 子项 | 判据 | 结论 | 证据 |
|---|---|---|---|
| ① `cargo check -p ridge-core` | 能过 | **PASS** | 亲自重跑 exit 0，0 warning。`cargo test -p ridge-core` 20 passed。 |
| ① `cargo check -p ridge`（src-tauri） | 能过 | **未验（环境约束）/ 抽查 PASS** | 见 S1-HIGH-2。薄封装 diff 逐行读：签名映射、`to_command_string()` 回退、re-export 类型保留，编译面无明显断点；但本机 ~10min check 未重跑。 |
| ② ridge-core 无 tauri/async_runtime 依赖 | 硬约束 | **PASS** | grep 命中全是注释/文档；`cargo tree -p ridge-core` 无 tauri/webview/wry/tao；`Cargo.toml` 仅 serde/serde_json/tokio(rt,sync,macros)/thiserror/tracing。`TokioSpawner` 直依 `tokio::spawn`，未经 `tauri::async_runtime`。 |
| ③ Ctx 四抽象面 | 状态/事件(广播vs单连接)/spawn/错误 | **PASS** | `ctx.rs`：① `CoreState`(Arc + downcast)；② `EventSink` + `EventScope::{Broadcast,Connection}`（落 D11）；③ `TaskSpawner`/`TokioSpawner`（tokio 直依）；④ handler 返回 `CoreError`，`error.rs` 双边界映射（`to_command_string` / `to_json_rpc`）。`Ctx` 全员 `Send+Sync`、`Clone`、per-request 构造，符合 §5.1。 |
| ④ dispatch stringly-typed + 能力策略层 | D-S1-1 + D8 | **PASS** | `dispatch.rs`：`dispatch(method:&str, args:Value, ctx)`，三步检查（能力准入→路径穿越守卫→方法表），与 legacy 顺序一致。`capability.rs`：`REMOTE_ALLOWLIST` 数据常量（~85 命令名），`remote_default()` / `allow_all()` / `from_methods()`；单测验证 host 特权命令（get_remote_info/set_remote_enabled/...）被排除。 |
| ⑤ 垂直片薄封装只委托、无行为漂移 | settings/theme/server | **PASS（含 1 项已澄清）** | 见下方"行为漂移核查"。 |
| ⑥ 台账覆盖剩余 11 文件 | 完整性 | **PASS** | `s1-migration-ledger.md` §2 列全 11 文件（git/project/process/workspace/pane/terminal/ridge_file/fs_watch/watch/remote + deep_root），每文件含 State/AppHandle 触点计数、归类、策略、风险；§3 跨切面待办（read-only gate 下沉、路径守卫、白名单同步、事件双路由）；§4 共存策略保 LAN 绿。 |

### 行为漂移核查（S1 ⑤ 重点）

- **`set_user_default_cwd`（曾疑似漂移，已澄清为 PASS）**：`core_bridge.rs:83` 用 `HostStateAccessor(Arc::new(state.clone()))`，初看 `state.clone()` 可能复制出独立锁。但核实 `state.rs:475` `user_default_cwd: Arc<RwLock<Option<PathBuf>>>` 是 **Arc 内部共享**，clone `AppState` 只增 Arc 引用计数、共享同一把锁，写入仍反映到原 state。**无漂移**。（前提：`AppState` 派生 Clone 且所有可变字段均 `Arc<...>`；从 state.rs 抽查 user_default_cwd/pty_pane_registry/remote_port 均符合。）
- **`theme.rs`**：`get_theme_data` 保留 `AppHandle` 的 `Resource` 解析（比 ridge-core 的 no-handle 祖先回溯更全，故**刻意未委托**，桌面行为不变）；`set_active_theme` / `read_active_theme_id` / `active_theme_entry_no_handle` 纯委托 core；`LoaderConfig`/`ThemeEntry`/`ThemeFile`/`ACTIVE_THEME_FILE` re-export 保留原公共面。`build_splash_init_script` 仍用 `AppHandle`（桌面专有，台账已标"保留 src-tauri"）。逐行 diff 端口逻辑一致（trim/empty 归一、default fallback、空 catalog 回退、错误串原样）。**无漂移**。
- **`server.rs::dispatch_invoke_request`**：三命令改 `core_result_to_envelope(ridge_core::dispatch(...))`，`Ok→{_result}` / `Err→{_error: to_command_string()}`，与 legacy `{_result|_error}` 信封一致；read-only gate（`is_mutating_invoke`，server.rs:2195）仍在入口生效，三命令非 mutating 不受影响。**无漂移**。

### S1 GAP

- **S1-MEDIUM-1（MEDIUM）**：`remote_ctx(app,state,connection_id)` 签名收 `connection_id`，但 server.rs:2400 调用硬编码字符串 `"remote"`，且 `DesktopEventSink::emit` 对 `EventScope::Connection` 仍走 `AppHandle::emit`（注释承认"per-connection routing 待 S3/S4 传输层带 connection id"）。这是**计划内的 S1 占位**（台账 §3.4 明列），非缺陷；但 D11 单连接精确路由在 S1 实为 no-op，须确保 S3/S4 真正接上，否则多浏览器连接时焦点/选区事件会广播串扰。建议 GM 在 S3 验收清单显式列入。
- **S1-MEDIUM-2（MEDIUM）**：`Ctx::spawner` / `TaskSpawner` 抽象已建但**垂直片无任何 handler 使用**（settings/theme 无后台任务）。即 spawn 面是"已声明未行使"。同理 `EventSink` 也无垂直片 handler emit。属正常分期（地基先于行使），但意味着"tokio 直依、不经 async_runtime"这条 R3 硬约束**尚未被任何运行路径实证**，仅靠依赖树证明。fs_watch/watch 迁移时才会首次行使，须在那一刻验背压（R8）。

---

## S2 Transport 抽象 —— 逐项

| 子项 | 判据 | 结论 | 证据 |
|---|---|---|---|
| ① bridge.ts 无 `RemoteConnection` import | grep | **PASS** | diff 确认 `import { RemoteConnection }` 已删，改 `import { RpcClient }` + `import type { ChannelTransport, ControlFrame, Unsubscribe }`。`attach()` 收 `ChannelTransport`；`+layout.svelte` 改 `bridge.attach(createLanWsTransport(conn))`。bridge 内已无任何 `RemoteConnection` 引用。 |
| ② L1/L2 分层 | request/超时/cancel/重连 reject | **PASS** | `types.ts`：L1 `ChannelTransport`（sendControl/onControl/sendPaneBytes/onPaneBytes/connect/close/state/onStateChange）+ L2 接口。`rpcClient.ts`：`request()`（id 关联+超时）、`cancel(id)`+AbortSignal（发 `$/cancel`）、`handleStateChange` 在 `connected→(reconnecting|disconnected|error)` reject 全部 in-flight（`RpcReconnectError`，不重放）、`onReconnected` resync hook。`rpcClient.test.ts` 覆盖 envelope/correlation/乱序/未知 id/超时/cancel/abort/reconnect-reject/不重放/notification/dispose——14 用例，齐全。 |
| ③ JSON-RPC 字段与 S0 §7.0 一致 | 三方一致 | **PASS** | 见 S0 表；`jsonRpc.ts` builder/guard 与契约信封同形。 |
| ④ `pnpm check` 0 error | svelte-check | **PASS** | 0 error / 0 warning / 4464 files / 0 files-with-problems。 |
| ⑤ lanWsAdapter 线协议翻译保 LAN 不变 | 行为等价 | **PASS（含 1 LOW）** | `toWire`：JSON-RPC request→`{type:'invoke-request',cmd,args,_reqId}`、notification→`{type:method,...params}`、`$/cancel`→`{type:'cancel',_reqId}`；`handleInbound`：`{type:'invoke-result',_reqId,_result|_error}`→JSON-RPC response，其余帧（含 `{type:'event'}`）verbatim 透传。bridge 经 `onControl` 仍收 `{type:'event'}` 走 `emitLocal`，RpcClient 对无 jsonrpc 字段的 event 帧忽略——双消费不冲突。**LAN 线协议字节不变**。 |

### S2 GAP

- **S2-MEDIUM-1（MEDIUM）**：`LanWsAdapter.handleInbound` 把所有 `_error` 一律映射为 `JSON_RPC_ERRORS.INTERNAL_ERROR`（-32603），丢失 host 侧的错误分类。配合 S1 server.rs 只回 `{_error: message}`（已丢 code/data），形成端到端**错误码全损耗**——见 S2-HIGH-1（跨子项）。adapter 测试覆盖**已确认充分**：`lanWsAdapter.test.ts` 14 用例覆盖 outbound 双向翻译（request/notification/params 展开/`$/cancel`/legacy 透传）+ inbound（`_result`/`_error`/null/event 透传）+ pane bytes + lifecycle（state/close），无需 GM 再抽查。
- **S2-LOW-1（LOW）**：`bridge.ts` 与 `lanWsAdapter.ts` 均保留 `console.error`（listener throw 兜底）。与全局规则"production 无 console.log/debug"擦边，但属错误兜底日志而非调试语句，且前端无统一 logger，**可接受**；S8 可观测层（tracing）落地时统一。
- **D9/D10 未实现（非 gap，须标注）**：`rpcClient.ts:14` 注释提及"D9 handshake consumed via onControl notifications"但**无 `$/hello` 实现**；`onReconnected` resync hook 与 bridge 的 re-subscribe 已为 D10 预留接口，但**快照重拉无实体**。这**符合计划**——D9/D10 是 §6 S3 验收项，S2 验收只要 L1/L2 + LAN-WS + RPC 单测。提请 GM 勿据 rpcClient 注释误判 S2 已含 D9/D10。

---

## 跨子项分歧清单（GM 重点）

### 分歧 1 —— 错误码端到端损耗链（HIGH，跨 S0/S1/S2）

> 标号：**S1-HIGH-1 / S2-HIGH-1**（同一条链，双记）

S1 `error.rs` 精心设计了稳定 JSON-RPC 码表（MethodNotFound=-32601、InvalidArgs=-32602、CapabilityDenied=1001、ReadOnly=1002、PathTraversal=1003、HostUnavailable=1004、Io=1005、Internal=1000）+ `data.kind` tag，并在文档自称"S7 parity 套件将断言其稳定性"。**但当前 LAN 路径完全用不到它**：

1. `server.rs::core_result_to_envelope` 把 `Err` 压成 `{_error: e.to_command_string()}` —— **只取 message，丢弃 code 与 data.kind**。
2. `lanWsAdapter.handleInbound` 把入站 `_error`(string) 重新包成 `{code:-32603, message}` —— **凭空贴 INTERNAL_ERROR，与 S1 原始 code 无关**。
3. 结果：S1 的 `CapabilityDenied(1001)` / `ReadOnly(1002)` / `PathTraversal(1003)` 经 LAN 往返后，client 侧 `RpcRemoteError.code` 一律是 -32603，`data.kind` 丢失。

**严重度判定 HIGH（非 CRITICAL）**：不破坏功能（message 仍在，UI 可显示），不破坏 LAN 现状（legacy 本就只有 message）。但它使 S1 精心建立的"跨 host 稳定错误码"在 LAN 腿上**形同虚设**，且 S7 的 parity/conformance 套件若断言 code 会直接挂。这是 raw-byte+JSON-RPC 统一协议的真实语义损耗。
**根因**：当前 WS 信封（legacy `{_result|_error}`）是 message-only 的，JSON-RPC 结构化 error 要等 S3 把 LAN host 也升级为 JSON-RPC-native 才能端到端透传。S1/S2 各自做了"对接 legacy 信封"的妥协翻译，方向正确，但**没有任何一方留下显式 TODO 锚点**把这条损耗链标到 S3。
**修复建议（给 GM）**：
- 不阻塞 S0/S1/S2 commit。
- 在 S3 spec 显式列一条任务："LAN host 升级 JSON-RPC-native 时，server.rs 直接发 `CoreError::to_json_rpc()` 的 `{code,message,data}`，lanWsAdapter 的 `handleInbound` INTERNAL_ERROR 兜底翻译随之删除"。
- S7 conformance 套件断言 error.code 前，须先确认 S3 已收口此链，否则套件会误判 S1。
- 建议 GM 在 orchestration-log 记一条决策点：S1 error.rs 码表是"为 S3 准备的、当前 LAN 腿暂不透传"，避免后续 agent 误以为已端到端生效。

### 分歧 2 —— JSON-RPC `error.code` 语义分工措辞（LOW，S0 vs S1）

见 **S0-LOW-1**：契约 §7.0 措辞暗示 `error.code` 仅承载协议级错误、业务码进 `data`；S1 error.rs 让正整数 code 直接承载业务语义。两侧都是合法 JSON-RPC、可互通，但措辞需在 S3 对齐。归 S3 协议定稿处理。

### 一致性确认（无分歧的对账项）

- **JSON-RPC 信封字段三方一致**：S0 契约 §7.0 ←逐字→ S2 `types.ts` 注释 ←结构→ S2 `jsonRpc.ts` builder。`$/cancel` 方法名三方一致（契约 §7.0 / types.ts CANCEL_METHOD / rpcClient.sendCancel）。**对得上**。
- **能力白名单 D8 一致**：S1 `REMOTE_ALLOWLIST`(~85) 与 server.rs legacy match 臂同源（capability.rs 注释明示镜像）；host 特权命令排除集（get_remote_info/set_remote_enabled/disconnect_session/enter_deep_root_mode/set_cloud_remote_active）与契约 §7.3 / 计划 §5.4 一致。**对得上**。
- **重连 reject 语义**：S2 `connected→(reconnecting|disconnected|error)` reject 全部 in-flight + 不重放，与契约 §7.0/§7.4"重连后 in-flight 一律 reject 再重订阅重拉快照"一致。**对得上**（快照重拉实体待 S3）。

---

## 给 GM 的放行建议（分级）

| 项 | 建议 | 理由 |
|---|---|---|
| **S0 契约** | **可直接 commit** | 6 改动点全落地，无 CRITICAL/HIGH，仅 1 LOW（措辞，归 S3）。 |
| **S1 ridge-core 地基 + 垂直片 + 台账** | **可 commit，但 commit message / log 须标注两点** | (1) "桌面运行回归未验，须用户 rebuild"（环境约束，非缺陷）；(2) error.rs 码表当前 LAN 腿不透传，S3 收口（S1-HIGH-1）。`cargo check -p ridge-core` + 20 单测 + 依赖树零 Tauri 已实证地基质量。 |
| **S2 Transport** | **可直接 commit** | bridge 去硬依赖达成，分层 + RPC 单测（14 例）+ adapter 单测（14 例）齐，`pnpm check` 绿。 |
| **需执行 agent 再补一轮** | 无 | 所有 GAP 均为 S3 范围或计划内占位，无 S0/S1/S2 内部必补项。 |
| **需 GM 拍板** | 错误码损耗链归属（S1-HIGH-1） | 记一条决策点：S1 码表是"为 S3 准备"，明确 S3 负责端到端透传 + S7 套件断言时序。 |
| **需用户 rebuild 验证** | S1 桌面全功能运行回归 | 本机 rebuild 杀会话 + cdylib 测试 0xc0000139，会话内无法运行 app；"零行为变化"在运行层面仍是**待验证**而非已验证。 |

**最终**：无 CRITICAL，无阻塞性 HIGH（两条 HIGH 均为跨子项协调/环境约束，不阻塞 commit）。三子项主体质量达 Wave 1 验收标准，**建议放行 commit**，并把上述 3 条标注（rebuild 待验 / 错误码损耗链 / S2 adapter 测试抽查）写入 orchestration-log 的执行进度与分歧拍板。
