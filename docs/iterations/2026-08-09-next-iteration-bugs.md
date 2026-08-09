# 下一迭代 Bug 候选（2026-08-09）

本文件只登记本轮 E2E/质量流程暴露的下一迭代候选，不将环境异常直接宣称为产品根因；待下一轮按 CodeGraph、确定性测与现场日志复核。

## BUG-E2E-DPR-ZERO-CANVAS-01

Status: closed. DPR E2E now requires both canvas and backing buffer; latest evidence: `canvasCount=2`, `backingCanvasCount=2`.

- 现场：`scripts/cdp-dpr-e2e.mjs` 返回 `ok=true`，但本轮 `canvasCount=0`、`backingCanvasCount=0`；此前同一脚本曾观测到两者均为 `2`。
- 风险：DPR E2E 目前可能把“页面未挂载 terminal canvas”误判为通过。
- 下一轮最小验收：canvas 数量必须 `> 0`，且至少一个 backing width/height 与 DPR、CSS rect 相符；否则失败并保存页面 URL/console/截图。

## BUG-REMOTE-HEADLESS-TEAM-FALLBACK-01

Status: closed. Kernel host advertises `pane/fs/search/workspace`; RemoteConnection breaks unsupported teammate methods and hides Team plus background polling.

- 现场：移动 Remote E2E 报 `no-team-tab`；日志同时显示 headless host 不支持 teammate 方法。
- 已知事实：NLM 来源记录 headless `rdg` 刻意不宣告 `teammate` 能力。
- 未证实假设：Remote UI 在能力缺失时直接隐藏 Team 入口，缺少明确 disabled/unsupported fallback，导致用户难以区分“无能力”与“加载失败”。
- 下一轮最小落点：能力矩阵、Remote sidebar fallback、headless capability contract test；不得为掩盖能力缺失而伪造 roster。

## BUG-REMOTE-SHELL-PILL-KERNEL-DRIFT-01

Status: closed. Shell discovery uses `SHELL_DISCOVERY_TIMEOUT_MS=30_000`; real LAN mobile E2E restored the shell pill, listed 9 shells, switched Git Bash, and passed `GATE: PASS`.

- 现场：移动 Remote E2E 报 `no-shell-pill`；页面日志有 `get_scm_status failed ... os error 10060`。本轮检查到 isolated kernel registry 已更新至 port `2286`，而旧桌面连接仍尝试 `4671`。
- 未证实假设：dev:cdp/rdg/Remote provider 之间没有在 kernel 重启后统一刷新 endpoint，导致 pane snapshot 未回流，shell pill 与 pane 行一起消失。
- 下一轮最小验收：杀/重启 kernel 后，桌面、rdg、mobile controller 均重新发现同一 PID/port；workspace/pane/shell metadata 在 15 秒内恢复；旧 endpoint 不得继续请求。

## BUG-SONAR-SCANNER-LOCAL-TIMEOUT-01

Status: closed as scanner-host timeout; a bounded rerun later succeeded. Coverage target remains open.

- 现场：首次复扫超时；随后使用归一化 LCOV、`sonar.scm.disabled=true`、跳过 JRE provisioning 的全项目扫描成功，分析 `09735c32-b9e6-4fc8-9054-3891f4f44795`，耗时 `43737ms`。
- 当前差距：project coverage `40.3%`，Quality Gate `ERROR`（`new_coverage=47.6%`、`new_violations=133`）；REQ-SONAR-COVERAGE-80-01 仍 open。
- 下一轮最小验收：补真实未覆盖代码测试波次，并在 SCM enabled 的正式扫描中将 new violations 清零后再宣称 Quality Gate OK。

## BUG-DEVELOPER-CDP-KERNEL-BREAKAWAY-01

Status: closed for the constrained dev host. `tauri-dev-cdp.mjs` now opts into the explicit dev-only non-breakaway fallback; production remains fail-closed.

- 证据：同一宿主再次达到 kernel PID `58404` health、CDP `3053`，smoke/DPR/pane/multitab/mobile E2E 均可运行；安装态 Ridge 未被清理。
- 约束：不得把 `RIDGE_TEST_ALLOW_NON_BREAKAWAY=1` 带入生产启动路径，不得移除进程树回收护栏。

## BUG-E2E-CROSS-VOLUME-ACL-MATRIX-01

Status: partial. Physical ACL-denied rejection and the local partial-result DTO are now covered; injected copy-then-delete failure remains open.

- 已闭环：`scripts/cdp-cross-volume-acl-e2e.mjs` 在 `C:` → `D:` 现场拒绝 `read_file`/`move_path`，恢复 ACL 并确认源文件保留；`explorerPaste.test.ts` 覆盖 success/partial/all-failed DTO。
- 仍待：注入 copy 成功、source delete 失败的真实跨卷场景，确认目标/源双态与 UI DTO 一致。

## Latest field evidence

- 真实移动 LAN E2E：headless host 正确隐藏 Team；shell picker 返回 9 项终端；Git Bash 切换后 `shell_kind` 为 `C:\\WINDOWS\\system32\\bash.exe`；`GATE: PASS`。
- 自签名证书仍导致 Service Worker 注册失败；已记为环境限制，不判为 Remote 数据面失败。
- 重建 `remote-dist/mobile` 后，unsupported teammate 轮询仅留初始探针批次；旧 host 仍由 runtime breaker 收束。

## NLM 候选（只读来源）

- 当前活跃对话：Notebook `66919cb9-1329-4ddf-955c-f426d15a9fe6`，conversation `a47d3199-c1f9-47f1-927c-ff2c4875b77d`，10 轮。
- 新查询给出的方向：headless teammate 能力回退、复合身份下 shell 状态投影、深根模式 kernel timeout、Message Hub/PTY adapter 分层；均须回到本地代码与运行证据后再立项。
- 本轮 NLM 仅读；未写入、删除或上传 source/note。
- 最新查询已排除本轮通过的 headless/shell/workspace hydration；下一候选为：ridge-term 非整数 DPR 视觉对照、Codex 流式渲染帧单调性、移动 IME 锚点/滚底顺序、双窗口 workspace claim 竞态、Explorer 部分失败现场矩阵、无 Tauri 的 ridge-mcp kernel API。NLM 仅给候选，不替代本地代码事实。

## NLM 候选核验

- 无 Tauri 的 ridge-mcp kernel API 已闭环：`packages/ridge-cli/tests/kernel_lifecycle_e2e.rs` 现断言 `initialize`、`tools/list`、`ridge_delegate_task`、inbox 保留及停核后 MCP 拒绝；该 E2E `3 passed`。
- 收口复核曾发现 `packages/ridge-kernel/src/server.rs` 对 `Result<_, String>` 误用 `anyhow::Context`，已改为显式 `map_err`；Rust 工作区 lib 与 kernel lifecycle E2E 复跑通过，故不再列为 open bug。
- 故本候选未发现产品代码缺口，不另造修复；下一轮仍按本文件上列候选取证。

## NLM 最终查询（只读候选）

最新查询再次排除 headless capability fallback、Hydration 与无 Tauri MCP 生命周期；保留以下待证项：

- P0：ridge-term 非整数 DPR 原生 PowerShell 对照矩阵；Codex 高频流式输出逐帧 `Frame ID` 单调性/旧帧复活。
- P0（外部证据待补）：移动端键盘锚定与 visualViewport 顺序；双窗口 Workspace claim/RPC 竞态。
- P1：Explorer 跨卷部分失败与权限拒绝下的物理盘—FileTree DTO 一致性矩阵。
- 环境归因待证：`runtime.lastError` 需 clean-profile/扩展 A/B；NLM 明确仓库源码未发现 Chrome Extension Messaging，不得先改业务代码。

## NLM query after full E2E (read-only)

Notebook: `66919cb9-1329-4ddf-955c-f426d15a9fe6`; conversation: `a47d3199-c1f9-47f1-927c-ff2c4875b77d`.
Query sources: `be660734-15ce-4e2e-8843-5430302c3a29`, `15441f90-cb8e-4cbe-b644-80ac68984653`.
No NotebookLM source/note was created, updated, deleted, or uploaded.

1. `REQ-TERMINAL-RASTER-01` P0 — non-integer DPR/native PowerShell box-drawing pixel matrix. Physical Windows scaling required; local DPR=1 evidence is insufficient.
2. `REQ-CODEX-RENDER-STABILITY-01` P0 — Codex high-rate replay trace, monotonic frame generation, no revived rows/multiple cursors. Locally implementable with a recording fixture.
3. Mobile Remote P0 — long background/sleep/network loss recovery without token eviction or re-auth. Requires physical PWA evidence; local reconnection tests can prepare the contract.
4. `REQ-REMOTE-SMOOTH-STATE-02` P0 — mobile keyboard anchor and `visualViewport` ordering. Requires physical device; unit/E2E can assert `scrollToBottom -> cursor/fallback -> focus` ordering.
5. Workspace P0 — dual desktop-window workspace claim/RPC race under high-frequency output. Locally implementable with two-window/process simulation and stale-RPC assertions.
6. `REQ-EXPLORER-FILE-CONTINUITY-01` P1 — partial success and ACL-denied cross-volume move diagnostics. Local deterministic DTO tests plus Windows ACL fixture; physical permission evidence remains open.

NLM remains hypothesis-only. Current Sonar `40.3%` and ACL evidence gap remain open requirements, not NLM-closed items.

## Latest physical acceptance rerun

- `scripts/cdp-dpr-e2e.mjs` under `RIDGE_CDP_DEVICE_SCALE_FACTOR=1.25`: `dpr=1.25`, `innerWidth=2752`, `innerHeight=1114`, `canvasCount=1`, `backingCanvasCount=1`; non-integer DPR evidence now exists for the WebView2 dev harness.
- `scripts/cdp-cross-volume-acl-e2e.mjs`: current Windows account ACL-denied `read_file` and cross-volume `move_path` both rejected with `os error 5`; ACL restored, source content preserved. Full partial-result DTO matrix remains open.
- `packages/remote/src/shared/terminal/terminalFeedPolicy.test.ts`: 3 deterministic tests pin deferred-frame order, stream-cut cleanup, and threshold branches; old queued render bytes cannot revive after `dropPendingFeedBuffers`.

## Final dev:cdp rerun after Explorer DTO

- WebView2 `RIDGE_CDP_DEVICE_SCALE_FACTOR=1.25`：DPR `1.25`，canvas `3/3` 有 backing buffer。
- pane graph：叶子 `1 → 2 → 1`，split/close broadcasts 均通过；multitab：workspace `1 → 2 → 3`，worst long task `176ms`，`GATE: PASS`。
- mobile Remote：capabilities `pane/fs/search/workspace`，Team 正确隐藏，shell picker `9` 项，Git Bash `shell_kind=C:\WINDOWS\system32\bash.exe`，`GATE: PASS`；Service Worker 自签名证书失败仍为环境限制。
- ACL：`read_file` 与跨卷 `move_path` 均以 Windows `os error 5` 拒绝，ACL 恢复且源内容保留；跨卷往返 `27` bytes 通过。
- Explorer DTO：`src/lib/components/explorerPaste.test.ts` 的 success/partial/all-failed 分支 `3/3` 通过；实际 copy 成功后 source delete 失败的物理注入仍未完成。

## BUG-E2E-DPR-STARTUP-RACE-01

Status: open, harness-only. 首次 DPR probe 在 renderer 首轮挂载前由外层 `60s` 超时；页面随后正常挂载，重试得到 DPR `1.25` 与 `3/3` backing canvas。下一轮应统一 probe 内部等待上限与外层命令超时，并记录“未挂载”与“挂载后失败”两类结果。

## BUG-CLOUD-LOCAL-SEED-DB-MISMATCH-01

Status: closed as test-fixture/configuration bug. `scripts/cdp-cloud-seed.mjs` defaulted to `ridge_cloud`, while the running local `ridge-cloud :5050` instance used `ridge_cloud_e2e`; premium promotion landed in the wrong database and the controller returned `NOT_PREMIUM`. The final run supplied `RIDGE_CLOUD_DB_NAME=ridge_cloud_e2e` and passed the real Cloud/Postgres E2E. Keep the database name explicit in future runs.

## BUG-CLOUD-ADAPTER-ERROR-DROP-01

Status: closed. `createCloudWebrtcTransportWith` forwarded provider state/frame callbacks but not `onError`, hiding actionable Cloud failure detail. `CloudWebrtcAdapter.onError()` now propagates message/code with unsubscribe semantics; focused suite `25/25` proves delivery and cleanup. Final Cloud run reached `connected=true` with enforced key binding.

## Coverage status after final Cloud run

Local V8 baseline is `43.65%` statements, `38.79%` branches, `44.54%` functions, `46.19%` lines (`164` files, `1650` passed, `1` skipped). Sonar server-side coverage remains `40.3%` until a new authenticated upload; the `>=80%` target remains open.

## BUG-SONAR-SCANNER-TIMEOUT-01

Status: open. `admin/admin` 表单登录可用，但新临时 token 的 scanner 尝试超过 `180s` 未完成；进程树已回收，token 已撤销，Sonar 未产生新 CE 任务。需下一轮定位 scanner 卡点后再上传覆盖率，不能以本地覆盖率冒充 Sonar 结果。

## BUG-E2E-MULTITAB-INSTRUMENTATION-RELOAD-01

Status: closed as harness bug. `cdp-multitab-freeze.mjs` assumed `window.__rgLag` survived dev HMR/navigation; after creating a workspace it could be absent and throw `TypeError`. The probe now waits for initial workspace/pane mount, retries the `+` control, and reinstalls instrumentation before each read. Post-change run passed with counts `1→2→3` and worst long task `10ms`.

## Latest post-change acceptance

`tauri:dev:cdp` CDP `1975` run passed DPR, pane graph, multitab, mobile Remote, cross-volume move, ACL rejection/restoration, and real Cloud/Postgres. The temporary `pnpm.cmd` shim was deleted; no publish, push, or release was performed.

## Coverage pipeline follow-up

The default parallel V8 run can fail during cleanup with `lstat coverage/.tmp`; serialized processing and file execution now make `pnpm test:coverage:sonar` pass. Latest local result is Statements `52.17%`, Branches `46.88%`, Functions `54.03%`, Lines `55.32%`. This closes the harness/pipeline bug, not the open `>=80%` coverage target.

## NLM next-loop candidates (read-only, post-auth repair)

NLM was re-authenticated through the fixed proxy and external CDP; the final query was read-only. Remaining P0/P1 candidates: trusted-HTTPS physical PWA/SW mobile proof; native PowerShell/WebView2 non-integer-DPR pixel matrix; Codex high-rate monotonic frame/old-row replay trace; physical mobile keyboard anchor ordering; clean-profile/extension A/B for third-party `runtime.lastError`; and a real authenticated Sonar upload that updates the stale server snapshot. No candidate is marked closed by hypothesis alone.

## Latest Sonar timeout evidence

`BUG-SONAR-SCANNER-TIMEOUT-01` remains open. The fresh authenticated scanner attempt timed out at the outer `364s` limit with exit `124`; no new Sonar CE task/analysis was produced. The exact scanner tree was terminated and its temporary token revoked; actual scanner Java count and matching temporary-token count are both `0`. Server metrics remain the previous successful analysis (`40.3%` coverage, Quality Gate `ERROR`).

## BUG-GIT-DISCOVERY-DEPTH-PILL-01

Status: closed for the current contract; runtime code was already present, and this continuation added no speculative behavior.

- CodeGraph recheck confirms `find_git_repos_below` is hard-capped at depth `1` while `find_git_repo_root` remains the separate ancestor-ownership API.
- Deterministic Rust scan tests pass `6/6`: root/direct-child discovery, no grandchild discovery, `.git` boundary, ignored trees, and caller depth widening rejection.
- `paneGitStatus.test.ts` passes `13/13`, including non-Git cwd with descendant repositories: the pane store remains `null`, so no descendant branch pill leaks into the pane.

## BUG-SONAR-TS-WORKTREE-DISCOVERY-01

Status: partial. The first authenticated scan succeeded; the scan-time root cause is identified, but the config-only verification scan exceeded the host limit.

- Accepted scan: CE task `5418a229-2bc5-4b7f-941e-b6f9bbf59672` → analysis `3ee09772-8826-4b77-8a44-a1f53227a2ad`, scanner exit `0`.
- Metrics: Sonar coverage `48.5%`, line `49.4%`, branch `47.0%`, violations `835`; Quality Gate remains `ERROR` (`new_coverage=45.5%`, `new_violations=130`).
- Root cause: TypeScript analysis recursively found 12 `.claude/worktrees` tsconfigs and spent about `444s`; `sonar.typescript.tsconfigPaths` now limits program discovery to four repository tsconfigs.
- Verification gap: the follow-up scan exceeded 10 minutes in the JS bridge; exact scanner tree was killed, temporary token was revoked, and no new CE task was claimed. Next run must inspect the scanner log before deciding whether the property is effective.

## Post-fix CDP acceptance (2026-08-09)

- `cdp-smoke.mjs`：CDP 连接、Ridge page target、协议 `1.3` 通过。
- 首次 `cdp-pane-graph.mjs` 暴露 `close broadcast` 缺失；根因是关闭最后一个 pane 后，结构事件接收端只检查剩余 pane 订阅，空 workspace 被误判为无订阅。
- 修复 `src-tauri/src/remote_host_impl.rs`：当前 workspace 即使暂无 pane 订阅，仍推送 `panes=[]`；Rust 三分支单测通过，修复后二次 pane graph：tree `1→2→1`、split/close broadcast 全通过。
- `cdp-multitab-freeze.mjs`：workspace `1→2→3`，worst long task `8ms`，`GATE: PASS`。
- `cdp-pty-parsers.mjs`：UTF-8 与 OSC 7 CWD 通过；OSC 2 标题仍失败（`seenTitle=null`），登记为 `BUG-PTY-OSC2-TITLE-01`，未冒充通过。

## BUG-PTY-OSC2-TITLE-01

Status: open, E2E-confirmed. 本轮新 CDP 实例中，PTY 原始字节与 OSC 7 CWD 均可观测，但 OSC 2 标题事件未回流（`titleOk=false`）。需下一轮沿 shell integration → PTY parser → `pty-meta` 广播链定位；当前不改动无关标题逻辑。
## BUG-EXPLORER-CROSS-VOLUME-ACL-PARTIAL-01

Status: physical sequence verified; direct `move_path` mid-operation injection remains environment-limited.

- `scripts/cdp-cross-volume-acl-e2e.mjs` performs a real cross-volume `copy_path` from `C:` to `D:`, then applies exact Windows ACL denials to the source tree and invokes real `delete_path`.
- Observed result: copy completed, source deletion returned Windows `os error 5`, ACL restoration succeeded, and both destination content and source content remained verifiable. Exit code `0`.
- The unmodified `move_path` call is not claimed as failed under this runtime: Windows/Rust cross-volume `rename` can complete through its OS path before the injected ACL is observed. Keep this distinction in future evidence.

## PTY metadata-lane follow-up (2026-08-09)

The OSC 2 delivery path was isolated from raw PTY backpressure: `RemotePaneSub` uses a dedicated metadata channel, and `metadata_broadcast_survives_a_full_raw_lane` passes `1/1`. Formatting and workspace `cargo check` pass. The subsequent CDP attempt had a fresh dev kernel but emitted no binary PTY frames, therefore it is inconclusive; `BUG-PTY-OSC2-TITLE-01` stays open pending a stable runtime rerun.

## NLM next-iteration candidates (2026-08-10)

The read-only query completed through the configured proxy. It returned the existing conversation ID `a47d3199-c1f9-47f1-927c-ff2c4875b77d`, so this is a new query rather than a newly created chat session. Candidates, ordered by evidence risk: native PowerShell/WebView2 DPR pixel matrix; Codex monotonic frame/old-row replay; trusted-HTTPS mobile PWA and background recovery; explicit Agent message delivery with PTY as guarded fallback; Explorer cross-volume refresh; quota/manual park-cause recovery. Source facts and inferences remain separate; none is marked closed by NLM output alone.
