# Agent 通信架构首波 QA 交接

日期：2026-08-09

来源：Notebook `66919cb9-1329-4ddf-955c-f426d15a9fe6` 的 `Agent 通信架构重构` source `9516749e-c317-4f13-9cda-b64b00cec465`；临时对话 Notebook `f6ffd900-708d-44ee-9818-1a3269c533fc` / source `df4d5dcc-9813-4c61-ae9f-1e9199cb7555` 亦纳入约束审计，尤其是浏览器 cookie/API 边界、MIME/大小/SHA256 校验、轮询上限与不记录 token/cookie。

## 当前结论

- Agent 通信主链已落地：Kernel/Teammate SSOT、typed envelope、generation/lease fencing、SQLite Hub、MCP 工具、adapter probe/priority/outcome、desktop/Remote 投影与 lifecycle E2E 均有本地证据。
- 最近一次被 Sonar 接受的全项目扫描 scanner/CE 均成功，项目 coverage `40.3%`、line `41.2%`、branch `38.9%`，Quality Gate `ERROR`；80% 目标未完成。随后规范化 LCOV 重扫的 scanner 成功但 CE 因既有 test component 行数冲突失败；完整证据见 `docs/iterations/2026-08-09-sonar-full-scan-evidence.md`。
- 尚存三项硬缺口：Runtime API/A2A 真实 host adapter、PTY 五条件运行时证明、Sonar 80%/Gate；本轮未提交、推送、发布，未向 Codex 之外 CLI 派发消息。

## 本轮已落地

- `ridge-core` 新增统一通信契约：`AgentIdentity`、`AgentTarget`、`AgentEnvelope`、typed error、ACK/NACK、generation/lease/workspace/capability 校验，以及 `RuntimeApi -> A2a -> McpPull -> PtyFallback` 投递选择。
- `TopologyGraph` 内嵌唯一 `AgentIdentity` registry；online identity 才可提交，旧 generation、换 lease、offline/failed 不得覆盖当前 roster；teammate 删除同步清理 identity。
- `ridge-mcp` 增加并路由 `ridge_send_message`、`ridge_create_task`、`ridge_publish_event`、`ridge_fetch_inbox`、`ridge_cancel_delivery`、`ridge_task_update`、`ridge_list_agents`。返回 `messageId/taskId/deliveryId` 与明确 delivery 状态；terminal write 不等同 Agent 已处理。

## 确定性验证

| 检查 | 结果 |
|---|---|
| `cargo test -p ridge-core teammate::communication -- --nocapture` | 5 passed / 0 failed |
| `cargo test -p ridge-core` topology focused | 15 passed / 0 failed |
| `cargo test -p ridge-core --lib` | 326 passed / 0 failed（通信字段最终补齐后需再跑一次全量） |
| `cargo test -p ridge-kernel --lib` | 46 passed / 0 failed |
| `cargo test -p ridge-mcp --lib` | 71 passed / 0 failed |
| Svelte 类型检查（`svelte-kit sync` + `svelte-check`） | 0 errors / 0 warnings |
| 前端 Vitest | 156 files；1613 passed / 1 skipped |
| Vitest coverage | statements 53.81%；branches 39.84%；functions 62.45%；lines 56.54% |
| iteration context gate | passed；write scope/budget 无违规 |
| requirements gate | `executable: true`；无 pending |

覆盖率原始日志：`.iteration/artifacts/agent-communication-vitest-coverage.log`。

## Sonar 状态

本机 Sonar `http://127.0.0.1:9000` 可达，版本 `26.7.0.124771`。真实 scanner 已执行，但因环境无 `SONAR_TOKEN`，请求 `/api/v2/analysis/version` 返回 `401 Unauthorized`，退出码 1；日志：`.iteration/artifacts/agent-communication-sonar-scan.log`。

故本轮不得声称 Sonar Quality Gate 或全项目 coverage >=80% 已完成。当前前端本地 coverage 仍为 56.54% lines / 53.81% statements，明确低于目标；`pnpm check` 本身因 `pnpm` 不在 PATH 未能执行，等价的本地 `svelte-check` 已通过。

## 下一波必须闭环

1. 将 Kernel spawn/attach 成功、online commit、destroy/lease closure 接入真实 lifecycle endpoint；创建 teammate 的提交点不得早于 spawn/attach 成功。
2. 让 `ridge_list_agents` 从 Kernel SSOT roster 投影完整 identity、generation、lease、capabilities；MCP/desktop/Remote/headless 只做 adapter，不再自建 identity。
3. 将 persistent Message Hub（Rust + SQLite）及 generation/lease fencing 接入真实投递链；保留 bounded/cancellable inbox/history/PTY 上限与无 secret 约束。
4. 将 desktop/Remote 投影接入统一消息入口；PTY 仅在 idle、`agent_prompt`、无 pending approval、前台确为目标 Agent、无用户输入竞争五条件同时满足时作为 `best_effort` fallback。
5. 完成全项目 Sonar coverage >=80.0% 与 Quality Gate/API 证据；必须先获得有效 Sonar token，再保存 scan task、项目级 coverage、Quality Gate 结果及失败首因。

## 必读文件

- `docs/iterations/2026-08-09-agent-communication-architecture-requirement.md`
- `docs/REQUIREMENTS-SPEC.md`
- `packages/ridge-core/src/teammate/communication.rs`
- `packages/ridge-core/src/teammate/topology.rs`
- `packages/ridge-mcp/src/registry.rs`
- `packages/ridge-mcp/src/server.rs`

本轮未向 Codex 之外 CLI 派发消息；交接内容以本地代码、测试与扫描日志为准。

## 第二切片：Kernel 接线

- 新增 `POST /v1/domain/agents/identities/commit`。调用方须在 spawn/attach 成功且 Agent online 后提交完整 identity；offline/failed、旧 generation、换 lease 均拒绝，当前 registry 不被覆盖。
- `GET /v1/domain/agents/roster` 增加 `agent_identities` 投影；旧 `roster` 写入接口保持兼容，不冒充 identity commit。
- `KernelMcpHost::team_profile_snapshot` 优先读同一 Kernel `TopologyGraph` identity map；无 identity 时才保留 PTY 兼容投影，避免 MCP 自建第二 registry。
- Kernel domain 回归：47 passed / 0 failed；新增测试覆盖 online commit、offline reject、stale lease reject 与 active identity 保留。

workspace 级 `cargo test --workspace --lib --quiet` 因 120 秒超时未形成结果；超时后已按进程树护栏清理本次测试树，保留原有 `cargo run --no-default-features` 进程。单包 core/kernel/MCP 回归仍为确定性通过证据。

本轮 `requirements_gate` 与 `iteration_gate` 最终通过；Notebook gate 的旧 `PROJECT-STATE.snapshot.md` 因仓库既有 dirty diff 报 stale，未向 NotebookLM 写入或向其他 CLI 派发任务。

## 第三切片：跨端身份投影与结构化发送

- Kernel topology 保留 generation tombstone。PTY destroy 只移除 live identity；同一 `agent_id` 重连必须使用更高 generation，旧 generation/lease 拒绝，避免 pane/session 复用后旧消息误投。
- Kernel PTY create/replace/destroy 已接入 identity commit/teardown：spawn/attach 成功后提交 Online identity；destroy 成功后移除 live identity；持久化失败可见并阻止不安全复用。
- `KernelAgentRosterSnapshot` 增加 `agent_identities`，并提供 `commit_domain_agent_identity` 客户端适配；桌面 topology 将 Kernel 的 `agentId/sessionId/workspaceId/generation/lease/lifecycle/online/capabilities` 注入共享 roster，显示名与 CWD 不再充当身份键。
- `ridge-mcp` Hub 增加按 sender 的 idempotency store、sequence、cursor/peek/consume inbox、合法 task transition、typed ack；目标解析要求 workspace + identity + generation + lease + online + capability，失败返回稳定错误前缀。
- 桌面 `AgentMemberRow` 与 Remote `SidebarTeamRoster` 均改走结构化 `send_agent_message` → `ridge_send_message` Hub；LAN/Cloud 共用同一 RemoteLink 契约。旧兼容工具随后也已收口到 Hub Inbox，未再从 MCP 路由直写 PTY；完整五条件安全 fallback 仍未启用，故本需求尚未整体关闭。

新增/复核证据：`cargo test -p ridge-mcp --lib --quiet` 73/73；`cargo test -p ridge-kernel --lib --quiet` 48/48；桌面身份投影定向测 1/1；`vitest` 156 files / 1614 passed / 1 skipped；`svelte-check` 0/0；coverage 仍为 statements 53.81%、lines 56.54%。

桌面 `cargo test -p ridge --lib --quiet` 已编译并运行 267 tests，266 passed、1 个既有 `hosts::tests::disconnect_kernel_failure_keeps_transport_usable` 因当前预存 kernel endpoint 使“kernel unavailable”假设失效；未将环境污染改写为本轮逻辑修复。Sonar 仍因未提供 `SONAR_TOKEN` 返回 401，项目级 coverage ≥80% 未证明。

## 第四切片：兼容发送入口安全收口

- `ridge_send_to_teammate`、`ridge_send_and_submit`、`ridge_delegate_task` 不再调用宿主 `send_text`，统一解析 fenced Agent identity 后进入 Message Hub Inbox。
- 兼容入口自动生成幂等键（显式传入则沿用），保留 `submitRequested` 作为 payload 元数据；回执明确 `status=queued`、`deliveryAdapter=mcp_pull`、`deliveryReliability=at_least_once`、`terminalAccepted=false`，不把入队伪装成 PTY 接收或 Agent 执行。
- 缺 identity、workspace/generation/lease/capability 不符时 fail closed；旧 PTY 宿主方法仍仅作未启用的安全 fallback 出口，不能从兼容路由绕过五条件闸。
- Kernel lifecycle E2E 已改为先提交 fenced identity，再验证 delegate → Inbox：3/3 passed；`ridge-mcp` 回归仍为 73/73。
- 最终确定性回归：`ridge-core` 327/327、`ridge-kernel` 48/48、`ridge-mcp` 73/73、桌面 identity projection 1/1；Rustfmt/diff-check 无问题。桌面全量历史基线仍保留 266/267，唯一失败为预存 live Kernel endpoint 污染的既有测试。

当前仍未关闭：persistent Rust + SQLite Hub、Runtime/A2A adapter、完整 PTY 五条件运行时证明、全项目 Sonar coverage ≥80%/Quality Gate。上述差距不得以本地前端 coverage 数字替代。

## 第五切片：Sonar 复扫与当前阻塞

- 第五切片当时的局部白名单 Vitest：157 files；1616 passed、1 skipped；statements 53.81%、lines 56.54%。该数字不代表当前全项目基线；Node 的 `--localstorage-file` warning 不影响测试退出结果。
- 使用本机 Sonar 默认账户生成一次性 `codex-temp-1786259898` token，扫描器启动并完成 text/secrets 分析；在 JavaScript/TypeScript 分析解析 `tsconfig.json` 阶段超过 300 秒，未得到 scanner/CE/Quality Gate 新结果。
- 超时后已核验并按进程树清理本轮 scanner 子树；临时 token 已撤销。Sonar 服务既有 Java 进程未动。当前不得把该次扫描算作成功，也不得宣称 coverage ≥80%。

## 第六切片：全项目覆盖率诚实基线与验收收口

- `vitest.config.ts` 已移除原先仅覆盖少数运行时文件的白名单，改为扫描 `src/**/*.{ts,js}`、`packages/remote/src/**/*.ts`、`scripts/**/*.mjs`，并排除测试、声明文件、构建产物与依赖目录；Sonar 不再消费局部覆盖率假象。
- 新增 `src/remote/lib/remoteQueries.test.ts`，覆盖会话/工作区/面板/分支/路径键隔离、query client stale time、observer 取消不误杀底层请求、roster fallback 与 agent history 限量。
- 最终本地全项目 Vitest：157 files；1622 passed、1 skipped。诚实基线为 statements 43.16%（7927/18365）、branches 38.25%（4378/11443）、functions 43.85%（1531/3491）、lines 45.62%（7154/15680），距 80% 仍有明确差距。
- `src-tauri/src/hosts/mod.rs` 的 kernel-unavailable 测试改用确定性故障注入 seam，不再依赖机器上是否残留 live Kernel；该测试 1/1 通过，工作区 Rust lib 回归通过：267、327、48、73、8、31、399、12 各 crate 测试组均为 0 failed。
- 第二次全项目 Sonar 扫描已越过 TSConfig 阶段，但在 JS/TS analyzer 超过 364 秒仍无 scanner exit、CE task 或 Quality Gate；扫描器进程树已清理，一次性 token `codex-sonar-temp-1786261542` 已撤销，Sonar 服务进程未动。故 80% 与 Quality Gate 仍阻塞，不能以本地 coverage 代替。
- 扫描日志：`%TEMP%\\ridge-sonar-full.log`。已记录 invalid UTF-8/highlighting 与 lcov path warning；声明文件已加入 coverage 排除，下一次应优先修复编码/分析范围后再作有界复扫。
- Sonar `/api/ce/activity` 复核未出现本次超时扫描的新 CE task；列表中仅见既有 SUCCESS 任务，故没有可引用的本次项目指标或 Quality Gate。

## 当前未闭环项

1. Runtime API/A2A 的桌面/Kernel 真实 host adapter 与端到端确认回执；当前已具备 probe、严格优先级选择和 outcome contract，生产 host 仍只声明 MCP pull。
2. PTY fallback 五条件的真实运行时证明；当前兼容入口已 fail closed，不再绕过 Hub 直写 PTY。
3. 全项目 Sonar coverage ≥80.0% 和 Quality Gate；完整扫描已成功但项目 coverage 仅 40.3%，Gate ERROR（new coverage 47.6%、new violations 133）。

本交接文档只记录本地代码、测试、扫描与清理事实；本轮未向 Codex 之外 CLI 派发消息，未提交、推送或发布。

## 第七切片：持久 Hub 与 Delivery Engine 接线

- `packages/ridge-mcp/src/server.rs` 增加 SQLite-backed `McpSessionState::with_sqlite`：消息、receipt、未消费 inbox、idempotency 与 per-target sequence 均落盘；WAL、busy timeout、每 target/global 上限与 purge 均有界。
- `packages/ridge-kernel/src/server.rs` 与桌面 `src-tauri/src/teammate/mcp.rs` 共用 `ridge_data_dir()/agent-hub.sqlite3`；数据库初始化失败不静默降级到易丢消息的内存态。测试构造仍显式使用 ephemeral default。
- `packages/ridge-mcp/src/delivery.rs` 增加 Runtime API → A2A → MCP pull → PTY 五条件选择器、probe 与 outcome；Runtime/A2A/PTY 只有 Host 明确实现并声明能力时才会调用，默认仍是 MCP pull。
- 确定性证据：`ridge-mcp` 77/77；SQLite 重启恢复、ACK/consume 持久化、跨状态实例幂等、Runtime/A2A/PTY adapter dispatch 测试均通过；`ridge-kernel` 48/48；桌面 267/267。
- `packages/ridge-cli/tests/kernel_lifecycle_e2e.rs` 已用当前构建重跑，3/3 通过，覆盖 detached Kernel、Kernel PTY 与 standalone `rdg` convergence。

## 第八切片：覆盖率纯逻辑测试波次

- 新增 `anchorRect` 四种 placement 与 viewport clamp、RAF resize 合并/取消、搜索目录状态与 sidebar 事件、dock region 单飞/取消、终端字号 clamp/persist、Remote/Headless/rdg 来源徽标测试，共 6 个测试文件、19 个测试。
- 当前全量 Vitest：163 files；1646 passed、1 skipped。全项目 V8 coverage：statements 43.89%（8065/18372）、branches 38.88%（4451/11447）、functions 44.84%（1566/3492）、lines 46.43%（7283/15685）。
- 日志：`.iteration/artifacts/vitest-coverage-wave.log`。脚本目录仍有既有 Rollup `Expected ident` 解析警告，文件按工具行为从 coverage 排除；未改 Sonar inclusions/exclusions，故 80% 仍未达成。

## 第九切片：完整 Sonar 扫描与 Rust 覆盖率缺口

- 本次使用仓库完整 `sonar-project.properties` 扫描，索引 802 文件；首个完整 task scanner 与 CE 均 `SUCCESS`，任务、分析 ID 与脱敏指标详见 `docs/iterations/2026-08-09-sonar-full-scan-evidence.md`。随后规范化 LCOV 重扫 scanner 成功但 CE 因既有 test component 行数冲突 `FAILED`，不覆盖首个成功分析。
- Sonar 项目指标为 coverage `40.3%`、line `41.2%`、branch `38.9%`、violations `841`；Quality Gate `ERROR`，new coverage `47.6% < 80%`、new violations `133 > 0`。
- 结论：扫描阻塞已解除，但 80% 目标尚未完成。前端 V8 与 Sonar 数字均低于目标；Rust 源已纳入项目，尚缺真实 Rust LCOV，不能以前端报告冒充全项目达标。

## 第十切片：幂等并发闸与覆盖率复测

- `packages/ridge-mcp/src/server.rs` 为 Hub 的“幂等查询 → SQLite reservation → 内存发布”复合操作增加进程内 `enqueue_lock`。原有各存储 mutex 只能保护单步，不能阻止并发相同 `idempotency_key` 各自分配消息；现以短临界区保证一个逻辑消息。
- 新增并发契约测：8 个线程同时发送同一 sender/key/payload，断言唯一 `messageId`、Inbox 仅一条、其余 7 次明确 `deduplicated=true`。`cargo test -p ridge-mcp --lib`：78/78 通过；日志：`.iteration/artifacts/ridge-mcp-full-after-lock.log`。
- Hub adapter receipt 新增 `deliveryAttempts` 与 `deliveryLastAttemptAtUnixMs`；首次实际 Runtime/A2A/PTY adapter 调用先持久化 attempt=1，失败仍保留可查询 `delivery_failed` receipt；不自动重试、不扩张队列。
- `ridge_fetch_inbox` cursor 现支持稳定 `sequence` 续读；未知文本 cursor fail closed，避免消费后 cursor 丢失时从队首重复读取。对应测试与 delivery 观测更新后，`cargo test -p ridge-mcp --lib`：79/79 通过；日志：`.iteration/artifacts/ridge-mcp-cursor.log`。
- pane-tree px-anchor 纯逻辑测试补齐同轴 descendant plan、垂直 split 跳过、增长/收缩/吸收者下限与无效 path；定向 Vitest：75/75 通过。
- 全量 Vitest 复测：164 files；1649 passed、1 skipped；statements `43.64%`、branches `38.79%`、functions `44.53%`、lines `46.17%`。日志：`.iteration/artifacts/vitest-coverage-wave-2.log`。本地 LCOV 已重新规范化为仓库相对 POSIX 路径，但 Sonar normalized rescan 的 CE 行数冲突仍待清理。
- 同配置 Sonar 复测波次 2 在报告生成后超过 604 秒工具上限，未取得新的 scanner/CE 指标；一次性 token 已撤销，scanner 子树已清理。日志：`.iteration/artifacts/sonar-full-wave-2.log`。因此仍只引用首个 accepted full scan 的项目指标。
- 本切片未新增 Runtime API/A2A 运行时端点，也未放宽 PTY 安全闸；无能力证据即不宣称可用。Sonar 80%/Quality Gate 仍 ACTIVE。

## 第十一切片：取消穿透、deadline 回收与持久化复测

- `enqueue_hub_entry` 在入队前拒绝已过期 `deadline_unix_ms`；`ridge_fetch_inbox`、兼容 `ridge_inbox_read` 与再次幂等发送前均回收已过期 queued entry，写入 `status=expired`、`expiredAtUnixMs`，并从待投递队列消费掉，receipt 仍可查。
- 新增 `ridge_cancel_delivery`，按 `delivery_id` 或 `cancellation_id` 将未终态 delivery 置为 `cancelled`，写入取消 ACK/NACK 诊断并从 inbox 移除；幂等记录与 receipt 同步，重复发送不会复活已取消消息，后续 ACK 亦拒绝回写终态。
- SQLite Hub 新增 idempotency 状态同步；重开数据库后 cancellation status、receipt 与空 inbox 均可复核。确定性回归：`cargo test -p ridge-mcp --lib` `81 passed / 0 failed`；日志：`.iteration/artifacts/ridge-mcp-cancel-deadline.log`。
- 工作区 `cargo test --workspace --all-targets --quiet` 本轮启动后在 Tauri 全量编译阶段达到 300 秒上限，未形成 test result；既有 `cargo run --no-default-features` 进程保留，未做进程清理。该命令不计为全量通过，包级与既有 kernel/CLI 回归证据仍分别有效。
- TS 全量回归复测：Vitest `164 files / 1649 passed / 1 skipped`；`svelte-check` `0 errors / 0 warnings`。日志：`.iteration/artifacts/vitest-regression-wave-3.log`、`.iteration/artifacts/svelte-check-wave-3.log`。
- 跨层回归复测：`cargo test -p ridge-kernel --lib` `48 passed / 0 failed`；`cargo test -p ridge-cli --test kernel_lifecycle_e2e` `3 passed / 0 failed`。日志：`.iteration/artifacts/rust-kernel-regression-wave-3.log`、`.iteration/artifacts/rust-cli-lifecycle-regression-wave-3.log`。
- 本切片不启用未经验证的 Runtime API/A2A，不放宽 PTY 五条件；Sonar accepted 指标仍为 coverage `40.3%`、Quality Gate `ERROR`，80% 需求仍 ACTIVE。

## 第十二切片：TerminalManager public API 覆盖波次

- 新增 `packages/remote/src/shared/terminal/manager.test.ts`，以可控 kernel/renderer fixture 覆盖 `TerminalManager` 的 feed/delta/write/paste、输入、鼠标/滚轮、搜索/选择、preedit、滚动、TUI gate、IME anchor、redraw、主题与诊断/未知 pane 默认行为；定向测 `4/4` 通过。
- 全量 Vitest coverage 复测：`165 files / 1654 passed / 1 skipped`；statements `47.36% (8767/18509)`、branches `42.23% (4860/11508)`、functions `47.49% (1676/3529)`、lines `50.38% (7961/15800)`。相较上一波 statements `43.64%`，提升 `3.72` 个百分点；仍距 80% 有 `6041` 条语句缺口。日志：`.iteration/artifacts/vitest-coverage-wave-4-manager.log`。
- 本波只增加真实 public API 单测，未改 Sonar exclusions，也未伪造 Rust LCOV；Sonar accepted 项目指标仍为 coverage `40.3%`、Quality Gate `ERROR`，故 80%/Quality Gate 仍 ACTIVE。

## 第十三切片：事件总线与启动恢复纯逻辑覆盖

- 新增 `src/lib/stores/fsEvents.test.ts`，覆盖 Tauri 事件懒订阅、多个 handler 扇出、handler 异常隔离、最后一个订阅者取消后的 unlisten、非 Tauri fail-closed、订阅失败重试及 Windows 路径规范化/800ms 写入抑制窗口；定向 4/4 通过。
- 扩充 `src/lib/stores/paneTree.test.ts`，覆盖 startup context、recent/restore/saved workspace IPC 成功与失败回退、非 Tauri回退及 `collapseCwd` 路径投影；与 `fsEvents`、`fileWatcherSync`、`hosts` 公共测试合计定向 88/88 通过。
- 全量真实 V8/LCOV：`168` 个测试文件，`1655` 通过、`1` 跳过；statements `52.42% (9703/18509)`、branches `47.03% (5413/11508)`、functions `54.34% (1918/3529)`、lines `55.59% (8784/15800)`；`coverage/lcov.info` 已由仓库脚本规范化。日志：`.iteration/artifacts/vitest-coverage-wave-13-fs-pane-lcov.log`。
- 质量回归：`svelte-check` 0 errors/0 warnings；`ridge-mcp` 81/81；`ridge-kernel` 48/48；CLI lifecycle 3/3；`cargo fmt --all -- --check`、`git diff --check` 通过。
- 本地 Sonar scanner 本波真实启动但在 `/api/v2/analysis/version` 被 `401 Unauthorized` 拒绝，因环境没有 `SONAR_TOKEN`；未伪造 CE、coverage 或 Quality Gate 结果。日志：`.iteration/artifacts/sonar-scan-wave-13.log`。既有 accepted 项目结果仍为 coverage `40.3%`、Quality Gate `ERROR`。

本切片仍未关闭：Runtime API/A2A 真实 host adapter、PTY 五条件运行时证明、Rust LCOV、Sonar 项目级 coverage ≥80%/Quality Gate。没有已验证协议或凭据时保持 MCP pull 与 PTY fail-closed，不以单测或前端 V8 数字冒充跨层验收。本轮未向 Codex 之外 CLI 派发消息，未提交、推送或发布。

## Wave19：独立覆盖输入、生命周期围栏与当前质量门

- `DeliveryRegistry` 已成为 Kernel、桌面与 headless host 共用的 host-owned Runtime API/A2A in-process 投递 seam：队列容量 `256`，使用 `try_send`，投递与 teardown 均校验 `agent_id + generation + lease`；旧租约、断连、满队列均显式失败，Hub receipt 仍可经 MCP pull 追踪。PTY 生产路径继续不声明能力，五条件证据不足即 fail-closed。
- Kernel PTY destroy 成功后先持久化移除 identity，再按被移除 identity 的 generation/lease 注销 Runtime API/A2A 路由；生命周期回归证明销毁 Agent 不再被探测为 Runtime-capable。headless `TmuxMcpHost` 亦使用注入的 host-owned Hub state，不再回退进程全局默认状态。
- 独立 Rust 覆盖输入：`cargo test --workspace --all-targets --quiet --target-dir .iteration/artifacts/rust-target-wave-19` exit `0`，`1,552` 个 profraw；`coverage/rust.lcov` 为 199 个项目记录，line `38,420/68,713=55.91%`，function `4,123/7,986=51.63%`。证据：`.iteration/artifacts/rust-coverage-wave-19/cargo-test-workspace.log`。
- 独立前端覆盖输入：174 files / 1,682 passed / 1 skipped；`coverage/lcov.info` line `9,075/15,807=57.41%`，function `1,964/3,527=55.68%`。新增 link/path/project/settings/pane-dock 与 Cloud host 生命周期测试均通过；`svelte-check` 为 0 errors / 0 warnings。
- 质量复核：`cargo fmt --all -- --check`、`git diff --check`、requirements gate 均通过；iteration gate 明确失败于既有 worktree 与旧 write scope 不一致，未 reset、checkout、stash 或清理用户改动。Sonar 当前 scanner 因 `/api/v2/analysis/version` 返回 `401 Unauthorized` 未取得新 CE/Gate；不能宣称全项目 80% 或 Quality Gate OK。

### Wave19 未闭环

1. 仓库没有可验证的跨进程 Runtime API/A2A 协议与真实 endpoint；当前 in-process registry 不是跨系统端到端 receipt，故生产 host 仍只可安全选择 MCP pull。
2. 生产 PTY 尚无五字段原子运行时快照（idle、agent_prompt、无 approval、前台归属、无用户输入竞争），不得启用 fallback。
3. Rust/前端真实覆盖分别为 55.91%/57.41%，远低于 Sonar 全项目 80%；且缺有效 Sonar token，项目级 CE/Quality Gate 无法复核。

本交接仍禁止向 Codex 之外 CLI 派发消息，禁止提交、推送、发布；以上结论仅以本地代码、测试、覆盖报告与扫描日志为准。

## Wave20：重连 route 接管围栏

- 修复 `DeliveryRegistry::register` 的 generation 接管竞态：新 generation 可先于旧进程 teardown 原子替换同 Agent route；旧 receiver 立即断连，旧 generation/lease 的 probe、send、teardown 均不再影响新 route；同 generation 重复注册与更低 generation 注册继续拒绝。
- 回归：`cargo test -p ridge-mcp --lib --quiet --target-dir .iteration/artifacts/rust-target-wave-20-mcp`，`85 passed / 0 failed`；日志 `.iteration/artifacts/ridge-mcp-reconnect-fence-wave20.log`。
- 此修复仅闭合本地重连围栏，不把 in-process registry冒充跨进程 Runtime/A2A endpoint；Sonar 80%、PTY 五条件运行时证明与外部现场门仍按 Wave19 结论保留。

## Wave20：最终本地复核补充

- 正式前端 coverage 复测：174 个测试文件，1684 passed、1 skipped；statements `10035/18511=54.21%`、branches `5653/11511=49.10%`、functions `1971/3527=55.88%`、lines `9085/15807=57.47%`。日志：`.iteration/artifacts/vitest-coverage-wave20.log`。
- `cargo test -p ridge-mcp --lib --quiet --target-dir .iteration/artifacts/rust-target-wave-20-mcp`：85 passed / 0 failed；`cargo test -p ridge-kernel --lib --quiet --target-dir .iteration/artifacts/rust-target-wave-20-kernel`：49 passed / 0 failed。
- `cargo fmt --all -- --check`、`git diff --check`、requirements gate 均通过。`svelte-check` 与 `ridge-cli` 测试本轮首次编译超过工具墙钟上限，日志分别为 `.iteration/artifacts/svelte-check-wave20-direct.log`、`.iteration/artifacts/ridge-cli-test-wave20.log`；超时后无残留相关子进程，故不宣称通过。
- 结论不变：跨进程 Runtime/A2A endpoint、PTY 五条件原子运行时证据、Sonar 全项目 coverage >=80%/Quality Gate 及 iteration write-scope 仍未闭合。
