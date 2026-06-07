# Wave 2/3 合并复核 + 跨切面一致性 Gap 报告（Manager → GM）

> 经理（Manager）对 Wave 1 之后已提交工作（git log 近 6 笔：S3 / S5 / S4-client / S8 / 契约 D-GM-7/8 登记 / R12）的**只读**合并复核 + 跨切面一致性审计。
> 上游：[`unified-remote-architecture-handoff-final.md`](./unified-remote-architecture-handoff-final.md) §6 验收 / §7 风险；[`orchestration-log.md`](./orchestration-log.md) 决策；契约 [`ridge-cloud-protocol.md`](../contracts/ridge-cloud-protocol.md)。
> 产出时间：2026-06-04。**本报告不改业务代码**（只读 + 写本报告）。语言约定：散文简体中文，标识符英文。

## 0. 复核范围（被复核的 6 笔提交）

| commit | 子项 | 内容 |
|---|---|---|
| `cdeec69` | S3 | JSON-RPC-native LAN host（向后兼容）：D9 `$/hello`、错误码透传、`$/cancel`、事件背压 |
| `3d2adf6` | S5 MVP | fs search+tree 收敛进 ridge-core，ridge-cli 接 dispatch |
| `efd6706` | S4-client | cloud-WebRTC L1 适配器 + 1-byte mux |
| `728eb7b` | S8 | fs root-scoping 沙箱 + SPA shim 审计 |
| `5584893` | 契约 | 登记 PANE_RAW paneId 布局（D-GM-7）+ ridge-core 错误码表（D-GM-8） |
| `b1bf0c5` | R12 | shim `@tauri-apps/plugin-opener` + 修 linkResolver 降级失效 |

复核手段已执行：`cargo test -p ridge-core` = **57 passed / 0 failed**；`pnpm exec vitest run src/lib/transport/remote/` = **88 passed / 0 failed**；`cargo tree -p ridge-cli` = **无 tauri**（R3 实证）；外加逐文件 diff 对照。

---

## 1. 各子项 PASS / GAP

### S3 统一线协议（host JSON-RPC 腿）—— **PASS**
- D9 `$/hello`：host `negotiate_hello`（server.rs:2939）取最高公共版本 + capabilities 交集；版本不足回 `$/bye`（`reason:"protocol-version-mismatch"`，非破坏关 socket，符合 D-GM-5）。client `rpcClient.hello()` + 重连自动重握手（handleStateChange）+ `hasCapability` 乐观放行。**一致。**
- 错误码透传（D-GM-2 解除）：`CORE_MIGRATED_METHODS`（server.rs:2848）经 `dispatch_invoke_jsonrpc` 调 `CoreError::to_json_rpc()` 透传完整 `{code,message,data.kind}`；未迁移 legacy 方法回 `INTERNAL_ERROR(-32603)` + message（计划内损耗，随迁移自动改善）。legacy `core_result_to_envelope`（server.rs:2490）仍 message-only，两处 D-GM-2 锚点在位。**一致。**
- `$/cancel` per-conn 取消登记（server.rs:1070/1141）、事件背压 coalesce（server.rs:2005，同名取最新防 R8 OOM）：均在位。
- D10：仅 `PaneSnapshotFrame` 类型 + `// §D10 接入点` scaffold，全量屏缓冲明确切 S5（D-GM-6）。`#[allow(dead_code)]` 标注诚实。**符合既定范围。**

### S5 headless ridge-cli MVP（fs search+tree 收敛）—— **PASS（含 1 个 LOW gap）**
- `fs::search` / `fs::tree` 算法已从 src-tauri **真正搬入** `packages/ridge-core/src/fs/`（src-tauri/src/fs/{search,tree}.rs 减 ~900 行，project.rs 薄委托）；ridge-cli `fs_reuse.rs` 经 `ridge_core::dispatch("search"/"get_directory_children")` 复用同一真源，只保留线形 DTO + 裁剪映射。**单一真源达成。**
- ridge-cli `Cargo.toml` 依赖 `ridge-core = { path = "../ridge-core" }`，**不再 path-依赖 src-tauri lib**；`cargo tree -p ridge-cli` 无 tauri → **R3 消除 Tauri 污染达成（实证）。**
- `headless_ctx()` 用 `CapabilitySet::remote_default()`，与桌面 LAN 同一 D8 白名单。
- **LOW gap（见 §3 一致性条 4）**：`headless_ctx()` 未调 `.with_roots(...)`，故 S8 沙箱对 headless **当前为 no-op**。S5 MVP 范围内可接受（search/tree 是 fail-soft 只读），但 R10 明确点名 headless 暴露整机 fs 风险最大 —— 须在 S5 后续片注入工作区根，否则沙箱对最危险的腿不生效。

### S4 cloud 桌面 host（client 切片）—— **PASS（范围内）**
- `cloudMux.ts` 实现 `0x10 || paneIdLen(u8) || paneId || raw` + `0x11 || JSON`，纯函数、双向、收发对称；demux 对结构性短帧不抛（`unknown`），与 provider per-frame reject 立场一致。
- `cloudWebrtcAdapter.ts` 是 L1 通道原语包装：解复用→onControl/onPaneBytes，state 映射含 `handshaking→connecting` 折叠（保 L2 的 connected↔reconnect 边沿正确）。**声称 role 无关、E2EE/JWT 由 provider 负责** —— 代码确实只见明文（provider.onFrame/sendFrame），不碰 E2EE/auth。**符合计划 §5.5。**
- host 侧 onFrame 接通 + Rust 迁移 + E2EE 密钥认证核实**明确不在本切片**（注释标 S4-host runtime work），属外部阻塞，见 §4。

### S8 安全（fs 沙箱 + shim 审计）—— **PASS**
- `sandbox.rs` RootScope：空根=unrestricted（向后兼容硬约束，多处单测覆盖）；词法归一化 + best-effort canonicalize 抓符号链接逃逸；symlink 边界**诚实文档化**（未存在的 write 目标无法在准入期 canonicalize，留 S4/S5 host fs 层防御）。在 `dispatch` 入口第 3 道关执行（capability→traversal→sandbox→method table）。
- shim 审计 `s8-shim-audit.md`：6 个 `@tauri-apps` 模块面枚举完整，覆盖矩阵清晰；唯一缺口 plugin-opener 已被 R12 修复（下条）。审计方法可复现。

### R12 shim plugin-opener —— **PASS（完整收口，超出审计最低要求）**
- 新增 `tauriShim/opener.ts`（`openUrl→window.open('_blank','noopener')`；`openPath/revealItemInDir` 安全降级 no-op）+ vite alias（vite.config.js:29）。
- **额外修了审计 §3.1 的高危降级失效**：`linkResolver.openShell` 判据从 `!isTauri()`（远控下恒 true）改为 `import.meta.env.RIDGE_WEB_REMOTE`，并在 catch 补 `window.open` 兜底。原生 Tauri 构建 tree-shake 该分支，桌面行为不变。`opener.test.ts` 4 passed。

---

## 2. 跨切面一致性矩阵（prompt §1-5 每条一行结论）

| # | 一致性切面 | 结论 | 证据 |
|---|---|---|---|
| 1 | **JSON-RPC 错误码端到端** | **一致** | `error.rs` 码值 1000-1006 + `-32601/-32602` ↔ 契约 §7.0 表（D-GM-8）↔ S3 `dispatch_invoke_jsonrpc` 透传 `to_json_rpc()` ↔ S2 `rpcClient` 用 `RpcRemoteError(method, frame.error)` 消费 `{code,message,data}` ↔ S8 新增 1006 `OutsideSandbox`（error.rs:52/118/153 + 契约表 1006 行 + sandbox 单测）。逐值核对**零漂移**。 |
| 2 | **D9 `$/hello`** | **一致** | host `HOST_CAPABILITIES`（server.rs:2834）= client `CLIENT_CAPABILITIES`（rpcClient.ts:51）= 契约 §7.3 = **`[pane,invoke,fs,git,search,workspace,theme]`** 逐字相同；`protocolVersion`=1 双侧；reconnect 重握手 client（handleStateChange 置 `helloSent=false`）+ host 无状态可重答。形状/版本/重握手一致。 |
| 3 | **paneId 帧布局对齐债** | **已知对齐债，已登记，不影响 LAN** | cloudMux `0x10\|\|paneIdLen\|\|paneId\|\|raw` ↔ 契约 §7 D-GM-7（明写 ridge-cli `protocol.rs` 现状单 pane 无 paneId 须对齐）↔ ridge-cli `protocol.rs:65 frame_pty_output` 现状**确为单 pane 无 paneId**。GM 已在契约 §7 + log Wave2 双处登记为 S4-host follow-up；cloudMux.ts 注释亦标记。LAN 走 WS binary（RemoteConnection.onRawBytes）不用此 mux → **不影响现网**。✅ 符合 prompt 预期。 |
| 4 | **能力白名单(D8)** | **一致（含 1 LOW gap）** | `REMOTE_ALLOWLIST`（capability.rs:129）含 fs/search/tree + `search` 别名（注释明示 = `text_search` 同 handler，dispatch.rs:206 `"text_search"\|"search"` 同臂）；host 特权命令（get_remote_info/set_remote_enabled/disconnect_session/enter_deep_root_mode/set_cloud_remote_active）有排除单测。**空根=unrestricted 向后兼容**有 4 个单测。**LOW gap**：headless `headless_ctx()` 未注入根 → 沙箱对 headless no-op（见 §1 S5 / §3）。 |
| 5 | **向后兼容** | **一致** | S3 双形态：legacy `dispatch_invoke_request` **一字未动**，host 按 `parsed["jsonrpc"]=="2.0"` 形态对称回复；client adapter 默认 legacy 翻译，收 host `$/hello` 才升级 native（lanWsAdapter.ts:189）。S5 src-tauri 薄委托（read-only fs 经 ridge-core，wire envelope 不变）。老 web-remote-dist/移动端零改动仍 invoke。R12 原生构建 tree-shake。 |

---

## 3. GAP 清单（按严重度）

| 严重度 | 标题 | 文件/位置 | 说明 + 修复建议 |
|---|---|---|---|
| **LOW** | headless 沙箱未启用 | `packages/ridge-cli/src/core_host.rs:47` `headless_ctx()` | `CapabilitySet::remote_default()` 未链 `.with_roots([工作区根])`，故 R10 最危险的腿（headless 暴露整机 fs）上沙箱是 no-op。S8 切片只交付了**能力**（dispatch 入口的 RootScope 守卫），未在 headless host 注入根。**S5 MVP 范围内可接受**（当前 ridge-cli 只暴露 fail-soft 只读 search/tree），但应在 S5 领域模型片（绑定工作区根时）调用 `.with_roots`，否则 fs 写命令一旦迁入 ridge-cli 即裸奔。**非阻塞，记 S5 收尾必办项。** |
| **NOTE** | D10 仅 scaffold（已知） | server.rs:3015 `PaneSnapshotFrame` | 全量 per-pane 屏缓冲切 S5（D-GM-6 既定）。当前 LAN 靠 64KiB scrollback 重放重绘（非渲染快照），alt-screen/光标/滚动区不精确。**符合既定切分，非新 gap。** |
| **NOTE** | ridge-cli PTY 仍本地实现 | `packages/ridge-cli/src/pty.rs:3` | 契约 §9 原意"复用 `engine::pty`"，但 src-tauri 的 `spawn_pty_reader` 绑 Tauri 事件生命周期，无头无法直接复用；ridge-cli 用本地 `PtyBridge`（TODO 注释诚实标注）。**这是 R3「零 Tauri 污染」的正确取舍**（宁可本地实现也不拖 Tauri 进无头二进制），非漂移。PTY 实现统一归 ridge-core 是后续项。 |

**无 CRITICAL / 无 HIGH。** 无"声称一致但实际漂移"的硬证据 —— 错误码、capabilities、白名单三处最易漂移的跨切面均逐值核对一致。

---

## 4. 给 GM 的收尾建议

### 可安全收口（会话内已验证，证据充分）
- **S3 / S2 / S5-MVP / S8 / R12 的代码层**：`cargo test -p ridge-core`(57) + `vitest transport/`(88) 全绿，`cargo tree -p ridge-cli` 无 tauri。错误码 / D9 capabilities / D8 白名单跨切面零漂移。这些子项的**编译 + 单测 + 一致性**层面可收口。

### 必须用户 rebuild 运行时验证（本机 rebuild 杀会话 + cdylib 0xc0000139，会话内只能到 cargo check/test）
- **S3 LAN 端到端**：老客户端仍 invoke、握手后 error 带 code/data、事件风暴不卡、`$/cancel` 取消搜索（与 Wave2 移交一致，仍未验）。
- **S5 ridge-cli 实跑**：无头机器经 cloud 提供 search/tree（需真实 WebRTC + 无头环境）。
- **R12 远控外链**：web-remote 构建下点 Markdown/终端外链在浏览器新标签打开（`pnpm check` 已过，运行时未验）。
- **workspace 构建**：`tauri build` / wasm-pack 产物布局（D-GM-3 旧 lock 清理前置）仍未验。

### 真正的外部阻塞（超出本仓库/需 e2e/需跨会话）
- **S4-host**：cloud 桌面 host 的 onFrame 接通 + host 侧 paneId 编码器对齐 D-GM-7 + Rust(webrtc-rs) 迁移 + **E2EE 密钥认证核实**（防 signaling MITM，§5.5/R10 安全硬项，尚未有任何实现核实）—— 需 WebRTC e2e，会话内无法完成。
- **S6 cloud 入口**：跨仓库 `C:\code\ridge-cloud` + CDN/code-split，超出本仓库。
- **剩余 handler 迁移**：S1 台账 11 文件（git.rs 最易=0 State/AppHandle），随 S5 领域模型片推进；迁移越多，JSON-RPC 腿错误码保真度自动越高（D-GM-2 机制）。

### 一处建议 GM 拍板的小项
- **headless 沙箱注入**（§3 LOW）：是否在 S5 领域模型片把"绑定工作区根 ⇒ `headless_ctx().with_roots(root)`"列为 fs 写命令迁入 ridge-cli 的**前置硬门**？建议是 —— 趁 ridge-cli 还只有只读 fs 时把根注入接好，避免写命令迁入后出现整机 fs 裸奔窗口。
