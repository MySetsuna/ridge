# Ridge 项目状态（唯一 NotebookLM 来源）

状态日期：2026-07-27（iteration 61：Remote 显式入口、配额停放、移动端弹层、全局 Agent 中心与无头会话回收）
覆盖仓库：`wind`（`C:\code\wind`）与兄弟仓库 `ridge-cloud`（`C:\code\ridge-cloud`）
用途：人类与 NotebookLM 共用的单一「当前现状 + 愿景 + 差距」来源，辅助规划、取舍与追问。
不含：密钥、生产凭据、用户数据；不把历史计划或未复测功能写成已验证事实。

证据等级：
- **代码事实**：由 2026-07-27 CodeGraph（629 文件 / 12,855 节点 / 19,620 边）与当前源码确认。
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
- `packages/ridge-term` 为终端语义 SSOT（parser/grid/scrollback/selection/search/增量渲染/WASM 绑定）；渲染为 **WebGPU-first + Canvas2D 自动回退**（`default=["webgpu"]` 生产默认特性，运行时 GPU 探测驱动，2026-05-05 用户反馈钦定「不设 build flag/opt-in」）——非实验代码，**不得删除**；真机收益测量属用户轨（E1）。
- `packages/ridge-core` 承接 workspace/pane/Git 命令与异步 dispatch；Tauri 保留宿主状态、平台资源与事件桥。
- `packages/ridge-cli/src/main.rs`：`tui` / `login` / `remote`（公网 host daemon）/ `connect`（LAN controller）/ `tmux`。
- Teammate/MCP：tmux shim + Ridge MCP server → teammate server / ridge-tmux → 工作区变更 → `AgentCenterPanel.svelte`。iteration 61 后 Agent Center 跨全部工作区聚合 roster，并显示 Claude/Codex JSONL 最近助手回复与 Agent 所创 native 无头会话；OSC 标题优先、前台进程兜底自动登记/释放 pane Agent 状态。桌面有 `resolve_hitl_request`；该能力刻意不在 Remote allowlist。

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
- `src/ws/handler.rs::ws_upgrade` 升级前后校验租户/token/scope/设备归属/parked/订阅/连接上限；房间 key 用已验签 `user_id`+设备名。host 与 controller 均按数据库实时用户组计算设备配额；配额停放以 `parked_by_quota` 区分人工禁用，故恢复配额不会误启人工关闭设备。
- `GET /api/v1/ice-servers` 恒返 STUN；配置 `TURN_HOST`+`TURN_STATIC_AUTH_SECRET` 才追加 coturn 时效凭据。
- Remote artifact 独立发布线：`wind` 构建 desktop/mobile 两套产物 → `RIDGE_ARTIFACT_TOKEN` 上传 `/api/v1/remote-artifacts` → 持久卷 `releases/<version>` → current 指针激活，保留最近 3 个回滚。
- PostgreSQL + SQLx，15 个顺序迁移；统一 `{ok,data}`/`{ok:false,error}` 信封；CORS/体限/限流/安全头/脱敏外层防线。

## 4. 仓库快照

| 项 | wind |
| --- | --- |
| 分支 / HEAD | `main` / `f110dd0`（工作树仅保留用户原有 `Cargo.lock` 改动，未纳入本轮提交） |
| 应用版本 | 0.0.20 |
| CodeGraph | 629 文件 / 12,855 节点 / 19,620 边（2026-07-27 healthy） |
| 工具链 | 2026-07-27：Vitest 99 文件，1,237 绿 / 1 skipped；svelte-check 0 errors / 2 既有 warnings；`cargo check -p ridge --lib`、tmux bin、ridge-tmux 11 测、rdg dashboard 2 测、JSONL 解析 3 测均 exit 0 |

`ridge-cloud`：`codex/remote-artifacts-status` / `beb87ea`；iteration 61 新增 migration 0015 与配额停放原因，`db::device_quota::tests` 6/6 绿。生产 Dokku SHA、TURN 可达性、artifact current 指针仍**未实测**。

## 5. 迭代闭环成果（iteration 1–4）与确定性证据

- **iteration 1**：校准陈旧基线；修复 Cloud TS capability mirror（缺 `get_workspace_snapshot`、mutating mirror 少 11 项）；固定计数测试改为 Rust canonical 逐项 parity。事后经用户授权同步 `ridge-cloud`，wind 陈旧协议全文收敛为 canonical 入口 + 自动守卫（**T2 关闭**）。
- **iteration 2**：建立 Controller-facing 最小 capability→RPC 合同与跨入口测试；补齐 rdg `get_file_tree/read_file/text_search` 路由；Remote Files/Git/Search、workspace 管理与 theme UI 按能力协商隐藏/收敛（**A2 主体落地**）。
- **iteration 3**：建立 provider→adapter→RpcClient 与 Host 背压的确定性 fault-injection 门禁（100 周期无 pending RPC/重复恢复/timer 泄漏）；修复 business-ready 门控缺陷（E2EE connected 但未授权时提前 hello/pane recovery，Host 丢弃不补发）。
- **iteration 4**（2026-07-23 收口）：
  - 新增两条 watchdog 升级时序门禁：`disconnected <15s` 自愈零副作用；`watchdog 15s → ICE restart → deadline 12s → rebuild` 后恰好恢复一次。
  - 新建聚焦真机 runbook `docs/plans/cloud-remote-physical-smoke-runbook.md`、evidence JSON Schema + 示例 + 校验脚本 `scripts/validate-remote-smoke-evidence.mjs`；证据目录 `/artifacts/remote-smoke/` 已 gitignore。
  - 自动验收全绿（2026-07-23 运行）：faultInjection 7/7；Cloud 定向回归 5 文件 156/156；增量 svelte-check 70 files / 0 errors / exit 0；evidence 校验脚本对示例 exit 0。
  - **真机双平台证据仍为空**：iOS Safari 与 Android Chrome 的换网/后台/token 跨窗场景须由人持真机按 runbook 执行并产出 evidence JSON。停机条件未触发；不以自动测试宣称双平台通过。
- **iteration 5**（2026-07-23，可信基线固化）：
  - S1 审计落地：构造点×校验器矩阵 `docs/security/cloud-fallback-matrix.md`（回落面 F1–F6 + 退役条件）；3 条钉死测试（含审计发现：**无 bindTranscript 时 trust-proof 非「直接失败」而是退化为无信道绑定签名 + 信任库裁决**，源注释已更正）；遥测/退役设计文档（零行为变更）。
  - T3 代码侧闭合：ridge-cloud `activate()` 写 `current.json` + 新增 token 守卫只读 `GET /api/v1/remote-artifacts/status`（+3 测试，124 全绿，分支待合并）；wind `scripts/check-prod-status.mjs` 一键两线汇总（桩验四径）。生产实跑待用户。
  - T1 绿灯：wind `cargo test --workspace --exclude ridge` 882 绿 + `-p ridge --bins` 27 绿 + ridge-cloud 124 绿；唯 `-p ridge --lib` 宿主载败（loader 级，先于本轮，Q4）。
  - A2 闭合：`docs/capability-matrix.json`（7 能力 × 6 入口，rdgHost 列由 `CLI_CAPABILITIES` 推导）+ 6 条一致性测试防矩阵成第二事实源（13/13 绿）。
  - A1 示范减法：删 state.rs 死 pane-output 通道面（净 −45 行，rustc dead_code + 全仓 grep 双证）；审计报告确认 git 面已薄委托、`commands/workspace.rs` 只读三件套是真双路径（下切片候选）。
  - 计划外：修复 signaling drift 门禁 Windows 误报（autocrlf 把 vendored 副本涂 CRLF；`.gitattributes` 钉 LF），vitest cloud+transport 全伞 382 绿 / 1 skipped。
- **iteration 6**（2026-07-23，P1 控制台 MVP）：
  - 新协商能力 **`teammate`**（唯一方法 `get_teammate_topology`，只读、轮询）六处同步宣告（Rust/TS allowlist、合同、client/LAN/cloud host 能力表、矩阵）；rdg 无头 host 刻意不宣告（denied）。共享 controller 新增 Team roster 面板（状态点/Leader 冠标/点按切 pane），桌面与移动同码。投影脱敏由 Rust 测试钉死（仅 id/name/paneId/paneIndex/role/status/capability）；HITL 裁决保持不可远达。
  - S1 遥测第一阶段落地：bridge F1（trust-proof transcript 在/缺）与双 provider F2（enforced/relay-trust）进程内计数 + 测试钉死，无新持久面。
  - A1 切片：workspace 列表投影同源化（删平行 `WorkspaceInfo`，net −12 行）；`get_active_workspace_id`/`get_workspace_snapshot` 审计确认本已单源。
  - 证据：vitest shared 全伞 558 绿 / 1 skipped；svelte-check 71 files 0 errors；cargo check + `--lib --no-run` 0 errors；bins 27 绿。
- **iteration 7**（2026-07-23，证据与固化轮，冻结新功能）：
  - **T1 完全关闭——loader 载败根修**：根因为依赖树引入 `comctl32!TaskDialogIndirect`（仅 common-controls v6 导出）；cargo lib 单测宿主无 manifest，加载器绑 WinSxS 5.82 → `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)`。修法：`build.rs` 注入 `/DELAYLOAD:comctl32.dll`（绑定推迟到首次真实调用，测试从不弹框故永不绑定；`rustc-link-arg-tests` 不覆盖 lib 单测宿主、`/MANIFEST:EMBED` 与 tauri-build RT_MANIFEST 冲突，均不可用）。结果：`cargo test -p ridge --lib` **92/92 本机首次全绿**（teammate 投影脱敏等安全断言首次真实执行）；`cargo test --workspace` **首次整仓 exit 0**；附 boot smoke 集成测试防回归。
  - R1 实验室轨关闭：抽共享 `__faultRig.ts`；`weakNetLab.test.ts` 九场景参数化扫描（脉冲 [1s,5s,14s] 自愈零副作用、28s 越 watchdog+deadline 升级链恰一次恢复、fail/recover [10,50] 周期零泄漏、背压 [1,8+ε,12]MiB×3 pane 丢帧后每 pane 恰一次 resync）；`scripts/run-weaknet-lab.mjs` 触发 + metrics.json 结构校验 exit 0；产物含「实验室确定性模型，非真机结论」disclaimer。
  - A1 审计：`Teammate` 六字段全被消费（role/status/capability 各有 grep 实证），**无死字段**，NotebookLM 删字段建议驳回。
  - 固化：`docs/plans/user-verification-checklist.md` 四件用户必办单页；README_CN 补能力协商 + Team 面板段；WORKFLOW 补双轨制段。
  - 证据：vitest shared 全伞 567 绿 / 1 skipped（37 文件）；svelte-check 0 errors；weaknet-lab 脚本 exit 0。
- **iteration 8**（2026-07-23，P2 阶段 1 + 支线）：
  - **P2 阶段 1 关闭（只读可见）**：`hitl.rs` PENDING 注册表加宽存脱敏元数据（原先发事件即弃）；新只读方法 `list_hitl_pending`（`teammate` 能力下，六处宣告同步）投影仅 `{id, initiator, level, reason, createdAt}`——**绝不含 `action` 命令全文**（Rust 测试钉死）；Team 面板只读 Pending approvals 区；裁决通道（`resolve_hitl_request`）保持不可远达。裁决/nonce/单次消费语义属阶段 2，未做。
  - S1 遥测第二阶段：F3 计数（controller `tofuChanged`，合法 0x02+指纹变化测试）、F4 计数（host `fallback0x01`，含签名失败降级）；**F5 退役删除**（`keyBindingVerifier` 钩子生产零接线双证后整链删除，矩阵行改已退役）；计划外根修 deviceTrust localStorage 探针 + 模块级内存回退（Node 残缺对象地雷）。
  - A1 切片：pane.rs 全量读写分类审计（报告在迭代文档）；rustc dead_code 扫出并删真死码二处（pane_tree `first_leaf/last_leaf`、parser `full_reframe_with_scrollback`，net −60 行）。
  - G1 设计文档（零代码）：`docs/superpowers/specs/2026-07-23-agent-suspend-resume-design.md`——三层边界、Windows 无 SIGSTOP 三候选（Job Object 冻结目标态）、顺序不变量、HITL fail-closed 计时不暂停。
  - 证据：`cargo test --workspace` exit 0；vitest shared 全伞 559 绿 / 1 skipped（删 9 增 1 帐目符）；svelte-check 0 errors；`-p ridge --lib` 93 绿。
- **iteration 9**（2026-07-23，G1 阶段一 + 审查闭环，零协议面变更）：
  - **G1 阶段一关闭（软暂停/恢复）**：`teammate/suspend.rs` 进程级注册表；agent 写路径四所归一（send-keys/delegate/MCP/exec → `agent_pty_write` 唯一收口，suspended 明确拒）；人类输入与断路器 Ctrl-C 刻意不门控（接管/刹车语义）；拓扑双路径 status 覆写 `Suspended`（无新字段）；Agent Center Pause/Play；`suspend_agent`/`resume_agent` 仅桌面 IPC + 不可远达负断言。OS 级冻结属阶段二未做。
  - C1 清单化：`scripts/rdg-gap-report.mjs` 派生缺口报告（3 supported / 5 denied，teammate 刻意排除，余待人工判定补路由或声明永久缺口）。
  - 审查辅助：`scripts/generate-review-pack.mjs` → `docs/review/branch-review-guide.md`（全提交分组 + 协议面/安全面标注 + 计数自校验）。
  - A1 审计新发现：**LAN host `close_workspace` 第三副本实分歧**——漏发 `WorkspacesChanged`/`WorkspaceListChanged`（LAN 关区不通知他端）+ 多删 `workspace_names`；同源化候选以 close 为首、升级缺陷修复级（`docs/audits/workspace-write-paths.md`）。
  - E1/E2 簿记校正见 §6。
  - 证据：`cargo test --workspace` exit 0；`-p ridge --lib` 96 绿（+3）；vitest 全伞 559 绿 / 1 skipped；svelte-check 0 errors；双脚本 exit 0。
- **iteration 10**（2026-07-23，A1 主线 + 簿记清偿，零协议面变更）：
  - **A1 写路径同源化 + 缺陷修复**：`close_workspace_core`/`rename_workspace_core` 唯一实现，三/双调用方委托；**修实缺陷**——LAN 副本漏发 `WorkspacesChanged`/`WorkspaceListChanged`（关区不通知他端）、names 残条泄漏三方对齐；broadcast 订阅测试钉死；净删 ~80 行副本（提交本身 −4 净行）。
  - M1 设计定稿（零代码）：sidecar json 落点（与 .ridge 布局解耦；清理挂 `close_workspace_core` 单点）、6 字段与首个读写方、隐私边界（决策只存风险分类+摘要）、三切片序（切片一 = suspended panes 持久化 + 启动恢复）。
  - C1 判定收口：五 denied 缺口逐项判定入脚本 `JUDGMENTS`（teammate 刻意排除；theme/invoke 语义完备永久缺口；git/workspace 补路由候选待需求），报告零「待人工判定」残留。
  - 节律固化：WORKFLOW 每轮闭环必刷审查导读；积压期不扩协议面。
  - 证据：cargo workspace exit 0；workspace::tests 2 绿；vitest 559/1skip；svelte-check 0 errors；双脚本 exit 0。
- **iteration 11**（2026-07-23，M1 切片一 + P2 阶段 2 设计，零协议面变更）：
  - **M1 切片一关闭（暂停态跨重启）**：sidecar `{app_data}/workspace-memory/{wid}.json`（仅 suspendedPanes+updatedAt，原子写、空集删文件）；启动载入重挂；全写方钩落盘；`close_workspace_core` 单点清理；IO 全程 fail-open（损坏 json 跳过不 panic）。dir 注入单测 3/3（重启恢复/不复活/损坏容忍/关区同清）。
  - **P2 阶段 2 设计定稿**：一次性裁决票据（nonce 随挂起项生成，恒时比对、取出即毁=单次消费防双裁决）；**远端 modify 永不开放**；120s fail-closed 不变不延；多 controller 首达生效+败者入审计（接 M1 decisions，不存命令全文）；传输面选 `teammate` 新方法（弃 CONTROL 混层）。**实现被红线冻结待用户轨**。
  - 证据：cargo workspace exit 0；suspend 3/3；vitest 559/1skip；svelte-check 0 errors。
- **iteration 12**（2026-07-23，收敛轮）：30 分钟核验会动线文档（用户轨一次清偿动线，覆盖 checklist 全部条目）；G1 阶段二/M1 余切片/M2 簿记归档（待证据/解冻重开）；维护态定型入 WORKFLOW（验收=门禁绿+导读刷新+零回归；解冻=用户轨首份证据）。
- **iteration 13**（2026-07-23，用户指令解冻，P2 阶段 2 实现）：一次性裁决票据落地——`PendingEntry`+=nonce（uuid v4 不可猜）、投影六字段（+`resolutionNonce`，仍无 action）、`resolve_remote` 同锁恒时比对+取出即毁（单次消费原子）、verdict 仅 approve/reject（**modify 永不开放**）、四态结局；`resolve_hitl_remote` 六处宣告 + `MUTATING_METHODS` 双侧归类；桌面版裁决/网关开关/暂停命令不可远达负断言维持；Team 面板 Approve/Reject 双按钮 + 结局反馈。**P2 全闭**。
- **iteration 14**（2026-07-23，M1 切片二 + M2，存量终轮）：`memory.rs` 单源 doc 级 RMW（互斥+原子写+updatedAt 元数据+空删；suspend 持久化改经之，DIR OnceLock 单点注入）；三消费点裁决审计落盘（桌面/远端含 nonce-mismatch 败者尝试/超时 fail-closed；条目 `{ts,source,initiator,verdict,riskLevel,reasonSummary,outcome}` 无命令全文、环形 50）；**M2 归因**：initiator 升级为稳定 agent_id（pane 反查回落）；读方 `list_hitl_decisions`（仅桌面 IPC）+ Agent Center 审批历史区。
- 证据（13+14）：`cargo test --workspace` exit 0×2；teammate:: 25 绿；vitest 559/1skip×2；svelte-check 0 errors×2。
- **iteration 15**（2026-07-24，开放愿景清单 11 项）：见 `CONTRACT-iteration-15.md` + `2026-07-24-open-vision-checklist.md`。V-H1 TCP；V-G1-OS/RB；V-M1-S3；V-B6A/B/B3；V-DISC；V-MOB-CP；V-TUI-CLK 核查；V-PASTE。证据：`cargo test -p ridge --lib` 114；ridge-term/remote 相关绿；vitest 5。

## 6. 差距组合现状（愿景 − 现状，含最新裁决）

| ID | 差距 | 优先级 | 当前状态 |
| --- | --- | --- | --- |
| T1 | 开发门禁可运行性 | P0 | **关闭**（iteration 7：loader 根修后 `cargo test --workspace` 首次整仓 exit 0，全部门禁本机可运行） |
| T2 | Cloud 协议双 SSOT | P0 | **关闭**（iteration 1 收敛 + 自动守卫；EOL 误报已根治） |
| T3 | 生产两条版本线状态证据 | P0 | **代码侧关闭**（status 端点 + 一键脚本）；生产实跑与分支合并部署待用户 |
| S1 | 兼容安全回落可观测退役 | P0/P1 | **审计 + 遥测两阶段关闭**（F1–F4 计数已实施；F5 已退役删除；F6 由 S1 门禁测试守构造纪律）；逐面 fail-closed 翻闸待真实数据窗口（用户轨） |
| P1 | Remote / Desktop Agent 控制台 | P1 | **代码侧关闭**（iteration 6 Remote roster；iteration 61 桌面跨工作区 roster、最近回复、自动 pane 状态、无头会话唤醒）；真机 UI 人工核验待用户 |
| P2 | Remote HITL/接管闭环 | P1 | **全闭**（iteration 13：远端裁决通道实现——票据单次消费、modify 永不开放、审计接 M1）；真机核验待用户 |
| G1 | 单 Agent 暂停/恢复/接管/回滚 | P1 | **暂停/恢复/软接管 + OS 冻结 + 回滚关闭**（iteration 15：soft gate + Unix SIGSTOP/Win NtSuspend fail-open；git worktree 补丁 checkpoint/rollback） |
| A1 | 共享内核减法审计 | P1 | **关闭**（iteration 10：close/rename 同源化落地 + LAN 漏广播缺陷修复；历史切片累计五处、净删 200+ 行；后续减法随日常纪律进行，不再占差距行） |
| A2 | 跨入口能力矩阵 + conformance | P1 | **关闭**：机器可读矩阵 + 一致性测试互证（新增能力必须声明矩阵） |
| R1 | 弱网与恢复证据化 | P1 | **实验室轨关闭**（fault 门禁 + iteration 7 九场景参数化扫描 harness）；真机轨待用户执行 runbook |
| M1 | Workspace Memory | P2 | **切片一+二+三关闭**（iteration 11/14/15：暂停态 + 裁决审计 + goal/constraints/tasks API/UI） |
| M2 | Agent 归因事件 | P2 | **关闭**（iteration 14：审批/裁决审计归因至稳定 agent_id） |
| H1 | 远端 host live PTY | P2 | **代码侧 LAN 出站闭合**（22–24/29–30：Transport+Mock+LAN 相位机、subscribe/write/resize/detach、**pump_host_output 生产命令**、Hosts 轮询泵、live cap；真机 OS WebSocket 读循环仍可接 `LanOutboundTransport::inject_socket_ready`）；真机双端联调用户轨 |
| C1 | rdg 行为一致性 | P2 | **关闭**（iteration 61：LAN/public Remote 显式手动启停，LAN 根 URL 与桌面路由语义一致） |
| E1 | WebGPU 收益测量 | P3/用户轨 | **重定义**（iteration 9 簿记校正）：WebGPU 为生产默认路径非实验（历史措辞误导致 NotebookLM 提删除建议，已驳回留档）；剩余工作 = 真机 GPU 收益测量，属用户轨 |
| E2 | 高级自动编排 | P3/实验 | **已关闭**（iteration 9）：待真实多 Agent 瓶颈证据重开；不占活跃清单 |

**终态声明（iteration 14 后）**：用户指令解冻后，P2 阶段 2 与 M1 切片二 + M2 亦已实现——**笔记本存量愿景与规划中一切可自动化项皆毕**。剩余项无一可无输入推进：G1 阶段二（待痛点证据）、G1 回滚 + M1 切片三（待用户需求定义）、C1 补路由（待真实场景）、真机/生产/合并（用户轨，动线 `docs/plans/30-min-verification-session.md`）。循环回归**低频维护态**：每轮 = 全门禁绿 + 导读刷新 + 零回归。

**iteration 15–16（2026-07-24）**：15：H1 TCP、G1-OS/RB、M1s3、B6A/B3/DISC/MOB-CP、TUI/resize/paste。16：V-G1-JOB + V-H1-LIVE 最小闭环。应用版本 **0.0.18**。

**iteration 17（2026-07-24 深研弧）**：NLM deep research（交互/健壮/协作/多 host/同工作区/容错）→ **只导入报告**（豁免清理至愿景全实现）。对抗评审后落地：R17-CTX 上下文文件扫描、R17-HOST-* foreign attach/list/output、R17-RECONN 退避策略、R17-TEAM-HEALTH/HITL-BADGE。**开放愿景 open=0**；报告源已删，来源恒 1。完整 WS 出站客户端仍为后续里程（非本报告硬门槛）。

**iteration 18（2026-07-24 双报告闭合）**：笔记本再入两份报告（Actionable Brief WI 1.1–5.2 + Architectural Blueprint F1–F8）。本地双表 `docs/iterations/2026-07-24-open-vision-checklist-dual-report.md` **open=0**。对抗 residual：Job freeze 产品入口接线（`suspend_with_os`→`job_object::try_freeze_primary`）。研究源删除后来源恒 **PROJECT-STATE**。完整 WS 出站 PTY 仍为下一里程。

**iteration 19（2026-07-24 Remote 弧）**：重点 remote dual-end、multi-host team、agent 监控面板、手机触屏/滑屏→TUI。库存见 `docs/iterations/2026-07-24-remote-multihost-agent-inventory.md`。落地：`mobileTouchScroll`（alt-screen 箭头 + mouse wheel + 本地 scroll；release btn=3）；`get_orchestration_health` 入 REMOTE_ALLOWLIST + Remote Team badges；roster Suspended 可见。清单 `…-r19-remote.md` **open=0**。完整出站 WS PTY 仍下一里程。

**iteration 20a（2026-07-24 Explorer free-follow）**：文件树分隔条 free-follow — 上界=栈顶→explorer 底（挤后续 cwd）；无 pane 插件不挂空 lower（禁 50/50）；body `flex:0 1 H` + RO reclamp。`resolveExplorerStackLayout` / `computeBodyHeightFromDrag`（11 测）。清单 `…-explorer-resize.md` **open=0**。

**iteration 20b（2026-07-24 Git 进程硬护栏）**：本机观测 Ridge 父进程下 `git.exe` 堆积/重生风暴（杀 git 即重生，需提权杀 ridge）。根因：仅有 semaphore+前端 `mapLimit`，**无超时杀子进程**；卡死 `git` 占满/阻塞后外部杀进程 → permit 释放 → 重生。落地（`packages/ridge-core/src/commands/git.rs`）：
- 统一 `git_output` / `run_command_with_timeout`：墙钟超时 + Windows `taskkill /T` 进程树回收；
- `spawn_git_blocking` 许可 **acquire 超时**（默认 60s）失败关闭；
- 活跃子进程计数 + peak；`RIDGE_GIT_TIMEOUT_MS` / `RIDGE_GIT_MAX_CONCURRENT` / `RIDGE_GIT_BIN`；
- 前端 `GIT_CONCURRENCY_MIN/MAX=2/12` 与后端常量对齐；
- 确定性：`guard_tests` 4 绿（cap / semaphore 峰值 / 超时杀挂起子进程 / `get_scm_status` 真 git 冒烟）；`ridge-core --lib` 328 绿；vitest pLimit+paneGit 13 绿。
- 版本 **0.0.19** 带资产 Release（见 LOG）。合同 `CONTRACT-iteration-20.md`。

**AC4 大弧（2026-07-24 · 诚实账）**：开放规划 Note 驱动 **CONTRACT-40…49 = C1–C10（AC4 唯一 10 分）**。原标注 C50–C59 的模块**并入 C1–C10 产品路径加厚**，禁止双计为第二组 10×约 2 日。产品接线：`linkOpenHost`→manager 开链；`livePumpPolicy`/`outboundLifecycle`/`foreignHistory`→`hosts.ts`+HostsPanel；`hostSessionIsolation`→hostReconnect；`admit_*`→remote_host_impl/dispatch；AgentCenter 绑 HITL filter/orch/git 策略。Skill 硬规矩 5（升值）/ 6（约 2 日）。门禁：`gates-credit-C1`…`C10`；hosts:: 40；vitest 产品路径；protocol/matrix/process/orch/hitl。规划 **open=0**（代码侧）。sole 来源仍为本文件。

**iteration 22–31（2026-07-24 开放规划大弧 · 约 2 日×10）**：综合 NLm 三次对话 + 待实现 notes 成开放规划清单（本地 `2026-07-24-open-planning-note-from-nlm-conversations.md`）。对抗默认 **reframe 升值落地**（skill 硬规矩 5–6）。落地摘要：
- **22–24 / 29–30 H1 完整 LAN 出站**：`hosts/outbound.rs` Transport+Mock；subscribe/write/resize/detach/重订；`write_to_pty_async` foreign 漏路由修复；live 输出 cap；foreign UI 状态机；reconnect_policy 延迟。
- **25 OP-TERM-LINK**：`linkAffordance` + manager Ctrl-hover 下划线/`dataset.linkUnderline` + TUI 点击仲裁。
- **26 OP-GIT-BYPASS**：timeout/acquire 计数 + 生产 spawn 静态门禁。
- **27 OP-AGENT-CP**：orch_health `degraded`/`generation`。
- **28 OP-CAP-PARITY**：orch admit + multi-host 出站命令禁止进 REMOTE_ALLOWLIST。
- **31 OP-USER-RAIL**：`scripts/check-user-rail-gates.mjs` 假凭据 fail-closed；notes 标 implemented。
- Skill 仓库 + 本机三端：`不简单驳回/升值落地` + `约 2 日大迭代禁糊弄`。

**iteration 61（2026-07-27，Remote × Agent Center 整合）**：
- `rdg` TUI 不再启动即自动开放 LAN；LAN 与公网 Remote 均为显式启停，LAN 展示根 URL（无 `/login`）。
- Web Remote 依据 host/query 先判 LAN/cloud；LAN 直入 `startWebRemoteBoot`，不再先打云端 bootstrap，消除桌面白壳等待。
- ridge-cloud 修 host 角色误按 free quota 的根因；新增 `parked_by_quota`，只自动恢复配额所停设备，人工禁用不复活。
- 移动端工作区与保存弹层经 portal 挂 `body`；Team 统一 Bot 图标；pane Agent 动作为无尾文图标；弹层右侧动作无边框。
- Agent Center 标题栏只留标题，控制项迁入内容；roster 跨工作区聚合并标工作区；读取 Claude/Codex session JSONL 的 assistant 内容显示最近回复。
- PTY 注入 `RIDGE_PANE_ID`，tmux shim 回传创建工作区/pane，ridge-tmux native session 保存并投影 creator；Agent Center 显示未接入无头会话并可唤醒回创建工作区。
- `RidgePane` 以 OSC 标题 + 前台进程识别 Claude/Codex/Gemini/OpenCode/Aider/Copilot，幂等登记/周期对账/退出释放，统一 pane 标题态与 Agent roster。
- 提交：wind `367b293`、`0b1985e`、`3bde775`、`f110dd0`；ridge-cloud `beb87ea`。自动门禁见 §4。

## 7. 开放问题

**当前无待定夺规划问题。** 用户轨：真机验证 LAN/public Remote 首屏、移动端弹层触控、Agent TUI 退出后的 roster 收敛、无头会话唤醒；ridge-cloud 分支审查/合并/部署。请 NotebookLM 基于 iteration 61 现状，只提出一个最高价值下一步，并给出反对理由、停止条件与最小验证闭环。

## 8. NotebookLM 评审要求（沿用）

对任何下一迭代建议必须输出：对应差距 ID；当前代码证据或需补查符号；价值/风险降低/解锁力/成本/可逆性；至少一个减法方案；可判定验收信号与停止条件；是否引入新状态源、协议副本、不可恢复写路径或生产运维负担。不能映射到 §6 差距的建议默认不进近期计划。不把历史日期、旧完成度、旧测试数当作当前运行证据。

## 9. 刷新规则

发生以下任一事件时覆盖式更新本文件，并在 NotebookLM 中**替换**旧来源（不叠加版本）：跨仓协议/身份/安全边界/Remote 数据流改变；ridge-core/ridge-remote/ridge-term 所有权边界改变；发布架构改变；某「部分/未验证」能力获得或失去确定性证据；P0/P1 差距关闭、新增或优先级变化。普通 bug 修复与局部 UI 调整不写入。

## 10. NotebookLM 存量 note 愿景对账（2026-07-24）

归档路径：`docs/iterations/2026-07-24-notebook-notes-archive/`。下列每一行在状态上均为**已实现**或**已关闭—待用户轨**（知识以本文件 + git 归档为准，不再靠 note 常驻）。

| 来源 note | 主题 | 落点 | 终态 | 证据摘要 |
| --- | --- | --- | --- | --- |
| 终端架构优化… | Bug4 Git 防抖/堆积 | 工程护栏 | **已实现（iteration 20 补硬护栏）** | watcher debouncer + `git_output` 超时杀进程树 + semaphore 许可获取超时 + 前端 `GIT_CONCURRENCY_MIN/MAX` 对齐；证据 `guard_tests` 4 绿 |
| 终端架构优化… | Bug1 多行粘贴时序 | 终端 I/O | **已实现** | `TerminalManager.paste` + bracketed paste |
| 终端架构优化… | Bug6a rdg staticassets | T3 | **已关闭—待用户轨** | status/publish 脚本代码侧；产物实跑用户轨 |
| 终端架构优化… | Bug5 Clear 一致化 | ridge-term SSOT | **已实现** | `clear_scrollback` API + 单测；iteration 1 |
| 终端架构优化… | Bug6b WSL reflow | R1 真机 | **已关闭—待用户轨** | 需 WSL 真机缩放验证 |
| 终端架构优化… | Bug2 Agent 状态/编组 UI | P1 | **已实现** | iteration 6 teammate roster |
| 终端架构优化… | Bug3 图片/文件夹外部刷新产品化 | 连续性旁支 | **已关闭—待用户轨** | `fs_watch` 基建在；1s 预览自动刷新无合同，不伪实现 |
| 第五迭代周期规划 | P1 Roster / 侧栏 | P1 | **已实现** | iteration 6 |
| 第五迭代周期规划 | H1 远端 live PTY | H1 | **已关闭—待用户轨** | iteration 10 簿记 |
| 第五迭代周期规划 | T1 cargo 全绿 | T1 | **已实现** | iteration 7 |
| 第五迭代周期规划 | HITL/Header 可见性 | P2/P1 | **已实现** | iteration 8/13 |
| 第五迭代周期规划 | CLI Agent 自动发现 | M2 旁支 | **已关闭—待用户轨** | 无独立合同；拓扑靠注册非热发现 |
| 第五迭代周期规划 | 分享安全/E2 | E2/S1 | **已关闭**（E2）；S1 代码侧关闭 | iteration 5/8/9 |
| 架构同源化策略 | A1 减法 / 同源 | A1 | **已实现/关闭** | iteration 5–10 |
| 架构同源化策略 | A2 能力矩阵协商 | A2 | **已实现/关闭** | matrix + 一致性测试 |
| 架构同源化策略 | 全局滚动条/主题变量 | UI 归一 | **已实现** | `app.css` `rg-os-theme` / `--rg-scrollbar` |
| 远程终端质量指南 | 移动端复制粘贴 | R1 | **已关闭—待用户轨** | 真机触屏 |
| 远程终端质量指南 | Scrollback/Clear | ridge-term | **已实现** | clear_scrollback；§3.2 懒加载架构 |
| 远程终端质量指南 | scrollback 硬上限 | 渲染护栏 | **已实现** | `DEFAULT_SCROLLBACK = 5000` |
| 远程终端质量指南 | `git_diff_file` 远端放行 | A2 | **已实现** | allowlist + host 路由 + 合同测试 |
| 远程终端质量指南 | TUI 可点击/鼠标报告 | 增强 | **已关闭—待用户轨** | 待产品定义，非自动轨存量 |
