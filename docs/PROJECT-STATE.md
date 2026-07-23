# Ridge 项目状态（唯一 NotebookLM 来源）

状态日期：2026-07-23
覆盖仓库：`wind`（`C:\code\wind`）与兄弟仓库 `ridge-cloud`（`C:\code\ridge-cloud`）
用途：人类与 NotebookLM 共用的单一「当前现状 + 愿景 + 差距」来源，辅助规划、取舍与追问。
不含：密钥、生产凭据、用户数据；不把历史计划或未复测功能写成已验证事实。

证据等级：
- **代码事实**：由 2026-07-23 增量同步后的 CodeGraph（537 文件 / 11,319 节点 / 18,166 边）与当前源码确认。
- **Git 事实**：由本地分支、HEAD 与提交历史确认。
- **运行事实**：必须有本轮测试/退出码证据；缺证据时明确写「未验证」。
- **文档声明**：若与代码冲突，以代码为当前行为、以协议为应修正目标。

---

## 1. 产品愿景与北极星（稳定段，少改）

Ridge 要成为**本地优先、随处可接入的人机协作开发控制平面**：人能看见每个开发智能体在做什么，能在关键时刻拦截、接管和恢复；多个智能体在同一工作空间协作；工作上下文不因终端、设备、模型或会话切换而丢失。

差异化在四个结果：
- **可见**：Agent 身份、状态、任务、改动与故障可被理解。
- **可控**：危险动作可审批，单个 Agent 可暂停、恢复、接管和回滚。
- **可续**：工作区记住目标、约束、决策、任务和运行状态。
- **可协作**：多个本地 CLI Agent 经可见 Pane、tmux 与 MCP 共同工作，人始终拥有最终裁决权。

入口定位：桌面 = 主控制室；手机/浏览器 Remote = 随身控制台（roster、切 Pane、HITL 审批、弱网恢复，不复制完整桌面 IDE）；`rdg`/SSH = 终端原生入口（与桌面一致的工作区/Pane/Remote 核心语义）。Remote 与 `rdg` 是「控制平面的随处入口」，不是通用 VNC。

明确非目标：通用远程桌面/VNC/多显示器；万能 AI IDE 或 VS Code 功能表对标；托管用户模型密钥或绑定单一 Agent CLI；聊天窗口/动画/插件市场作为核心竞争力；Agent 自治凌驾于人类审批、数据安全和可恢复性；为假想端一次性大重构；手写重复协议与不断增长的 handoff 墓地。

决策过滤器（提案先答）：是否明显增强可见/可控/可续/可协作之一？用户能否在真实工作流感知价值？能否先复用现有能力？是否引入新协议副本、状态源或不可恢复写路径？有无低成本证伪实验？删除或简化是否更接近目标？前两问为否则不进路线图。

## 2. 锁定决策与安全不变量（稳定段）

- 协议 SSOT 唯一：`C:\code\ridge-cloud\docs\ridge-cloud-protocol.md` 为权威全文；`wind` 侧只保留 canonical 入口 + 自动守卫（iteration 1 后已收敛，双 SSOT 债务关闭）。协议变更先改权威契约，再改服务端与所有客户端。
- relay 不读取终端业务明文；业务帧只在端侧 E2EE 加解密。
- host 与 controller 必须同账户；role 匹配 JWT scope 与设备；房间/配额/付费权限用已验证身份 + 数据库实时状态（不信任长寿命 JWT plan claim）。
- TOTP/受信设备验证必须在业务帧门控前完成；**business-ready = transport connected + authorized**（iteration 3 修复后锁定：E2EE connected 但 TOTP 未授权时不得发送 hello/pane recovery）。
- 远控 resize、输入、Pane 订阅与 scrollback 语义跨 desktop/LAN/cloud/CLI 一致；能力必须先协商宣告，未宣告入口显式拒绝而非静默分叉（iteration 2 起由跨入口合同测试守卫）。
- migration 只追加；日志不得输出 token、TOTP seed、私钥、`RIDGE_ARTIFACT_TOKEN`。
- 发布有两条独立版本线：`ridge-cloud` 代码 SHA 与 Remote artifact version，必须分别验证，不混为一个版本。
- 授权阶梯 Level 2（Draft）：改动在独立分支形成可审查提交，人工验证后合并，不自动合并或发布。

## 3. 当前架构（CodeGraph 勾勒）

### 3.1 wind 桌面与终端主链路

```
Svelte 页面/组件 → Tauri invoke/事件 → src-tauri commands / ridge-core dispatch
  → 工作区状态 + PTY engine → pane 输出/GridDelta
  → @ridge/remote TerminalManager → ridge-term WASM Kernel + Renderer → Canvas
```

- `src/routes/+page.svelte` 组装工作区、侧栏、远控与 Agent Center；`SplitContainer.svelte` / `RidgePane.svelte` 管 Pane 布局与交互。
- `packages/remote/src/shared/terminal/manager.ts::TerminalManager` 统一终端实例生命周期，桌面与 Remote 复用。
- `packages/ridge-term` 为终端语义 SSOT（parser/grid/scrollback/selection/search/增量渲染/WASM 绑定）；Canvas2D 是生产主路径，WebGPU 属实验、需真机证据。
- `packages/ridge-core` 承接 workspace/pane/Git 命令与异步 dispatch；Tauri 保留宿主状态、平台资源与事件桥。
- `packages/ridge-cli/src/main.rs`：`tui` / `login` / `remote`（公网 host daemon）/ `connect`（LAN controller）/ `tmux`。
- Teammate/MCP：tmux shim + Ridge MCP server → teammate server / ridge-tmux → 工作区变更 → `AgentCenterPanel.svelte`（拓扑、分组、HITL 审批、熔断）。桌面有 `resolve_hitl_request`；该能力刻意不在 Remote allowlist。

### 3.2 远控三入口

| 入口 | 控制面 | 数据面 |
| --- | --- | --- |
| 本地桌面 | Tauri 进程内命令/事件 | 本机 PTY |
| LAN Web | host 内置 HTTPS/WSS（TOTP/session 鉴权） | 局域网 WS，共享 Remote 协议 |
| 公网 Web | ridge-cloud 认证/信令 | WebRTC DataChannel + E2EE |

公网链路（代码确认）：

```
host: RidgeCloudHost(device JWT) ↔ ridge-cloud /ws（认证、授权、房间、SDP/ICE 转发）
controller: ControllerCloudProvider(user JWT) ↔ WebRTC DC ↔ E2EE ↔ CloudHostBridge ↔ 本机 invoke/Pane 输出
```

关键符号与行为（均有确定性测试守卫，见 §5）：
- `packages/remote/src/shared/cloud/controllerCloudProvider.ts::ControllerCloudProvider`（:114）：退避重连；RTC `disconnected` 15 秒 watchdog → ICE restart；restart 后 12 秒 deadline 未恢复 → 升级整体重建（旧 PC/DC/WS 关闭）；重建后重新 E2EE + TOTP，hello/pane recovery **恰好一次**，timer 清零；`disconnected` <15 秒自愈不触发任何重建/重复恢复。`reconnect`（:684）。
- `packages/remote/src/shared/cloud/cloudHostBridge.ts::CloudHostBridge`（:202）：验证完成前门控 invoke 与 Pane 订阅；TOTP、信道绑定 TOTP、trusted-controller、E2EE 临时公钥绑定钩子；Pane 背压 drain 后每受影响 Pane 恰好重同步一次、不串 Pane。注意：若某些 verifier 未注入，桥为兼容旧路径可能默认放行——「代码支持安全钩子」不自动证明每个生产入口已启用（→ 差距 S1）。
- 1 房间 = 1 host + N controller，controller 有随机 `cid` 定向寻址；同 `cli` 新连接顶替旧连接。
- Pane 历史：首屏小预算 + 滚顶懒加载；DataChannel 分片/重组 + 发送缓冲背压上限。

### 3.3 ridge-cloud

单 Rust/Axum 服务：API + WebSocket relay + 多 SPA 托管（主域账户/设备、`admin.{base}` 管理端、`{device}-{username}.{base}` 租户 controller 按 UA 分桌面/移动产物）。
- `src/router.rs::build_router` / `spa_fallback`；`src/middleware.rs::tenant_resolver`。
- JWT `user`/`device` scope，新签发 EdDSA(Ed25519)、迁移期兼容 HS256；父域 `ridge_sso` HttpOnly cookie + 短时 access token；DB 只存 refresh hash。
- `src/ws/handler.rs::ws_upgrade` 升级前后校验租户/token/scope/设备归属/parked/订阅/连接上限；房间 key 用已验签 `user_id`+设备名。
- `GET /api/v1/ice-servers` 恒返 STUN；配置 `TURN_HOST`+`TURN_STATIC_AUTH_SECRET` 才追加 coturn 时效凭据。
- Remote artifact 独立发布线：`wind` 构建 desktop/mobile 两套产物 → `RIDGE_ARTIFACT_TOKEN` 上传 `/api/v1/remote-artifacts` → 持久卷 `releases/<version>` → current 指针激活，保留最近 3 个回滚。
- PostgreSQL + SQLx，14 个顺序迁移；统一 `{ok,data}`/`{ok:false,error}` 信封；CORS/体限/限流/安全头/脱敏外层防线。

## 4. 仓库快照

| 项 | wind |
| --- | --- |
| 分支 / HEAD | `codex/remote-git-diff-iteration-1` / `6be9173`，较 `origin/main` 领先 13 提交（Level 2 draft，待人工审查合并） |
| 应用版本 | 0.0.17 |
| CodeGraph | 537 文件 / 11,319 节点 / 18,166 边（2026-07-23 sync） |
| 工具链 | pnpm + vitest + svelte-check 本机可运行（本轮多次 exit 0 证据）；`pnpm check` 根脚本非增量、性能不可接受，增量 svelte-check 为可用替代（iteration 2 checker 结论）；cargo 全仓测试本轮未跑 |

`ridge-cloud`：本轮未改动、未重新探测；生产 Dokku SHA、TURN 可达性、artifact current 指针均**未验证**（→ 差距 T3）。

## 5. 迭代闭环成果（iteration 1–4）与确定性证据

- **iteration 1**：校准陈旧基线；修复 Cloud TS capability mirror（缺 `get_workspace_snapshot`、mutating mirror 少 11 项）；固定计数测试改为 Rust canonical 逐项 parity。事后经用户授权同步 `ridge-cloud`，wind 陈旧协议全文收敛为 canonical 入口 + 自动守卫（**T2 关闭**）。
- **iteration 2**：建立 Controller-facing 最小 capability→RPC 合同与跨入口测试；补齐 rdg `get_file_tree/read_file/text_search` 路由；Remote Files/Git/Search、workspace 管理与 theme UI 按能力协商隐藏/收敛（**A2 主体落地**）。
- **iteration 3**：建立 provider→adapter→RpcClient 与 Host 背压的确定性 fault-injection 门禁（100 周期无 pending RPC/重复恢复/timer 泄漏）；修复 business-ready 门控缺陷（E2EE connected 但未授权时提前 hello/pane recovery，Host 丢弃不补发）。
- **iteration 4**（2026-07-23 收口）：
  - 新增两条 watchdog 升级时序门禁：`disconnected <15s` 自愈零副作用；`watchdog 15s → ICE restart → deadline 12s → rebuild` 后恰好恢复一次。
  - 新建聚焦真机 runbook `docs/plans/cloud-remote-physical-smoke-runbook.md`、evidence JSON Schema + 示例 + 校验脚本 `scripts/validate-remote-smoke-evidence.mjs`；证据目录 `/artifacts/remote-smoke/` 已 gitignore。
  - 自动验收全绿（2026-07-23 运行）：faultInjection 7/7；Cloud 定向回归 5 文件 156/156；增量 svelte-check 70 files / 0 errors / exit 0；evidence 校验脚本对示例 exit 0。
  - **真机双平台证据仍为空**：iOS Safari 与 Android Chrome 的换网/后台/token 跨窗场景须由人持真机按 runbook 执行并产出 evidence JSON——这是当前唯一用户必办件。停机条件未触发；不以自动测试宣称双平台通过。

## 6. 差距组合现状（愿景 − 现状，含最新裁决）

| ID | 差距 | 优先级 | 当前状态 |
| --- | --- | --- | --- |
| T1 | 开发门禁可运行性 | P0 | **大部关闭**：pnpm/vitest/增量 svelte-check 有本轮 exit 0 证据；cargo 全仓与 ridge-cloud 测试仍无本轮绿灯 |
| T2 | Cloud 协议双 SSOT | P0 | **关闭**（iteration 1 后收敛 + 自动守卫） |
| T3 | 生产两条版本线状态证据 | P0 | 未做；需生产只读探测（cloud SHA、health、activated artifact version、持久卷） |
| S1 | 兼容安全回落可观测退役 | P0/P1 | 未做；先构造点矩阵与遥测设计，不直接翻 fail-closed 总开关 |
| P1 | Remote Agent 控制台 MVP | P1 | 未做；依赖只读 teammate topology 经 Remote 白名单安全暴露 |
| P2 | Remote HITL/接管闭环 | P1 | 未做；需 nonce/单次消费/过期/审计/多 controller 裁决语义，不简单加白名单 |
| G1 | 单 Agent 暂停/恢复/接管/回滚 | P1 | 未做 |
| A1 | 共享内核减法审计 | P1 | 部分：workspace/pane/Git 已大量入 core；剩余双路径需按调用图逐项 |
| A2 | 跨入口能力矩阵 + conformance | P1 | **主体关闭**：合同测试在；机器可读矩阵文档化仍缺 |
| R1 | 弱网与恢复证据化 | P1 | 实验室部分**关闭**（确定性 fault 门禁）；真机部分待用户执行 runbook |
| M1 | Workspace Memory | P2 | 未做；先最小 6 字段 discovery |
| M2 | Agent 归因事件 | P2 | 未做；依赖 stable_id 可靠 |
| H1 | 远端 host live PTY | P2 | 未做；`hosts.ts::connectHost` 仍仅登记 |
| C1 | rdg 行为一致性 | P2 | 未做；先用 A2 矩阵暴露缺口 |
| E1 | WebGPU 收益验证 | P3/实验 | 未做；无显著收益则停做或删除 |
| E2 | 高级自动编排 | P3/实验 | 未做；需真实多 Agent 瓶颈证据 |

近期组合原则（沿用 GAP_PORTFOLIO §7）：可信基线包（T1 余项 + T3 + S1 审计设计）→ 一条产品主线（P1 控制台 MVP）→ 一个工程护栏（A2 矩阵文档化）→ 测量而非开发（R1 真机）。四项完成前不启动 Workspace RAG、WebGPU shared surface、复杂自动编排或新 Remote 桌面功能。

## 7. 开放问题（请 NotebookLM 定夺）

1. iteration 5 主线二选一：**S1**（安全回落构造点矩阵 + 遥测/退役设计，先审计不改行为）还是 **P1**（Remote Agent 控制台 MVP，只读 roster + 切 Pane）？请按价值/风险降低/解锁力/成本排序并给不做方案。
2. 真机 smoke（R1）由用户择机执行期间，自动化侧可并行哪些与其无冲突的目标？
3. T3 生产探测需生产环境访问权：应做成什么形态的只读 runbook/脚本才能一次命令产出两条版本线证据？
4. cargo 全仓与 ridge-cloud 测试绿灯（T1 余项）应该在哪个迭代补齐，是否值得单独一轮？

## 8. NotebookLM 评审要求（沿用）

对任何下一迭代建议必须输出：对应差距 ID；当前代码证据或需补查符号；价值/风险降低/解锁力/成本/可逆性；至少一个减法方案；可判定验收信号与停止条件；是否引入新状态源、协议副本、不可恢复写路径或生产运维负担。不能映射到 §6 差距的建议默认不进近期计划。不把历史日期、旧完成度、旧测试数当作当前运行证据。

## 9. 刷新规则

发生以下任一事件时覆盖式更新本文件，并在 NotebookLM 中**替换**旧来源（不叠加版本）：跨仓协议/身份/安全边界/Remote 数据流改变；ridge-core/ridge-remote/ridge-term 所有权边界改变；发布架构改变；某「部分/未验证」能力获得或失去确定性证据；P0/P1 差距关闭、新增或优先级变化。普通 bug 修复与局部 UI 调整不写入。
