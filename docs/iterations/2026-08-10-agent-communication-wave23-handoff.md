# Ridge Agent 通信重构：Wave23 交接

日期：2026-08-10

## 来源与范围

- 主笔记：`Ridge 项目现状、愿景与规划基线（2026-07-21）`。
- 深化来源：Notebook `66919cb9-1329-4ddf-955c-f426d15a9fe6` 的 `Agent 通信架构重构`（source `9516749e-c317-4f13-9cda-b64b00cec465`）。
- 临时对话来源：Notebook `f6ffd900-708d-44ee-9818-1a3269c533fc` / source `df4d5dcc-9813-4c61-ae9f-1e9199cb7555`，已纳入约束审计；未向 Codex 之外 CLI 派发消息。
- 需求清单已获用户批准；历史 intake 的 `requirements_sha256` 曾与当前 `docs/REQUIREMENTS-SPEC.md` 不一致，已按批准需求重建 intake；当前 gate 为 `executable=true`、`pending_ids=[]`。运行态 intake 仅作证据，未纳入代码提交。

## 本波落地

Agent Hub、typed envelope、AgentIdentity/Topology、generation/lease fence、SQLite session state、MCP delivery tools、adapter probe/priority/outcome、桌面/Remote 投影与生命周期围栏已落地于既有实现；本波继续补齐可复核测试：

- 纯逻辑与 stores：i18n、插件注册、transport context、theme、remote readiness、member task、host ports、Svelte remote tree。
- 浏览器兼容层：Tauri bridge/core/dialog/event/window、PTY 原始字节事件、剪贴板回退。
- Cloud：共享工作区投影打开/关闭、授权失败、scope fence、host workspace 越界、metadata/panes 更新。
- Terminal：剪贴板图片、theme bridge、TOTP transcript/plaintext/timeout。

## 验证证据

| 检查 | 结果 | 日志 |
|---|---:|---|
| 新增纯逻辑测试 | 19/19 | `.iteration/artifacts/pure-logic-coverage-wave21.log` |
| 兼容层/树/剪贴板/theme bridge | 17/17 | `.iteration/artifacts/pure-logic-coverage-wave22.log` |
| Shared workspace projection | 10/10 | `.iteration/artifacts/shared-projection-test.log` |
| Cloud controller TOTP | 5/5 | `.iteration/artifacts/cloud-controller-test.log` |
| 全量 Vitest + V8/LCOV | 185 files；1727 passed；1 skipped | `.iteration/artifacts/vitest-coverage-wave23.log` |
| 前端 statements | `10692/18511 = 57.76%` | `coverage/lcov.info` |
| 前端 branches/functions/lines | `51.22% / 60.16% / 61.27%` | `coverage/lcov.info` |
| 直接 `svelte-check` | 0 errors / 0 warnings | `.iteration/artifacts/svelte-check-wave20-direct.log` |
| 格式/空白 | `cargo fmt --all -- --check`、`git diff --check` 通过 | 命令结果 |

Rust 独立输入仍以 Wave19 为准：workspace all-targets exit `0`，line `55.91%`、function `51.63%`；本次 Wave21 workspace 全量编译在工具时限内未形成 `test result`，不得算作通过。

## 提交

- `d67247ef fix remote iteration quality gates`：Agent Hub、delivery、lifecycle 及测试/配置主体。
- `bb03b9ba docs: correct timeout evidence`：修正交接文档中的超时证据表述。
- `2fd43027 test: expand pure frontend coverage`：首批纯逻辑覆盖测试。
- `91f17ec4 test: cover remote compatibility and cloud flows`：本波兼容层、Cloud、Terminal 覆盖测试。

## 未闭环与下一步

1. Sonar 全项目 coverage ≥80% 与 Quality Gate 尚未完成：最新可引用的 Sonar accepted 项目指标仍为 coverage `40.3%`、line `41.2%`、branch `38.9%`、Gate `ERROR`；本机当前无有效 `SONAR_TOKEN`，不得伪造扫描结果。
2. 跨进程 Runtime API/A2A 真实 endpoint、协议与 receipt 仍无可验证证据；当前生产默认保持 MCP pull。
3. PTY fallback 五条件（idle、agent_prompt、无 pending approval、目标前台 Agent、无用户输入竞争）尚无生产原子运行时证明；继续 fail-closed。
4. full workspace regression 需在无冲突的独立构建窗口重跑；不得杀掉既有 `cargo run --no-default-features`，不得清理用户运行态产物。

交接原则：上述未闭环项保持 ACTIVE；本波已提交代码与测试，但不宣称“全部验收完成”或“Sonar 80% 已达标”。

## Wave24 收口复核

- `src/remote/lib/remoteQueries.test.ts` 新增 query 去重、abort、timeout、错误传播与 workspace target 测试，10/10 通过；提交 `1ad63c4a`。
- 全量 `pnpm test:coverage:sonar`：185 files；1727 passed；1 skipped；statements `10758/18511 = 58.11%`、branches `51.37%`、functions `60.92%`、lines `61.59%`。日志：`.iteration/artifacts/vitest-coverage-wave24.log`。
- 覆盖率较 Wave23 再升 statements `+0.35` 个百分点；距离 80% 仍有 `7753` 条语句未覆盖，Sonar 项目级 80%/Gate 仍不可宣称完成。

## Wave26 终端队列修复与复核

- 修复 `packages/remote/src/shared/terminal/manager.ts` 延迟 feed 排空的真实死循环：排空队列时不再把刚取出的 chunk 重新视作待排空队列；有界 drain 分片时，余量置于后续 chunk 之前，保证字节顺序不乱。
- 新增并通过 manager 生命周期、内联 TUI feed/reply/event、ESC 大包、renderer park/unpark、延迟队列顺序测试：9/9，日志 `.iteration/artifacts/manager-lifecycle-wave25.log`。
- 全量 `pnpm test:coverage:sonar`：185 files；1734 passed；1 skipped；statements `10931/18517 = 59.03%`、branches `52.32%`、functions `61.01%`、lines `62.62%`。日志 `.iteration/artifacts/vitest-coverage-wave26.log`。
- 本波提交：`01f25db9 fix: drain deferred terminal feeds safely`、`cb093503 fix: preserve deferred terminal feed order`。
- 当前需求 gate 已按最新 `docs/REQUIREMENTS-SPEC.md` 重建并通过：`executable=true`、`pending_ids=[]`；运行态 intake 文件仍不纳入提交。

Wave26 后 Sonar 全项目 ≥80%、跨进程 Runtime/A2A receipt、PTY 五条件生产运行证据及 Rust workspace 全量回归仍为 ACTIVE；覆盖率数字不得等同于 Sonar Quality Gate 通过。

## Wave28 质量增量

- manager 新增选择、输入、滚动、清空、鼠标与安全默认值边界测，10/10；Cloud controller 新增 trust grant 合法/非法/超时测，7/7。
- 全量 `pnpm test:coverage:sonar`：187 files；1750 passed；1 skipped；statements `11130/18527 = 60.07%`、branches `53.63%`、functions `61.61%`、lines `63.59%`。日志 `.iteration/artifacts/vitest-coverage-wave28.log`。
- 当前仍有 coverage provider 对部分 `.mjs` 脚本的 parse warning；这些脚本未凭空计入覆盖率，需另行修复/验证，不能视作质量门通过。
- 本波提交：`988761a0 test: cover terminal manager edge projections`、`7cf52e2e test: cover cloud controller trust handshake`。

## Wave29 质量增量

- `TerminalManager` 新增 Host link/open-plan、共享壁纸 Host、静态边界投影测；`paneTree` 新增保存元数据及 saved-workspace 生命周期命令测；`hosts` 新增远端 attach/detach 生命周期测，明确 detach 仅摘除本地视图、不终止远端 PTY。新增 3 个测试，全量通过。
- 全量 `pnpm test:coverage:sonar`：187 files；1753 passed；1 skipped；statements `11194/18527 = 60.41%`、branches `6221/11534 = 53.93%`、functions `2180/3527 = 61.80%`、lines `10122/15822 = 63.97%`。日志：`.iteration/artifacts/vitest-coverage-wave29.log`。
- 相比 Wave28：statements `+0.34` 个百分点、branches `+0.30`、functions `+0.19`、lines `+0.38`；距 Sonar 全项目 80% 仍有 `7333` 条语句未覆盖。
- 覆盖率 provider 仍对部分 `.mjs` 脚本报 `PARSE_ERROR` 并排除；`node --check` 可通过不等于覆盖率可计入，Sonar 真实扫描/Gate 仍未完成。

## Wave30 远程传输护栏

- `packages/remote/src/shared/transport/wsRemoteRpcScheduler.test.ts` 新增 3 条确定性测试：鉴权拒绝进入终态且不重连；传输断开后 pane RPC 保留、失败计数/退避重试并重连；心跳无 pong 时主动断开半开连接。聚焦回归 `21/21` 通过，日志：`.iteration/artifacts/wsremote-wave30.log`。
- 全量 `pnpm test:coverage:sonar`：`187` 个测试文件，`1756 passed / 1 skipped`；日志：`.iteration/artifacts/vitest-coverage-wave30.log`。
- 当前覆盖率：statements `11265/18527 = 60.80%`，branches `6255/11534 = 54.23%`，functions `2195/3527 = 62.23%`，lines `10179/15822 = 64.33%`。相较 Wave29：`+0.39 / +0.30 / +0.43 / +0.36` 个百分点；距 80% 仍缺 `7262` 条 statements。
- `wsRemote.ts` 提升至 statements `78.64%`、branches `63.95%`、functions `77.62%`、lines `85.92%`。覆盖报告仍对部分 `.mjs` 报 `PARSE_ERROR` 并排除；故仅记本地证据，不宣称 Sonar Quality Gate 通过。

## Wave31 PTY 元数据回流修复

- 定位并修复 `packages/ridge-cli/src/kernel_host_impl.rs` 的标题选择缺口：此前按 OSC 0/1/2 类型固定优先，旧 OSC 0 会遮蔽同一缓冲区中较新的 OSC 2；现统一交给 `ridge-core::pty::title::parse_title_from_output`，按字节位置取最后一个完整标题。
- `packages/ridge-core/src/pty/title.rs` 改为跨 OSC 类型按流位置解析，并保留未闭合序列的容错；新增逆序覆盖测试。
- 证据：`cargo test -p ridge-core --lib pty::title` 为 `6 passed`；`cargo test -p ridge-cli pty_metadata_frame -- --nocapture` 为 `2 passed`。物理/WebView2 稳定重跑尚未完成，故 BUG-PTY-OSC2-TITLE-01 保持 partial，不宣称现场闭环。

## Wave32 DPR 冷启动判定护栏

- `src/routes/+page.svelte` 在既有 `ridge:app-ready` 边界写入持久 `window.__ridgeAppReady` 标记；保留原事件语义，供晚到的 CDP 探针读取。
- `scripts/cdp-dpr-e2e.mjs` 拆分 app-ready 与 renderer 两段有界等待，分别记录 readiness timeout 与 backing-canvas timeout；默认各 `60s`，可由 `RIDGE_DPR_APP_READY_TIMEOUT_MS`、`RIDGE_DPR_RENDERER_TIMEOUT_MS` 调整。
- 本波只改变失败分类与探针观测，不把无 canvas 判为通过；真实 WebView2 `dpr=2` 冷启动复跑仍未执行，BUG-E2E-DPR-STARTUP-RACE-02 保持 partial。
- 受控复跑：读取现存 CDP 端口 `2393` 后，`/json/list` 无 Ridge page，探针在找页阶段 exit `1`（日志：`.iteration/artifacts/cdp-dpr-wave32.log`），未生成截图；这是 stale/no-page 运行态阻塞，不作为 DPR 通过或产品失败证据。

## Wave33 覆盖率与 Sonar 环境复核

- 重跑 `pnpm test:coverage:sonar` 成功：`187` 个测试文件，`1756 passed / 1 skipped`；日志输出含既有 `.mjs` `PARSE_ERROR`，这些脚本由 coverage provider 排除，不能等同 Sonar 扫描通过。
- 当前本地 V8/LCOV：statements `11265/18537 = 60.77%`、branches `6255/11542 = 54.19%`、functions `2195/3527 = 62.23%`、lines `10179/15831 = 64.29%`；距 80% 尚缺 `7272` 条 statements。该数字仅作补测基线，不替代 Sonar project metric。
- 本机复核：`SONAR_TOKEN_PRESENT=False`、`SONAR_HOST_URL_PRESENT=False`，未发现 `sonar-scanner` 或 `sonar` 命令；故本波无法执行真实 Sonar scan/CE/Quality Gate，也不宣称 `REQ-SONAR-COVERAGE-80-01` 完成。
- 本波工作树另有既存运行态/coverage 产物及 `sonar-project.properties`、`tsconfig.json` 改动；未将其混入本波提交，按保留 dirty worktree 规则交还。

## Wave34 跨进程 Agent delivery bridge

- `packages/ridge-mcp` 新增共享 `GET /api/v1/agent-events/ws` bridge；桌面、Kernel、tmux 复用 `mcp_router`，不新增第二份 Hub/identity state。
- 注册必须先通过 host token 与当前 roster 的 `agent_id/generation/lease/online/lifecycle` 校验；route 使用现有 `DELIVERY_ROUTE_CAP=256`，桥接转发另有 1 条待发槽；满载、旧 generation、旧 lease、断连均 fail-closed，MCP pull 仍可恢复。
- Hub envelope 原样经 WebSocket 投递；Agent 回 `type=ack` 后由 durable receipt 更新 `agentAcknowledged/ack`，ACK 同样校验 receipt 的目标身份 fence；不记录 token/cookie。
- 隔离 `target\\codex-delivery-test` 实测 `cargo test -p ridge-mcp --features axum-transport --lib`：`89 passed / 0 failed`，含实际 TCP/WebSocket 鉴权、注册、Hub `ridge_send_message`、投递、ACK、断连注销 E2E。共享默认 target 曾被外部运行态清理，未触碰其进程；隔离 target 通过。
- 该 bridge 是 Ridge-owned 等价 Runtime/A2A adapter，不宣称兼容第三方 CLI 私有协议；第三方协议验证、PTY 五条件原子运行时证据、Sonar 80%/Gate、物理现场项仍 ACTIVE。

## Wave35 覆盖率补测与最终质量门

- `src/lib/stores/paneTree.test.ts` 新增 3 组确定性测试：无 DOM/空叶节点退化、splitter 几何与同轴吸附、junction 去重/定时器/拖拽更新/释放清理；聚焦文件 `81/81` 通过，未改 coverage include/exclude。
- 全量 `pnpm test:coverage:sonar` 通过：`187` 个测试文件，`1759 passed / 1 skipped`；`svelte-check` 仍为 `0 errors / 0 warnings`（前序证据），`cargo fmt --all -- --check` 通过。
- 最新本地 V8/LCOV：statements `11495/18537 = 62.01%`、branches `6373/11542 = 55.21%`、functions `2233/3527 = 63.31%`、lines `10386/15831 = 65.60%`；相对 Wave33 statements 增加 `230` 条、`+1.24` 个百分点；达到 80% 尚缺 `3335` 条已覆盖 statements（当前未覆盖 `7042` 条）。
- `.mjs` coverage provider `PARSE_ERROR` 排除告警仍存在；本机仍无 `SONAR_TOKEN`/`SONAR_HOST_URL` 与 scanner，故没有伪造 Sonar project metric 或 Quality Gate 结论。`REQ-SONAR-COVERAGE-80-01` 继续作为下一迭代 ACTIVE 目标。

## Wave36 Cloud Remote 状态机补测

- `src/remote/lib/cloudRemote.test.ts` 新增 7 个确定性测试，覆盖：初始化可选探针失败仍保持可用、`pane-meta-changed` payload 校验、resync/tail 双种子失败后的 live 保活、PTY listener 成功但 host 注册失败后的有界重试、resume 跳过破坏性 seed、历史页 malformed/at-oldest，以及历史 RPC 拒绝后的恢复。
- 聚焦回归：`cloudRemote.test.ts` `47/47` 通过。
- 全量 `pnpm test:coverage:sonar` 通过：`187` 个测试文件，`1766 passed / 1 skipped`；本地 V8/LCOV statements `11507/18537 = 62.07%`、branches `6391/11542 = 55.37%`、functions `2232/3527 = 63.28%`、lines `10398/15831 = 65.68%`。
- 相较 Wave35，covered statements 增加 `12`；达到 80% 仍缺 `3323` 条 covered statements。`.mjs` `PARSE_ERROR`、本机缺 `SONAR_TOKEN`/`SONAR_HOST_URL` 与真实 CE/Gate 证据仍属阻塞，故 `REQ-SONAR-COVERAGE-80-01` 保持 ACTIVE。
- 质量门：`pnpm check` 0 errors / 0 warnings；`cargo fmt --all -- --check` 通过；`git diff --check` 通过。生成的 `coverage/*` 与 `.iteration/*` 运行态变更不纳入本次提交。

## Wave37 Host/Terminal 覆盖补测

- `src/lib/stores/hostsPublic.test.ts` 新增共享工作区投影与远端 mutation 失败闭闸测试；聚焦 `10/10` 通过。
- `packages/remote/src/shared/terminal/manager.test.ts` 新增 shared-host resize 边界、workspace invalidate、shell snapshot 异常与 TUI cursor anchor 测试；聚焦 `13/13` 通过。
- 全量 `pnpm test:coverage:sonar` 通过；相较 Wave36 新增 `4` 个测试。最新本地 V8/LCOV：statements `11603/18537 = 62.59%`、branches `6453/11542 = 55.91%`、functions `2247/3527 = 63.71%`、lines `10471/15831 = 66.14%`；距 80% 尚缺 `3227` 条 covered statements。
- `.mjs` `PARSE_ERROR`、本机缺 `SONAR_TOKEN`/`SONAR_HOST_URL` 与真实 Sonar CE/Gate 仍阻塞正式闭环；`REQ-SONAR-COVERAGE-80-01` 保持 ACTIVE。PTY 五条件仍 fail-closed，因宿主尚无完整可验证运行时快照，未擅自放行。
- 质量门：`pnpm check`、`cargo fmt --all -- --check`、`git diff --check` 应复核通过；`coverage/*` 与 `.iteration/*` 运行态变更不纳入提交。

## Wave38 Agent 通信架构：PTY proof fencing

- `packages/ridge-mcp/src/delivery.rs` 新增 host-owned PTY safety proof 注册表；proof 绑定 `agent_id/generation/lease`，同代换 lease、旧代注册、旧代注销均拒绝，同代刷新原子替换。
- proof 新增 `PTY_SAFETY_MAX_AGE=3s` 新鲜度闸；过期或 identity 不匹配即清空 PTY probe，选择器不能走 PTY，MCP pull 保留。
- `McpSessionState` 提供注册/注销入口；Kernel PTY destroy 的 identity teardown 同时撤销 Runtime API、A2A 与 PTY proof，避免旧代 proof 残留。
- 证据：`cargo test --target-dir target/codex-delivery-test -p ridge-mcp --features axum-transport --lib` 为 `92 passed / 0 failed`；`cargo test --target-dir target/codex-kernel-agent-comm -p ridge-kernel --lib` 为 `49 passed / 0 failed`；`cargo fmt --all -- --check` 通过。
- 本波仅补齐 proof 存储、刷新、过期与销毁 fencing 基础设施；宿主五条件尚无完整原子运行时快照，故 PTY 生产放行继续 fail-closed。Sonar 仍为本地 `62.59%`，无 scanner/token/host，不能宣称 80% 或 Quality Gate 通过。

## Wave40 交接：build-ridge 可测化与质量门复核

- 代码：`scripts/build-ridge.mjs` 新增 ESM 入口保护和纯逻辑导出；直接执行仍走原构建流程，导入测试不会触发真实 Tauri 构建。
- 测试：新增 `scripts/build-ridge.test.mjs`，覆盖参数/版本/GUID、Cargo/WiX/Tauri 配置、tauri 子进程成功与失败、双前端构建调用、临时目录清理及失败恢复；定向 `7/7` 通过。
- 质量：全量 `pnpm test:coverage:sonar` exit `0`；V8/LCOV 为 statements `11988/18538=64.66%`、branches `6629/11546=57.41%`、functions `2310/3527=65.49%`、lines `10830/15832=68.40%`。距本地 statements 80% 尚差 `2843` 条；该数值不等价于 Sonar 项目指标。
- 复核：`pnpm check` `0 errors / 0 warnings`；`cargo fmt --all -- --check`、`node --check scripts/build-ridge.mjs`、`git diff --check` 通过。
- 未闭环：部分既有 `.mjs` 仍有 V8 remap `PARSE_ERROR/Expected ident`；本机 SonarQube `26.7.0.124771` 与 scanner 已安装且 API `UP`，但项目 coverage `56.7%`、Quality Gate `ERROR`，故 `REQ-SONAR-COVERAGE-80-01` 仍为 `ACTIVE`，不得宣称 Sonar 80% 或 Quality Gate 完成。PTY 五条件生产原子快照、第三方 Runtime/A2A 真实兼容性及物理设备门仍按前波交接。
- 工作区纪律：未向 Codex 外 CLI/agent 派发消息；未 push、tag、release；仅提交本波脚本/测试/交接文档，coverage 和 `.iteration` 运行态变更保留在工作区。

## Wave39 覆盖率与通信边界回归

- `packages/remote/src/shared/terminal/manager.attach.test.ts` 新增隔离 wasm/DOM 生命周期测试：未 ready 拒绝、Canvas/scrollback/theme 注入、重复 attach fencing、focus in/out、TUI mouse press/release、double/triple click、ResizeObserver 与 detach 清理；聚焦 `2/2` 通过。既有 `manager.test.ts` 与 `controllerIdentity.test.ts` 合计 `25/25` 通过。
- `src/remote/lib/cloudRemote.test.ts` 新增 3 组边界测试：空 pane/零尺寸输入 fail-closed、Agent/shell/HITL/health/workspace parity 命令、空 workspace 创建与 close 失败可重试；全文件 `50/50` 通过。
- 全量 `pnpm test:coverage:sonar`：`188` 个测试文件，`1779 passed / 1 skipped`；本地 V8/LCOV statements `11891/18537 = 64.14%`、branches `6599/11542 = 57.17%`、functions `2298/3527 = 65.15%`、lines `10739/15831 = 67.83%`。相较 Wave38 covered statements 增加 `288`，距本地 80% 尚缺 `2939` 条；该 LCOV 仍不替代 Sonar project metric。
- `node --check scripts/*.mjs` 全部通过，但 V8 remap 仍对部分 `.mjs` 报 `PARSE_ERROR/Expected ident` 并排除；本机仍无 `SONAR_TOKEN`/`SONAR_HOST_URL`、scanner 与 Quality Gate 证据，故 `REQ-SONAR-COVERAGE-80-01` 保持 ACTIVE，不宣称 80%/Gate 完成。
- 质量证据：全量 coverage 命令退出 `0`；`cargo fmt --all -- --check`、`git diff --check` 应随提交前复核；`pnpm check` 本波未重跑（既有 Node/dev 进程竞争曾导致超时）。coverage 与 `.iteration` 运行态产物不纳入提交。

## Wave41 Remote 通信契约覆盖与 Sonar 差距复核

- `manager.attach.test.ts` 新增开发诊断面真实调用，覆盖 PTY feed/write、可见网格、主题/光标探针、selection/delta、PTY write spy、worker 状态与 detach；聚焦 `3/3` 通过。
- 新增 `wsRemote.behavior.test.ts`，覆盖 LAN 消息/二进制 PTY 路由、capability/theme/meta/resize、Agent typed envelope、HITL、history/resume、workspace/shell、saved workspace 与断开取消；与既有 scheduler/attach 回归合计 `27/27` 通过。
- 全量 `pnpm test:coverage:sonar` exit `0`；本地 V8/LCOV statements `12307/18538 = 66.38%`、branches `6772/11546 = 58.65%`、functions `2399/3527 = 68.01%`、lines `11118/15832 = 70.22%`。statements 目标 `14831`，尚缺 `2524` 条；既有 `.mjs` `PARSE_ERROR/Expected ident` 仍有记录。
- `pnpm check` `0 errors / 0 warnings`。Sonar monitor 仍为 coverage `56.7%`、Quality Gate `ERROR`；本波无新 scanner/CE 成功证据，`REQ-SONAR-COVERAGE-80-01` 继续 ACTIVE。
- 未闭环：PTY 五条件原子运行时快照、第三方 Runtime/A2A 真实兼容性、Sonar `>=80%`/Gate；coverage 与 `.iteration` 运行态产物不纳入提交。未向 Codex 外 CLI/agent 派发消息，未 push/tag/release。
