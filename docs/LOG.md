# Ridge 迭代日志

按时间倒序追加；每条记录包含已完成事项、下一步、熔断与被驳回建议。

## 2026-07-27 — iteration 62（阶段三：共享桌面资源投影）

- 完成：阶段二三层资源树全项；阶段三以不可委派 scoped token 建独立内存投影，将共享 Terminal、Files、Git、Search、Agent 接入桌面；真实 pane/metadata 回写接入树；不污染本机 workspace/global transport；二跳继续 fail-closed。
- 门禁：Vitest 107 文件 1255 绿 / 1 skip；svelte-check 0 errors / 2 既有 warnings；`cargo check -p ridge` 与 desktop host boundary 2/2 先前同轮绿；Remote/桌面 production build exit 0。
- 提交：`9adfc4d`、`0e71da6`、`08eeff6`。
- 发布：`main` 推至 `9a7e5f9`；`publish-remote` run `30265723678` success，已激活 `0.1.6+g9a7e5f9`（231 文件，21.50 MiB）。
- 修复：发现几何算法虽已发布，standalone Remote `MainApp` 漏接 `setSharedRemoteMode(true)`；`1d3a347` 补接 browser/embedded shared grid，重发 run `30272240047` success，激活 `0.1.6+g1d3a347`。
- 复核：公网浏览器若 WebGPU 降级 Canvas2D，shared-grid 几何原先旁路；`67e0631` 令降级路径同样 letterbox/坐标映射，run `30278843596` success，激活 `0.1.6+g67e0631`。
- 发布：移除 `RIDGE_WEBGPU` 本地降级开关，公网始终优先 WebGPU；`5f7433d`，run `30284595465` success，激活 `0.1.6+g5f7433d`。
- 未闭：R62-WS-SHARE、R62-HOST-TREE、R62-GEOMETRY 均只欠相应真实跨账号/LAN/public E2E。
- NLM 对抗评审：采纳内存投影；驳回不存在的 `RpcClient.scoped`、`CloudHostBridge.rs`、`remote_read_dir` 及“纯终端降级”，校正为 host scoped JWT/DB 授权 + 现有 RPC 复用。

## 2026-07-24 — iteration 50（未用符号 + 终端/IO 性能）

- **卡顿根因**：Hosts 每拍全量 history base64 + 双 BP IPC + 健康机仍 step 重连；hover 无 Ctrl 仍 hitTest；AgentCenter 3s 全量重 IPC。
- **性能**：hover 修饰键短路；泵/stats 并行；history 30s+展开；重连条件 step；Agent 轻重分层 + 自适应间隔。
- **未用接线**：live_bp.clear_session；reconnect attempt/cancelled/err/Idle mark_idle；attach seed_parser_feed。
- **门禁**：hosts:: 45；vitest 18；无删有业务语义的 API。
- **合同**：`CONTRACT-iteration-50.md`。

## 2026-07-24 — AC4 诚实账终态 + C8–C10 多模块垂直

- **计数**：**仅 C1–C10 / CONTRACT-40…49**；C50–C59 标签**并入**对应 C，禁止双计约 2 日。
- **产品路径**：manager `planHostOpen`；hosts 泵 + `get_live_backpressure`；cloudHostBridge `decideRemoteInvoke`；matrixParity；AgentCenter。
- **C8**：`hosts/live_backpressure.rs`（每会话 drop + 聚合命令 + inject 接线）+ TS liveBackpressure/livePump + hosts 泵。
- **C9**：matrix_guard + matrixParity + check-capability-matrix.mjs + shipped matrix 测。
- **C10**：protocol_guard + remoteInvokeAdmit + cloudHostBridge 双门 + dispatch/remote_host_impl admit。
- **硬证据**：`{SCRATCH}/skill-sync.txt`（四宿主 SHA 一致 RULE5/6）；`ac1-nlm-notes` open=0；`contracts-index` 40–49；C1–C10 gates all ok。
- **非做**：空 Release；薄单文件充约 2 日；双计。

## 2026-07-24 — AC4 诚实账：C1–C10（CONTRACT-40…49）

- **债务冻结**：原 22–31 拆账不计 10 分；合并为 C1–C4（WS-PTY / TERM-LINK / Git+process_guard / Agent CP）。
- **新主线 C5–C10**：reconnect_supervisor、foreign_history、hitl_audit+allowlist、liveBackpressure+stats、capability_matrix_guard、protocol_guard+dispatch 规范化。
- 合同：`CONTRACT-iteration-40`…`49`；ledger `{SCRATCH}/ac4-ledger.md`；门禁 `gates-credit-C1`…`C10` all exit 0。
- Skill 双硬规矩仍生效；NLm open 规划档案 + 已实现 notes 不变。
- 非做：把 22–31 再标 10 轮；空 Release。

## 2026-07-24 — iteration 20b（Git 进程硬护栏 + 0.0.19 Release）

- 根因：本机 ridge 拉起 git 堆积/重生；仅 semaphore 无超时杀进程树。
- 修：`git_output` 墙钟超时 + `taskkill /T`；acquire 超时；活跃计数；前端 MIN/MAX 对齐。
- 门禁：`guard_tests` 4；`ridge-core --lib` 328；vitest pLimit+paneGit 13。
- NLM：sole PROJECT-STATE 替换；guidance+对抗评审归档；note open=0。
- 发布：版本 0.0.19 annotated tag + release.yml 资产。
- 驳回：把 T3/真机 smoke 塞进自动轨；强行新开 A1 差距行做遥测。

## 2026-07-24 — iteration 20a（Explorer free-follow 压缩下方展示域）

- 根因：cwd body 与 plugin 无列栈 flex；lower `min-height:auto` 顶住分隔条。
- 修 v1 列栈 + clamp。Skeptic 纠偏：无 pane 插件不挂空 lower；body `flex:0 1 H` + RO reclamp；free-follow 上界=栈顶→explorer 底（挤后续 cwd）。
- 门禁：`explorerLayout.test.ts` 11 绿；清单 open=0。
- 并行会话其他愿景未动。

## 2026-07-24 — iteration 19（Remote / multi-host / agent 监控 / mobile）

- 库存：`2026-07-24-remote-multihost-agent-inventory.md`（dual-end / hosts / panel / touch）。
- 修：mobile `decideTouchScroll`（alt-screen 方向键 + SGR wheel + local scroll）；touch release btn=3 对齐桌面；Remote Team orch health allowlist + badges + Suspended 点。
- 门禁：mobileTouchScroll + capability/allowlist vitest；hosts/orch_health cargo。
- NLM：sole PROJECT-STATE；note `[已实现]` open=0。
- 驳回：完整 WS 出站 PTY；Agent Center 视觉大改。

## 2026-07-24 — iteration 18（双报告愿景闭合）

- 完成：NLM 两份深研（Actionable Brief + Architectural Blueprint）本地归档 + 全表对抗清单 `open-vision-checklist-dual-report.md` open=0。
- Residual F2：`suspend_agent` 读 `PtyHandle.job`/`child_pid`→`try_freeze_primary`；**None 路径直 `os_freeze`，禁 create_job**（防 create 失败导致永不 thaw）；`resume_agent` 同理。
- 测：`try_freeze_thaw_none_matches_os_freeze_no_create_job`；`suspend_with_os_real_entry_soft_and_require_os`。
- 门禁：job_object 6 / suspend 4 / hosts 7 / reconnect 2 绿；来源恒 PROJECT-STATE，note `[已实现]`。
- 驳回：保持 1.2/2.1/3.2/4.1/4.2、`/effort`、CRDT、cgroup 内存条；完整 WS 出站 PTY 下一里程。

## 2026-07-24 — iteration 15（开放愿景 11 项代码闭合）

- 完成：CONTRACT-15 锁定 11 项全落地——V-TUI-CLK 核查既有 `encode_mouse`；V-PASTE 100 行序；V-B6A `REMOTE_UI_MISSING`；V-MOB-CP `copySelectionOnly`；V-B6B resize 尺寸测；V-M1-S3 memory goal/constraints/tasks + Agent Center；V-G1-RB git checkpoint/rollback；V-G1-OS `os_freeze`+soft fail-open；V-DISC 注入进程表；V-B3 imageVersion；V-H1 TCP probe+attach 门控。
- 验证：`cargo test -p ridge --lib` 114 绿；`ridge-term` paste/mouse/resize 绿；`ridge-remote` remote_ui 2 绿；vitest mobileCopy+imagePreviewVersion 5 绿。
- 熔断：无。未做：完整出站 PTY 流、Job Object 预建、生产部署/merge。
- 驳回：无。

## 2026-07-24 — iteration 17（深研弧：交互/多 host/协作/容错）

- Skill：深研只导入报告、禁源网站、报告**豁免清理直至衍生愿景全实现**（四端同步）。
- 调研：deep research；报告归档；**WI 1.1–5.2 全表**对抗（implemented*/rejected）后落地。
- 补 skeptic 缺口：HOST-OUT **fanout→parser**；RECONN 接 cloud scheduleReconnect + tmux `send_retry`；CTX 在 spawn-process(is_agent) 写 env；AgentCenter **HITL badge + 暂停数**。
- 门禁：hosts 7（含 fanout）、teammate 39、reconnect rust+vitest 绿。
- 报告源删除；来源恒 1；清单 open=0。

## 2026-07-24 — 换机交接文档

- 完成：`docs/handoffs/2026-07-24-machine-switch-0.0.18.md`（Remote 云发布步骤、可选安装包、门禁、NLM、PR 顺序）。
- 下一步：换机 pull → `pnpm publish:remote-cloud`。

## 2026-07-24 — iteration 16（有价值非功能项纳入 + 0.0.18）

- 评估：完整出站 PTY、Job Object **纳入**；真机/生产/merge **不纳入愿景**（运维/Level2）。
- 完成：V-G1-JOB（spawn 预建 Job+assign）；V-H1-LIVE 最小闭环（live_sink 路由、attach_host_session、write 经 remote_ref）；版本 0.0.18。
- 验证：hosts 4 绿；job_object 2 绿。
- Release：桌面尝试 `build:release`；Remote 云发布本机无 `RIDGE_ARTIFACT_TOKEN` → **换机再发**。
- 下一里程：H1-LIVE 完整 WS 客户端（复用 ridge-cli lan_session 语义）。

## 2026-07-24 — iteration 15（开放愿景清单代码闭合）

- 完成：CONTRACT-15 拍板 + 实现 11 项 V-*（H1 TCP 探测、G1-OS/RB、M1s3、B6A/B3/B6B、DISC、MOB-CP、TUI-CLK/PASTE 核查）；清单 note → `[已实现]`，open=0 视同清理。Skill 四端同步（Notes 清空≡愿景全实现）。
- 验证：`cargo test -p ridge --lib teammate` 34 绿；hosts 3 绿；ridge-term paste/resize 绿；ridge-remote remote_ui 2 绿；vitest mobileCopy 2 + imagePreviewVersion 3 绿。
- 熔断：无。未做：完整出站 PTY 流、Job Object 预建、真机/生产/merge。
- 驳回：无限「等用户」挂起——改为执行者拍板实现。

## 2026-07-24 — 规则纠偏 + 重建开放愿景清单

- 完成：更新并四端同步 `notebooklm-iteration-loop` Skill；重建开放清单（当时 11 open）。
- 后续：见 iteration 15 闭合。

## 2026-07-24 — note 对账归档轮（已纠偏：误清空 notes）

- 完成：4 份历史 note 全文本地归档；PROJECT-STATE §10 对账；替换来源。**错误**：将未代码落地项标关闭并清空 notes。
- 纠正：见上条「规则纠偏」。

## 2026-07-23 — iteration 14（M1 切片二 + M2，存量终轮）

- 完成：`memory.rs` 单源 doc 级 RMW（互斥+原子写+updatedAt 元数据+空删；suspend 持久化改经之，DIR OnceLock 单点注入，删双解析路径）；三消费点裁决审计落盘（桌面/远端含败者尝试/超时 fail-closed；无命令全文、环形 50）；**M2 归因**（initiator → 稳定 agent_id）；读方 `list_hitl_decisions`（仅桌面 IPC）+ Agent Center 审批历史区。
- 验证：cargo workspace exit 0；teammate:: 25 绿；vitest 559/1skip；svelte-check 0 errors。
- 熔断：无。收窄注记：already-resolved/bad-verdict 尝试无条目归属不落审计（范围如实记）。
- **循环状态：可自动化存量全毕（P2/M1s1s2/M2 在内），回归低频维护态。剩余项全需外部输入：G1 阶段二待痛点证据、G1 回滚 + M1 切片三待用户需求定义、C1 补路由待场景、真机/生产/合并属用户轨。**

## 2026-07-23 — iteration 13（P2 阶段 2 实现，用户指令解冻）

- 完成：远端 HITL 裁决通道——一次性票据（nonce 恒时比对+取出即毁=单次消费原子）、verdict 仅 approve/reject（**modify 永不开放**）、四态结局、超时后无副作用；`resolve_hitl_remote` 六处宣告 + MUTATING 双侧归类；负断言维持（桌面版裁决/网关开关/暂停命令仍不可远达）；Team 面板双按钮+结局反馈。**P2 全闭**。
- 验证：cargo workspace exit 0；hitl 2 绿；合同/parity 23 绿；vitest 559/1skip；svelte-check 0 errors。
- 熔断：无。解冻授权存照于 CONTRACT-13（用户「按你审理解继续推进」）；Level 2 与导读节律不变。

## 2026-07-23 — iteration 12（收敛轮）

- 完成：30 分钟核验会动线（用户轨一次清偿，覆盖 checklist 全条目 + 历轮顺带核验）；G1 阶段二/M1 余切片/M2 簿记归档（待证据/解冻重开）；维护态定型入 WORKFLOW；PROJECT-STATE 转终态声明。
- 验证：门禁全绿复验（cargo exit 0 / vitest 559 / svelte-check 0 errors）+ 导读刷新。
- 熔断：无；本轮零代码。
- **循环状态：自动轨存量做尽，低频维护态生效。解冻 = 用户轨首份证据（动线文档任一件）；解冻首轮 = P2 阶段 2 实现（设计已备）。**
- 驳回：NotebookLM「evidence 打包脚本」（YAGNI，各件证据形态已定义）。采纳：转维护态、G1 阶段二同 E1 处置、核验会动线主线。

## 2026-07-23 — iteration 11

- 完成：**M1 切片一关闭**——suspended panes 落 sidecar（原子写/空集删文件/损坏容忍 fail-open）+ 启动载入重挂 + 全写方钩落盘 + `close_workspace_core` 单点清理；暂停态跨重启存活（dir 注入单测 3/3）。**P2 阶段 2 设计定稿**（一次性裁决票据/单次消费/远端 modify 永不开放/首达生效+审计/传输面选 teammate 新方法），实现按红线冻结。
- 验证：cargo workspace exit 0；vitest 559/1skip；svelte-check 0 errors。
- 熔断：无停机触发（启动载入无竞态，懒载降级未启用）。
- 下一步：iteration 12 收敛轮（NotebookLM 裁决：直接转低频维护态）。
- 驳回：NotebookLM「ridge-core 载 sidecar」层错、「200ms 停机线」不可判定（改 fail-open 韧性条款）、`docs/designs/` 路径。采纳：M1 主线、维护态红线表述、Windows 竞态存照。

## 2026-07-23 — iteration 10

- 完成：**A1 关闭**——close/rename 三/双副本同源化为 `close_workspace_core`/`rename_workspace_core`，**修实缺陷**（LAN 副本漏发 WorkspacesChanged/WorkspaceListChanged、names 残条泄漏），broadcast 订阅测试钉死，净删 ~80 行。M1 设计定稿（sidecar json、6 字段、三切片序）。**C1 关闭**（五缺口判定入脚本 JUDGMENTS 零残留）。**H1 簿记关闭**（待需求证据重开）。节律固化（闭环必刷导读、积压期不扩协议面）。
- 验证：cargo workspace exit 0；workspace::tests 2 绿；vitest 559/1skip；svelte-check 0 errors；双脚本 exit 0。
- 熔断：无停机触发；同源化未暴露锁序冲突；rename 未触发减法条款。
- 下一步：iteration 11——M1 切片一（suspended panes 落 sidecar + 启动恢复）主线 + P2 阶段 2 设计文档 + 维护节律。存量趋收敛：其后仅剩 G1 阶段二与 M2/M1 后续切片，做尽即转低频维护态待用户轨。
- 驳回：NotebookLM「ridge-core 自动加载 sidecar」（层错——suspend 居 src-tauri）；「关区写入 >200ms 停机线」（不可判定，改 IO fail-open 韧性条款）；`docs/designs/` 路径（归 `docs/specs` 惯例）；M2 并轮建议（留待切片一落地后裁决）。采纳：M1 主线、低频维护态与「制造工作」红线、Windows 竞态缺口文档声明存照。

## 2026-07-23 — iteration 9

- 完成：**G1 阶段一关闭**——软暂停/恢复（suspend.rs 进程级注册表；agent 写路径四所归一至 `agent_pty_write` 唯一收口；人类输入/断路器刻意不门控；拓扑双路径 Suspended 投影无新字段；Agent Center Pause/Play；暂停命令仅桌面 IPC + 负断言）。C1 清单化（rdg-gap-report 脚本，3 支持/5 缺）。审查导读包自动化（36 提交分组 + 协议/安全面标注 + 计数自校验）。E1/E2 簿记校正（WebGPU 生产默认不得删）。A1 写路径审计**发现实缺陷**：LAN close 第三副本漏双广播 + 多删 names。
- 验证：cargo workspace exit 0；`-p ridge --lib` 96 绿（+3）；vitest 559/559（1 skipped）；svelte-check 0 errors；双脚本 exit 0。
- 熔断：无停机触发；G1 确认四写点即 agent 写径全集（spawn 属进程注入面留阶段二边界）。
- 下一步：iteration 10——A1 close/rename 三副本同源化 + LAN 漏广播修复（主线）+ M1 设计文档 + H1 簿记降级 + C1 判定入脚本 + 审查节律固化。
- 驳回：NotebookLM「Vitest 验 LAN 广播」（层错——LAN close 乃 Rust，改 cargo 订阅 remote_structural_tx 断言）；M1 struct-only 三度提案（无消费者死脚手架，改设计文档先行；其「暂停态重启即失冲突可续愿景」论据采纳）；H1「移除开发指针」模糊验收（收敛为状态行变更）。采纳：A1 主线、H1 降级类 E2、审查包节律、C1 判定落脚本 JUDGMENTS 保单源。

## 2026-07-23 — iteration 8

- 完成：**P2 阶段 1 关闭**——`list_hitl_pending` 脱敏只读端到端（PENDING 注册表加宽存元数据；投影恰五字段绝不含 action 命令全文，Rust 钉死；六处宣告同步；Team 面板 Pending approvals 只读区；裁决通道保持不可远达）。S1 遥测二阶段：F3/F4 进程内计数 + 测试；**F5 `keyBindingVerifier` 钩子退役删除**（生产零接线双证，0x02+B3 覆盖，net −173 行档）。A1：pane.rs 读写分类审计 + 删 rustc 确证死码二处（pane_tree first/last_leaf、parser full_reframe_with_scrollback，−60 行）。G1 设计文档定稿（三层边界/Windows 三候选/顺序不变量/HITL 计时不暂停）。计划外根修 deviceTrust Node localStorage 残缺对象地雷（探针 + 模块级内存回退）。
- 验证：`cargo test --workspace` exit 0；vitest shared 559/559（1 skipped，删 9 增 1 帐目符）；cloud 236 绿；svelte-check 0 errors；`-p ridge --lib` 93 绿。
- 熔断：无停机触发；F5 删除未触发回滚条款。
- 下一步：iteration 9——G1 阶段一实现（send-keys 网关门控软暂停，纯本机零协议面）+ C1 缺口报告 + 分支审查导读包 + E1/E2 簿记 + A1 写路径审计文档。
- 驳回：NotebookLM「删 WebGPU 实验代码」——**重大驳回**（`default=["webgpu"]` 生产默认 + 2026-05-05 用户反馈钦定运行时探测；错在状态文档陈旧措辞，E1 改簿记校正）；M1 无消费者结构体（死脚手架）；G1 停机条件混淆阶段（Job Object 属阶段三）；`pnpm build`/不存在的 `test:conformance` 作验收；bash+正则审查包（改 node .mjs）。采纳：G1 主线、审查辅助包属自动轨、C1 矩阵派生收口、E2 关闭待证据。

## 2026-07-23 — iteration 7

- 完成：证据与固化轮四目标全落——**T1 完全关闭**（loader 根修：`comctl32!TaskDialogIndirect` 仅 v6 导出、lib 单测宿主无 manifest 绑 WinSxS 5.82 → `/DELAYLOAD` 推迟绑定；`-p ridge --lib` 92/92 本机首次全绿，`cargo test --workspace` 首次整仓 exit 0，附 boot smoke 防回归）；R1 实验室轨关闭（抽 `__faultRig.ts`，weakNetLab 九场景参数化扫描 + metrics.json 校验脚本 exit 0，含非真机 disclaimer）；A1 审计 Teammate 零死字段（驳回删字段建议）；固化（用户必办四件单页、README_CN 能力协商+Team 段、WORKFLOW 双轨制段）。
- 验证：vitest shared 全伞 567/567（1 skipped，37 文件）、svelte-check 0 errors、weaknet-lab 脚本 exit 0、整仓 cargo exit 0。
- 熔断：无停机触发；G2 扫描未暴露缺陷；G3 零删除为合同允许结论。
- 下一步：iteration 8——P2 第一阶段 `list_hitl_pending` 只读端到端（脱敏投影，绝不含 action 全文）+ G1 暂停/恢复设计文档（零代码）+ A1 pane.rs 审计 + S1 F3/F4 计数与 F5 删除审计。
- 驳回：NotebookLM 臆造 `nonce`/`category` 字段（代码无此，现有 id/level/reason/initiator 即够）；`telemetry.log` 持久面（违锁定的进程内计数设计）；`AgentStatus` 加死枚举变体（违 YAGNI 且其 SSOT 论据张冠李戴）；A1「至少删 2 处」配额（审计前无据）；T3 CI 桩挂钩（本仓无 CI，屡次臆造）；T1 boot smoke 当新目标（已交付）。关键补正：NotebookLM 未见 `PENDING` 注册表只存 sender 不存元数据——`list_hitl_pending` 前提是扩注册表，已入合同。

## 2026-07-23 — iteration 6

- 完成：P1 控制台 MVP 代码侧关闭——新协商能力 `teammate`（唯一只读方法 `get_teammate_topology`）六处同步宣告 + LAN/cloud 路由 + 共享 controller Team roster 面板（轮询/切 pane/无团队零噪声）；投影脱敏 Rust 测试与 HITL 不可远达负断言。S1 遥测第一阶段（F1/F2 进程内计数 + 测例钉死）。A1 切片：workspace 列表投影同源化（删平行 WorkspaceInfo，net −12）。
- 验证：vitest shared 全伞 558/558（1 skipped）、svelte-check 71 files 0 errors、cargo workspace-excl-ridge 882 绿、ridge bins 27 绿、`--lib --no-run` 可编译。
- 熔断：无停机触发；cloudHostBridge 硬编码七能力清单按合同更新为八。
- 下一步：iteration 7 证据与固化轮（冻结新功能）：主线修复 `-p ridge --lib` loader 载败；弱网实验室参数化 harness；Teammate 字段审计；用户必办件清单单页。
- 驳回：NotebookLM 的「TURN-only/真实丢包自动化验收」（需真实网络工装/凭据，改双轨：自动轨实验室 harness 显式标注，不冒充真机结论）；T3 实跑与真机归档入自动合同（凭据/设备在用户手，列用户轨）；「清理 Teammate 冗余字段」直接删（收窄为审计先行）。采纳：冻结轮、loader 修复升格、`list_hitl_pending` 方案归档待 P2。

## 2026-07-23 — iteration 5

- 完成：可信基线固化五目标全落——S1 构造点矩阵 + 3 条回落钉死测试（发现并更正「无 transcript 时 trust-proof 直接失败」的错误注释：实为退化为无信道绑定 + 信任库裁决）+ 遥测/退役设计；T3 ridge-cloud 只读 status 端点（124 绿，独立分支）+ wind 一键两线脚本（桩验四径）；T1 双仓 cargo 绿灯（wind 882+27，cloud 124）；A2 机器可读矩阵 + 6 条一致性测试；A1 审计报告 + 删死 pane-output 面（−45 行）。
- 计划外：根治 signaling drift 门禁 Windows EOL 误报（.gitattributes 钉 LF）；vitest cloud+transport 全伞 382 绿。
- 熔断：`cargo test -p ridge --lib` 宿主 `STATUS_ENTRYPOINT_NOT_FOUND` 载败（loader 级、先于本轮、其余全绿）——如实记录不宣称全仓绿，根因挂开放问题。
- 下一步：iteration 6 主线 P1 Remote Agent 控制台 MVP（capability `teammate`、只读 `get_teammate_topology`、轮询、rdgHost denied）+ S1 计数实施 + A1 只读三件套同源化。
- 驳回：NotebookLM 的 `agent_roster_view` 命名（改 `teammate` 对齐短名词惯例）、`subscribe_agent_updates` 订阅流（MVP 轮询即可）、rdg-Host Supported（无 roster 数据源，denied）、`telemetry.log` 新持久面（按设计用进程内计数）、T3 挂 CI（无测试 CI，属用户决策）、移除 WebGPU 构建检查（违反 E1「先证伪再删」）。

## 2026-07-23 — iteration 4 收口 + 笔记本压缩

- 完成：补跑 iteration 4 自动验收全绿（faultInjection 7/7 含两条新 watchdog 升级时序、Cloud 定向 156/156、增量 svelte-check 0 errors、evidence 校验脚本自测 exit 0）；写收口报告。真机双平台 smoke 证据仍空，保持唯一用户必办件（runbook/schema/校验脚本已备）。
- 笔记本压缩：三份基线文档 + checker 附录导出归档至 `docs/ridge-baseline-digest/`；骨架合并进新唯一状态文档 `docs/PROJECT-STATE.md`；上传确认后删除全部 7 份旧来源，终态 1 来源（经用户「轻量多余来源」指示授权）。
- 下一步：iteration 5 可信基线固化——S1 构造点矩阵 + 门禁测试主线；支线 T3 只读 status 端点与汇总脚本、T1 双仓 cargo 绿灯、A2 矩阵机器可读化、A1 收窄审计。
- 驳回：NotebookLM 的 S1 验收「100% 遥测断点」（不可判定且本轮不实现遥测）；T3「Dokku API + curl 探测 artifact 指针」（grep 证实无 GET 状态端点、health 无 SHA，改为新增只读端点 + 脚本）；A1「删 3–5 个 handler」数字目标（收窄为审计 + 至多一个示范删除）。

## 2026-07-21 — iteration 3

- 完成：建立 Controller provider→adapter→RpcClient 与 Host 背压的确定性 fault-injection 门禁；100 周期验证无 pending RPC、重复恢复或 timer 泄漏。
- 修复：L2 原先在 E2EE connected 但 TOTP 未授权时提前发送 hello/pane recovery，Host 丢弃后不补发；现以 connected + authorized 为 business-ready。
- 验证：修复前新门禁稳定红灯；修复后 fault 5/5、Cloud 定向回归 161/161、增量 svelte-check 70 files/0 errors、Remote build 均 exit 0。
- 下一步：iteration 4 做 iOS Safari/Android Chrome 真机换网与后台生命周期 smoke；短前置补 watchdog/deadline 两条升级时序和外置证据模板。
- 驳回：不新增已存在的 watchdog/deadline；不为 smoke 加产品遥测；Not-ready 排队和 Agent roster 不夹带进入本轮。

## 2026-07-21 — iteration 2

- 完成：建立 Controller-facing 最小 capability→RPC 合同与跨入口测试；补齐 rdg `get_file_tree/read_file/text_search` 实际路由；Remote Files/Git/Search、workspace 管理与 theme UI 按 D9 能力协商隐藏/收敛。
- 验证：修复前门禁稳定命中 `get_file_tree` 缺路由；修复后 Remote 定向 Vitest 47/47、ridge-cli rpc 16/16、session 10/10、ridge-core capability 10/10、dispatch 25/25、Remote production build 均 exit 0；增量 svelte-check 70 files、0 errors、exit 0。
- 下一步：iteration 3 仅做 Cloud Remote 重连/背压/恢复的确定性跨层 fault-injection 门禁，优先只改测试。
- 熔断：默认 `pnpm check` 300 秒零诊断超时；checker 证明 `--incremental` 24.6 秒可完成，故未误升为工具链 P0。全仓 rustfmt 被既有格式债务阻断；未批量改无关文件。
- 驳回：NotebookLM 的 `pnpm check --filter ./packages/*` 命令无效；随机 5% 丢包 100% 和 NLC 不作为 CI 验收；Agent 控制台仍缺 Remote topology/安全合同与真实 pane-switch API，继续延期。

## 2026-07-21 — iteration 1

- 完成：校准《远程终端迭代规划与质量修复指南》的陈旧项；修复 Cloud TS capability mirror 缺 `get_workspace_snapshot`、mutating mirror 少 11 项；将固定计数测试改为 Rust canonical 逐项 parity。
- 验证：Remote 定向 Vitest 48/48、transport conformance 32/32、`ridge-term` clear 定向测试、WASM dev build、Remote production bundle 均 exit 0。
- 下一步：iteration 2 仅做“能力宣告→allowlist→handler→UI”的跨入口合同门禁。
- 熔断：note 五类原始建议大多已实现，触发来源陈旧熔断；`pnpm check` 可启动但 180 秒无诊断后超时，完整门禁未宣称全绿。
- 驳回：NotebookLM 的“恢复工具链为 P0”已过期；Remote Agent 控制台在数据能力远程化前不启动；弱网与真机风险无数据，不做确定排序。
- 后续：用户授权跨仓修改后，已同步最新 ridge-cloud，并将 wind 陈旧协议全文收敛为 canonical 入口 + 自动守卫；协议双 SSOT 债务关闭。
# 2026-07-29 · Iteration 65 deep research

- 需求门禁：`requirements_gate.py assert-executable --file docs/REQUIREMENTS-SPEC.md` 通过；已批准 Commune/Remote 两项 Active。
- CodeGraph：`codegraph sync` 成功；复合 pane 身份、键盘回底/光标锚定、Agent 状态同步及 WebWorker 候选已纳入合同 65。
- NotebookLM：以 `REQUIREMENTS-SPEC`、`PROJECT-STATE` 与深研报告查询下一步；结论为 Worker 仅卸载 CPU 解析/合并，服务端仍须独立 bounded priority writer。
- 验证：`teammateModel.test.ts`、`keyboardOffset.test.ts` 22/22 通过；全量 Vitest 现有 12 项失败，集中于 Remote API 复合 `PaneRef` 迁移兼容（cloudRemote、lanWsAdapter、remotePaneBindings），未宣称全绿。
## 2026-07-29 · iteration 65（Remote 连续性收口）

- 完成：严格复合 `PaneRef`、Worker 分页解码/seq 验证、cursor-only 键盘回底、RemoteTab 切换失败回滚、Agent roster→pane header 同步与 Commune 控制区移底。
- 完成：LAN WebSocket sink 独占 writer；高优先 control/active 与有界低优先 background/scrollback 分 lane，低队列满只标 desync，不阻断 reader。
- 验证：Vitest 109 文件 1263 通过 / 1 skip；svelte-check 0 error/0 warning；cargo check 成功；Rust lane 优先级测试通过。
- 未闭：真实手机/桌面弱网 E2E 与用户真机验收仍需执行；Worker 已落地但性能收益须由 trace/真机量测证实。
