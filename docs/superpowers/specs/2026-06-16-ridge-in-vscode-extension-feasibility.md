# 将 ridge 内核（含 ridge-tmux / ridge-remote）植入 VSCode 扩展 —— 可行性方案与必要性调查

> 类型：可行性 + 必要性 调查报告（设计文档，非实现任务）
> 日期：2026-06-16
> 已确认形态：**完整 SPA 嵌入 VSCode webview + 子进程 `rdg` 作后端**
> 状态：调查完成，建议 **PoC 先行、门控推进**；进入实现前先据本文敲定 §九 决策点。

---

## 一、Context（为什么做这件事）

ridge 现已是一个跨多宿主的终端 / IDE 产品：桌面（Tauri）、移动 PWA / web-remote（浏览器）、headless `rdg`（无头 host）、ridge-cloud（公网中转）。问题是：**所有 GUI 体验都要求用户先安装桌面 app**，触达成本高；而团队已经在做的"统一远控架构"（`docs/plans/unified-remote-architecture-handoff-final.md`）已把内核与传输彻底分层，使得"再多挂一个前端宿主"的边际成本理论上很低。

本调查回答两件事：
1. **必要性**：是否值得把 ridge 内核装进 VSCode 扩展？相对"已有的桌面 app + 浏览器 web-remote"，增量价值在哪？
2. **可行性**：在已确认的形态下（**完整 SPA 嵌入 VSCode webview + 子进程 `rdg` 作后端**）技术上能否做、要补什么、风险在哪、分几步。

**已确认的范围决策**：
- 集成形态 = **完整 SPA 嵌入**（整个 ridge 桌面 UI 跑在 VSCode webview 标签页里，扩展宿主提供 invoke 桥）。
- 必要性动机 = **四项全选**：触达分发 / 复用编辑器生态 / 降低维护成本 / 战略技术验证。
- 后端宿主 = **子进程 `rdg`**（扩展 spawn 现有 headless 二进制，走 JSON-RPC）。

---

## 二、结论摘要（先给判断）

- **可行性：高（HIGH）**，且被现有架构大幅去风险。关键事实：`ridge-core` / `ridge-tmux` 本就零 Tauri 依赖；`rdg`（`packages/ridge-cli`）已证明内核能脱离 Tauri 无头运行；web-remote 的 `tauriShim` + L1/L2 传输层已把前端与 Tauri 解耦；终端渲染是纯 `term-wasm` + Canvas。**"完整 SPA 嵌入 + rdg 子进程"≈ 把 web-remote 指向一个本地 spawn 的 `rdg`**，新代码集中在三处（见 §五），其余复用。
- **真正的成本不在 VSCode 胶水，而在补齐 `rdg` 的命令面**：桌面 SPA 需要 pane/workspace/terminal 生命周期、完整 git、LSP 等，而这些目前仍部分留在 `src-tauri`，无头 host 尚未补全（unified-remote 路线 S1/S3/S5 正在推进）。VSCode 扩展应**搭车**这条路线，而不是独立另起炉灶。
- **必要性：中等偏正面，但有一处需诚实面对的张力**。"完整 SPA 嵌入"得到的是一个 VSCode 标签页里的**自包含孤岛**——它**不直接复用** VSCode 的 Monaco / SCM / 扩展生态（SPA 自带 Monaco）。所以"复用编辑器生态"只在**工作流层面**成立（用户留在一个 VSCode 窗口、市场一键装、与其它扩展共存），而非在编辑器内核层面。真正深度融合需要的是"内核为后端"形态（未选）。**最强的必要性论据其实是"降低维护成本 + 战略验证"**：统一远控路线已让无头 host 成为一等公民，VSCode webview 是一个近乎免费的额外前端，既验证多宿主论题，又打开市场分发。
- **建议**：以 **spike 先行、门控推进**的方式立项，骑在 unified-remote 路线上；不作为独立大押注。先用 1~2 天 PoC 证明 webview 三大约束（CSP / WASM / 二进制分发）可过，再决定是否进入 MVP。

---

## 三、必要性调查（Necessity）

### 3.1 四项动机逐条评估

| 动机 | 成立度 | 说明 |
|---|---|---|
| **触达与分发** | ★★★★☆ | 最强。VSCode 市场 = 数千万存量用户、一键安装、自动更新；免去"下载安装桌面 app"门槛。是其它形态拿不到的独有增量。 |
| **复用编辑器生态** | ★★☆☆☆ | **部分成立且有张力**。完整 SPA 嵌入是孤岛：自带 Monaco、不接 VSCode SCM / LSP / 扩展。"复用"仅指：单窗口工作流、与用户既有 VSCode 主题/键位/扩展**共存**、可选地深链到 VSCode 原生编辑器。深度融合需"内核为后端"形态。 |
| **降低维护成本** | ★★★★☆ | 成立。新增前端复用 `ridge-core`/`ridge-tmux`/`term-wasm`/`tauriShim`/传输层，几乎不新增内核与协议；推动 `rdg` 命令面补全本就是路线既定目标，VSCode 复用其成果。 |
| **战略 / 技术验证** | ★★★★★ | 最契合当前阶段。直接检验"内核可移植 + 多宿主"论题的边界，为未来（JetBrains 插件、纯云 IDE、CI 内嵌终端等）铺路。 |

### 3.2 与替代方案对比（为什么不"什么都不做"）

| 替代项 | 它已能做什么 | 为什么仍不够 |
|---|---|---|
| 现有桌面 app | 完整体验 | 需安装；触达受限 |
| 现有 web-remote（浏览器 PWA） | 同一套 SPA 已能在浏览器跑，并指向桌面 host | 仍需先有一个运行中的桌面/`rdg` host；浏览器标签页脱离用户的 IDE 工作流；无市场分发渠道 |
| 把 web-remote 直接做成可安装 PWA | 安装门槛低 | 仍是独立窗口，不在 VSCode 内；无法借市场存量；无法与用户扩展共存 |

**结论**：VSCode 形态的独有价值 = **市场分发 + 单窗口工作流 + 本地自带 host（spawn rdg，无需用户另开桌面 app）**。这三点是其它任何已有形态都不同时具备的。

### 3.3 必要性裁决

**建议立项，但定位为"骑在 unified-remote 路线上的低成本战略前端"**，而非独立产品大押注。判定理由：增量工程量主要落在本就要做的无头 host 补全上；VSCode 专属胶水小；战略/维护收益清晰；触达是独有增量。需向干系人讲清"完整 SPA 嵌入 ≠ 与 VSCode 编辑器深度融合"这一张力，避免预期错配。

---

## 四、可行性分析（Feasibility）

### 4.1 现有可复用资产

| 资产 | 路径 | 对本方案的意义 |
|---|---|---|
| `ridge-core`（零 Tauri 内核） | `packages/ridge-core/src/{dispatch.rs,capability.rs,ctx.rs,error.rs,sandbox.rs}` | 单一 `dispatch(method,args,ctx)` 入口 + 能力白名单 + 沙箱；运行时无关，rdg 直接用 |
| `ridge-tmux`（零 Tauri 无头会话引擎） | `packages/ridge-tmux/src/{lib.rs,http.rs}` | PTY 会话/窗口/pane 树 + 可选 `http` axum 路由；桌面与 rdg 共用同一 tmux 协议 |
| `rdg` 无头 host（已证明脱 Tauri 运行） | `packages/ridge-cli/src/{core_host.rs,fs_reuse.rs,rtc.rs,e2ee.rs,protocol.rs,pty.rs,session.rs}` | 已能 serve 远控（TLS/WS + WebRTC），`headless_ctx` 带根沙箱。本方案的后端就是它 |
| 共享服务器基建（TLS/HTTP/WS） | `packages/ridge-remote/src/{server.rs,tls.rs,ua.rs}` | rdg 与桌面共用；可绑 `127.0.0.1` |
| 终端渲染内核（纯 web） | `packages/ridge-term/`（`@ridge/term-wasm`） | WASM + Canvas，零平台依赖，任意 webview 可跑 |
| 前端 Tauri 解耦层 | `src/lib/transport/tauriShim/{bridge.ts,core.ts,event.ts,window.ts,dialog.ts,clipboard.ts,opener.ts}` | 构建期把 `@tauri-apps/api/*` 别名到 WS 后端 shim——VSCode 形态直接复用 |
| 分层传输 | `src/lib/transport/remote/{types.ts(L1 ChannelTransport),rpcClient.ts(L2),lanWsAdapter.ts,cloudWebrtcAdapter.ts,cloudMux.ts}` | VSCode 适配只是"第三个/复用第一个适配器" |
| 构建期宿主分叉机制 | `vite.config.js`（`RIDGE_WEB_REMOTE` 别名）/ `vite.remote.config.js` | 复制为 `RIDGE_VSCODE` 目标即可产出 webview bundle |
| 宿主侧协议参照 | `src-tauri/src/remote/server.rs`（`dispatch_invoke_jsonrpc`、`build_remote_pane_list`） | rdg 需对齐的 JSON-RPC + D9 握手 + pane 列表语义 |

**核心洞察**：本形态下 VSCode webview 本质上就是"指向本地 `rdg` 的 web-remote"。客户端栈几乎整套复用，**唯一真正的新前端代码是一个 L1 适配器（甚至可直接复用 `lanWsAdapter` 指向 `127.0.0.1`）+ 一层 vscode shim 兜底**。

### 4.2 目标架构（推荐）

```
┌────────────────────────── VSCode 进程 ──────────────────────────┐
│  扩展宿主 (Node, extension.ts)                                    │
│   • 激活时 spawn 平台专属 rdg 二进制                              │
│       rdg remote --daemon --bind 127.0.0.1:0 --token <ephemeral> │
│       --root <workspaceFolder>   (能力沙箱锁到工作区)            │
│   • 读取 rdg 实际端口 + ephemeral token（stdout 首行）           │
│   • 创建 WebviewPanel，asWebviewUri 提供 web bundle 资源         │
│   • 通过 postMessage 把 {wsUrl, token} 注入 webview              │
│   • 监听 webview→ext 的"打开原生编辑器/复制/通知"等可选深链      │
│                                                                  │
│  ┌──────────── Webview (受限浏览器环境) ────────────┐           │
│  │  ridge 桌面 SPA（RIDGE_VSCODE bundle）           │           │
│  │   • @tauri-apps/api/* → vscodeShim/*（别名）     │           │
│  │   • L2 RpcClient ── L1 lanWsAdapter ─────────────┼── WS ──┐  │
│  │   • term-wasm + Canvas2D 渲染终端                 │        │  │
│  └──────────────────────────────────────────────────┘        │  │
└───────────────────────────────────────────────────────────────┼──┘
                                                                  │
                          (loopback WS, 127.0.0.1, ephemeral token)│
                                                                  ▼
              ┌─────────────── rdg 子进程（无头 host）───────────────┐
              │ packages/ridge-remote 服务器 (/ws, /verify)          │
              │ ridge_core::dispatch(method,args, headless_ctx+roots)│
              │ ridge-tmux 会话引擎（PTY 生命周期 / capture）        │
              │ 【缺口】pane/workspace/terminal CRUD、完整 git、LSP  │
              └──────────────────────────────────────────────────────┘
```

**为什么走 loopback WS 而非 stdio**：rdg 已经内建 `/ws`+`/verify`+二进制 pane 帧；绑 `127.0.0.1:0`（OS 分配端口）+ 每次启动随机 token，复用现成服务器与 `lanWsAdapter`，**新代码最少**。stdio 也可行（用 cloud 那套 `0x10/0x11` 单字节 mux 处理二进制 PTY），但要新写帧封装，收益不抵成本。

### 4.3 三种后端宿主对比（已选 rdg，附理由）

| 维度 | **子进程 rdg（选定）** | NAPI 原生插件 | WASM 内嵌 |
|---|---|---|---|
| 新增 FFI/工具链 | 无 | napi-rs + 各平台 MSVC 预编译 | wasm 目标移植 |
| 复用现有 host 代码 | 极高（rdg 现成） | 中（需把 dispatch 单独导出） | 低 |
| 二进制分发 | 需随 .vsix 带平台二进制 | 同样需平台二进制 | 单 wasm 跨平台 |
| `git` / `std::fs` / `tokio` | 原生可用 | 原生可用 | **重大障碍**（git2 不编 wasm、fs/tokio 受限） |
| PTY | rdg 原生 PTY | 进程内 PTY 复杂 | 不可行 |
| 隔离/稳健性 | 进程隔离，崩溃不带崩 VSCode | 进程内，崩溃带崩宿主 | 进程内 |
| 延迟 | loopback 一跳（可忽略） | 最低 | 中 |
| **裁决** | **最优**：零 FFI、最大复用、隔离好 | 仅当需极致低延迟才考虑 | PTY/git 死路，淘汰 |

### 4.4 关键技术障碍与可解性

1. **VSCode webview CSP 严格** —— webview 默认锁死，脚本需 nonce，资源需 `asWebviewUri`。
   - WASM 需在 CSP 加 `script-src 'wasm-unsafe-eval'`；可设。
   - 外连需 `connect-src` 放行 `ws://127.0.0.1:*`（loopback WS）。可设。
   - SvelteKit/Vite 产物需改为 webview 友好（相对资源、nonce 注入、`<base>` 处理）。属适配工作，非阻断。
2. **WebGPU 在 VSCode（Electron）webview 多半不可用** —— `term-wasm` 默认 `webgpu`，但有 **Canvas2D 回退**（web-remote 现也走 Canvas）。PoC 必须验证 Canvas2D 路径在 webview 内 OK。Web Worker（off-main-thread 渲染）在 webview 可用但需 blob/URI 处理。
3. **平台二进制分发** —— 需为每个目标（win-x64 / mac-arm64 / mac-x64 / linux-x64…）打 `rdg`。VSCode 支持**平台专属扩展**（按 target triple 发不同 .vsix）或激活时按需下载。属成熟模式（语言服务器同款）。
4. **【最大缺口】无头 host 命令面不全** —— 桌面 SPA 需要 pane/workspace/terminal 生命周期、完整 git（含凭据）、LSP，而这些目前部分仍在 `src-tauri`、未完全下沉到 `ridge-core`/`rdg`。这是工作量主体，也正是 unified-remote S1/S3/S5 的目标。**VSCode MVP 应采用与该路线一致的"渐进命令面"**：先 终端 + 文件树 + 搜索 + 只读编辑器，再 git，再 LSP。
5. **会话生命周期与持久化** —— 关闭/重载 webview 时 rdg 子进程的去留、reconnect 后的 pane 快照（D10 attach snapshot，路线已设计）。复用现有 reconnect + 快照机制。
6. **安全** —— 即便 loopback 也必须 token 鉴权（防本机其它进程连 `/ws`）；能力沙箱 `with_roots([workspaceFolder])` 锁到工作区；host-privileged 命令（`get_remote_info`/`enter_deep_root_mode` 等）保持不在远控白名单。这些机制现成。
7. **LSP** —— 桌面 LSP host 在 `src-tauri/src/lsp/mod.rs`（spawn `typescript-language-server`/`rust-analyzer`，stdio JSON-RPC）。要么把它移植到 rdg 侧 spawn，要么 MVP 阶段先不带 LSP（依赖用户 VSCode 自身的语言扩展，作为"复用生态"的一种弱形式）。

---

## 五、目标改动点（执行时；新代码集中在三处）

> 形态决定：客户端栈几乎整套复用，新增工作量按"VSCode 胶水（小）"与"rdg 命令面补全（大、搭车路线）"两类划分。

**A. VSCode 扩展宿主（全新，小）**
- 新建独立扩展工程（建议 `packages/ridge-vscode/`）：`extension.ts`（激活、spawn rdg、读端口+token、建 WebviewPanel、注入 `{wsUrl,token}`、CSP/nonce）、`package.json`（贡献点：命令、视图、平台 target）。
- 平台二进制打包脚本（随 .vsix 带 `rdg`，或激活按需下载 + 校验）。

**B. 前端：新增 VSCode 构建目标 + shim 兜底（小，大量复用）**
- 复刻 `vite.config.js` 的 `RIDGE_WEB_REMOTE` 别名机制为 `RIDGE_VSCODE` 目标，产出 webview 友好 bundle（nonce、相对资源、`connect-src ws://127.0.0.1`）。
- 传输层：**优先直接复用 `src/lib/transport/remote/lanWsAdapter.ts` 指向 `127.0.0.1`**；仅当需要 ext↔webview 的 `postMessage` 桥（如深链 VSCode 原生编辑器/系统通知）时，新增极薄 `src/lib/transport/vscode/` 适配。
- shim 审计（路线 R12）：确保 SPA 触达的所有 `@tauri-apps/api/*`（core/event/window/dialog/clipboard/opener/plugins）在 webview 形态下都有 stub —— 大多复用现有 `tauriShim/*`，仅 dialog/clipboard/notification 可选改走 ext `postMessage`。

**C. rdg 命令面补全（大，与 unified-remote 路线共担）**
- 对齐 `src-tauri/src/remote/server.rs::dispatch_invoke_jsonrpc` 的 JSON-RPC + D9 握手 + `build_remote_pane_list` 语义到 rdg。
- 按 MVP 切分推进：① 终端（ridge-tmux 已有）+ 文件树 + 搜索 + 只读编辑器 → ② 写文件 + 完整 git（凭据注入）→ ③ pane/workspace 生命周期 + LSP（移植 `src-tauri/src/lsp`）。
- 头疼项（路线 §5.4 已列）：git 凭据无 GUI、PTY 的 shell/TERM/env 注入 —— 沿用路线既定方案。

**参照契约（勿改协议、对齐即可）**：`docs/contracts/ridge-cloud-protocol.md`、`docs/plans/unified-remote-architecture-handoff-final.md`（D4/D5/D7/D8/D9/D10、S1/S3/S5）。

---

## 六、风险与缓解

| 风险 | 级别 | 缓解 |
|---|---|---|
| webview CSP/WASM/Canvas 实际跑不起来 | HIGH | **PoC 先行**（§七 阶段 0）做唯一 go/no-go 闸门，1~2 天验证 |
| 无头 host 命令面补全工作量被低估 | HIGH | 不独立扛；搭车 unified-remote S1/S3/S5；MVP 用渐进命令面 |
| 平台二进制分发 / .vsix 体积 | MED | 平台专属扩展或激活按需下载；二进制带签名校验 |
| WebGPU 不可用导致渲染降级 | MED | 强制走 Canvas2D 回退（web-remote 已验证路径），PoC 必测 |
| "完整 SPA 嵌入"被误期望为"VSCode 深度融合" | MED | 必要性章节已显式声明张力；干系人沟通；保留后续演进到"内核为后端"的路径 |
| loopback 端口被本机他进程探测 | LOW | 每启动随机 token + `/verify`；绑 127.0.0.1；能力白名单不含 host-privileged |
| rdg 子进程泄漏（webview 关而进程留） | LOW | 扩展 deactivate / panel.onDidDispose 显式 kill；心跳兜底 |

---

## 七、阶段划分（spike 先行，门控推进）

- **阶段 0 — PoC / Go-No-Go（~1~2 天，唯一闸门）**
  手动 spawn 现有 `rdg remote --daemon --bind 127.0.0.1`，最小 VSCode 扩展开一个 WebviewPanel，加载 web-remote 既有 bundle，`lanWsAdapter` 指向 loopback。**只验三件事**：①CSP 下 WASM + Canvas2D 终端能渲染并交互；②loopback WS + token 鉴权通；③至少一个 `dispatch` 命令（如 `get_file_tree`）端到端跑通。任一不过 → 暂停，重新评估形态。

- **阶段 1 — MVP（终端优先）**
  正式 `RIDGE_VSCODE` 构建目标 + 扩展工程骨架 + 二进制打包；rdg 命令面补到"终端 + 文件树 + 搜索 + 只读编辑器"；reconnect / pane 快照接通。

- **阶段 2 — IDE 写能力**
  写文件 + 完整 git（凭据）+ workspace/pane 生命周期；shim 审计补全。

- **阶段 3 — LSP 与打磨**
  移植 `src-tauri/src/lsp` 到 rdg spawn；可选深链 VSCode 原生编辑器/SCM；市场打包发布流程。

每阶段结束产出可演示构建 + 决定是否进入下一阶段。

---

## 八、验证方式（端到端）

1. **PoC 验证**：在装好 PoC 扩展的 VSCode 里打开 webview → 看终端渲染、敲命令、`get_file_tree` 返回工作区文件树；DevTools（webview 可开）确认无 CSP 报错、WASM 加载成功、WS 已连 `127.0.0.1`。
2. **传输契约**：复用现有 vitest 传输/allowlist 用例（`src/lib/transport/remote/*` 测试 + `remoteAllowlist.ts` 计数 pin），保证 VSCode 适配不破协议。
3. **rdg host 单测**：`cargo test -p ridge-cli -p ridge-core -p ridge-tmux` 覆盖新补命令面。
4. **回归**：确认 `RIDGE_VSCODE` 构建目标不影响桌面 / web-remote 既有产物（三目标分别 build 通过 + svelte-check 0/0）。
5. **安全核验**：无 token 连 `/ws` 应 403；越出 `--root` 的路径请求应被沙箱拒（`OutsideSandbox`）；host-privileged 命令经远控不可达。
6. **生命周期**：关闭/重载 webview 后 rdg 子进程被回收（任务管理器 / `ps` 核对）。

---

## 九、待定决策点（执行前需拍板）

1. **MVP 是否带 LSP**：移植 `src-tauri/src/lsp` 到 rdg（增量大），还是 MVP 先依赖用户 VSCode 自身语言扩展（弱"复用生态"）？建议后者，阶段 3 再补。
2. **二进制分发**：随 .vsix 内嵌（体积大、离线友好）vs 激活按需下载（体积小、需网络 + 校验）？建议平台专属扩展内嵌。
3. **是否立即立项还是只到 PoC**：建议先批 PoC（阶段 0），以其结论作为是否进入 MVP 的硬闸门。
4. **扩展工程位置**：`packages/ridge-vscode/`（同仓）vs 独立仓？建议同仓，最大化与内核/前端共演进。
