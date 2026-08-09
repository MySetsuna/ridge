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
