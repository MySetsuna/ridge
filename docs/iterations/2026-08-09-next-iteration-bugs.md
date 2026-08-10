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

Status: open. 旧默认凭证已失效；新临时 token 的 scanner 尝试超过 `180s` 未完成，进程树已回收，token 已撤销，Sonar 未产生新 CE 任务。需下一轮定位 scanner 卡点后再上传覆盖率，不能以本地覆盖率冒充 Sonar 结果。

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

Status: partial. Root cause fixed locally: the Kernel/rdg metadata path now parses all OSC 0/1/2 candidates by stream position, so a stale OSC 0 cannot mask a later OSC 2. `ridge-core` title tests pass `6/6`; `ridge-cli` metadata tests pass `2/2`. Stable physical/CDP rerun remains required to close the field evidence gap.
## BUG-EXPLORER-CROSS-VOLUME-ACL-PARTIAL-01

Status: physical sequence verified; direct `move_path` mid-operation injection remains environment-limited.

- `scripts/cdp-cross-volume-acl-e2e.mjs` performs a real cross-volume `copy_path` from `C:` to `D:`, then applies exact Windows ACL denials to the source tree and invokes real `delete_path`.
- Observed result: copy completed, source deletion returned Windows `os error 5`, ACL restoration succeeded, and both destination content and source content remained verifiable. Exit code `0`.
- The unmodified `move_path` call is not claimed as failed under this runtime: Windows/Rust cross-volume `rename` can complete through its OS path before the injected ACL is observed. Keep this distinction in future evidence.

## PTY metadata-lane follow-up (2026-08-09)

The OSC 2 delivery path was isolated from raw PTY backpressure: `RemotePaneSub` uses a dedicated metadata channel, and `metadata_broadcast_survives_a_full_raw_lane` passes `1/1`. Formatting and workspace `cargo check` pass. The subsequent CDP attempt had a fresh dev kernel but emitted no binary PTY frames, therefore it is inconclusive; `BUG-PTY-OSC2-TITLE-01` stays open pending a stable runtime rerun.

## NLM next-iteration candidates (2026-08-10)

The read-only query completed through the configured proxy. It returned the existing conversation ID `a47d3199-c1f9-47f1-927c-ff2c4875b77d`, so this is a new query rather than a newly created chat session. Candidates, ordered by evidence risk: native PowerShell/WebView2 DPR pixel matrix; Codex monotonic frame/old-row replay; trusted-HTTPS mobile PWA and background recovery; explicit Agent message delivery with PTY as guarded fallback; Explorer cross-volume refresh; quota/manual park-cause recovery. Source facts and inferences remain separate; none is marked closed by NLM output alone.

## BUG-AGENT-PTY-SAFETY-PROOF-01

Status: local runtime gate closed; live field evidence remains open.

- `DeliveryRegistry` now records the complete five-condition PTY proof against Agent `generation`/`lease`, expires proof after `3s`, rejects stale teardown, and routes expired/unsafe proof to MCP pull.
- Deterministic Rust coverage: invalid identity, same-generation refresh, generation/lease fencing, stale teardown, expiry, and safe/unsafe adapter selection; `cargo test -p ridge-mcp --lib` `90/90`.
- Remaining proof: live Agent interruption/replay while the target CLI is busy; PTY remains best-effort fallback and is not claimed as a production Runtime API/A2A substitute.

## New rerun findings (2026-08-10)

### BUG-CLOUD-SEED-DB-MISMATCH-01

Status: closed in fixture. `cdp-cloud-seed.mjs` now defaults to `ridge_cloud_e2e` and verifies `RETURNING username`; a wrong database can no longer silently mint a non-premium token. Full Cloud/Postgres plus pane PTY E2E passed.

### BUG-E2E-DPR-STARTUP-RACE-02

Status: partial, harness guard added. The desktop page now publishes a persistent `window.__ridgeAppReady` marker at the existing workspace readiness boundary; `scripts/cdp-dpr-e2e.mjs` waits for that marker before a separate bounded renderer wait, so a cold startup timeout is classified as app-readiness failure rather than renderer failure. Real WebView2 `dpr=2` cold-start evidence and app-ready → first-canvas timing remain required.

### BUG-MOBILE-PWA-TRUSTED-HTTPS-01

Status: external-environment gap. LAN mobile protocol and shell flow passed; self-signed `https://127.0.0.1:9527` caused Service Worker registration `SecurityError`. Trusted certificate/public-host or physical-device evidence remains required; no business-code workaround is justified.

### BUG-MOBILE-RUNTIME-LASTERROR-ATTRIBUTION-01

Status: evidence incomplete. Clean Chromium profile produced zero `runtime.lastError` entries, but no extension A/B or physical mobile run exists. Keep the source attribution open and do not modify listeners merely to suppress the warning.

## BUG-CODEX-FRAME-REPLAY-01

Status: deterministic local guard closed; physical replay evidence remains open.

- NLM query `43268d567cb5` selected the Codex/ridge-term old-frame resurrection candidate. The implementation adds a per-pane monotonic `frameId` from `TerminalManager` through `workerRendererBridge` and `WorkerHostedRenderer` into the render-worker protocol.
- The worker rejects invalid frame ids and ACK-drops stale/replayed generations before `kernel.applyDeltaFrame` or `renderer.render`; no-frame legacy messages remain compatible.
- Tests cover increasing, stale, invalid, manager counter, bridge wire propagation, and renderer non-repaint branches. Full Vitest and `svelte-check` are green as recorded above.
- Remaining proof: recorded PTY/JSONL replay and physical ConPTY/Agent latency trace. These are not inferred from the local unit guard.

## BUG-SONAR-ES-DISK-WATERMARK-01

Status: host state repaired; accepted coverage gate remains open.

- CE task `5c57440f-6f4b-40a7-8568-dec11c91e6af` failed because Elasticsearch marked `projectmeasures` read-only at the flood-stage watermark. Local logs show the data path at about `4.1%` free.
- Generated Rust build cache was moved to the recoverable D: archive `D:\wind-target-debug-archive-20260810`; Elasticsearch later logged that the high watermark cleared and read-only blocks were released.
- Follow-up scans were bounded and exact scanner trees were cleaned. The full Rust analyzer still exceeded the host bound after cache relocation; no false green was recorded.
- Reopen with a warmed Rust build cache or an explicitly bounded Sonar Rust strategy, then obtain a new scanner exit `0`, CE `SUCCESS`, project metrics, and Quality Gate. Current accepted baseline remains `48.5%` coverage and Gate `ERROR`; target `>=80%` is not closed.

## BUG-AGENT-PTY-FALLBACK-CONTRACT-01

Status: local contract branch closed; live Agent delivery evidence remains open.

- NLM's next-batch audit proposed replacing PTY-as-message with Message Hub routing and retaining PTY only as a guarded fallback. CodeGraph found the Hub/Inbox/receipt/adapter path already present in `packages/ridge-mcp`; no speculative architecture rewrite was justified.
- Added deterministic coverage for `objective` payload alias, `submitRequested=false`, and non-text payload rejection. `cargo test -p ridge-mcp --lib`: `87 passed; 0 failed`.
- Remaining proof: a live Agent/runtime interruption or recorded old-frame replay showing Message Hub delivery while a target CLI is busy, with PTY fallback only after all five safety predicates pass.

## NLM next-batch audit (2026-08-10)

Status: local contracts already present; external lifecycle/field proof remains open.

- Async query `e172bd743e38` returned five candidates: deep-root kernel lifecycle, Message Hub delivery, structured Agent history/resume, cross-volume Explorer continuity, and kernel-owned domain SSOT. It reused the existing conversation ID; no new chat or note mutation is claimed.
- CodeGraph found `AgentResumeSpec`, native session aggregation, exact identity/CWD binding, `KernelHost`, and `kernel_lifecycle_e2e`. The kernel lifecycle integration test passed `3/3`; the history parser and grouping tests are present. This iteration adds no speculative architecture rewrite.
- Open acceptance remains: actual desktop-shell exit with kernel survival/reattach, live Message Hub delivery while a CLI is busy, physical cross-volume mid-operation ACL injection, and proof that every domain path is callable after the desktop shell exits.

## 2026-08-10 real dev:cdp rerun

- Fresh dev instance CDP `4515` / Vite `12865`: CDP smoke passed; physical WebView2 DPR was `1`, with `1` backing canvas; cross-volume copy-success/source-delete-denied preserved the source and copied the destination.
- `scripts/cdp-pty-parsers.mjs` had two harness defects: repeated `panes` snapshots could resubscribe and move the marker command to another pane, and the target filter rejected the dynamic Vite port. The script now drives once and accepts any loopback port. A stable post-fix PTY parser rerun is still required.
- Mobile E2E now passes `workspaceId` explicitly to `write_to_pty`; before this, a pane could pass scrollback readiness and then fail `Pane not found` on write. The next run reached real LAN verification, workspace/pane projection, headless capability rejection, and shell picker, but the in-app WebView2 target detached during the roster probe after navigating the Tauri page to the remote HTTPS page; no mobile UI pass is claimed.

### BUG-MOBILE-CDP-NAV-LIFECYCLE-01

Status: closed as harness defect. The script now supports a separate external Chrome CDP target with bounded commands and explicit socket cleanup. The fresh external-target rerun completed the authenticated mobile SPA/data-plane and reached the shell gate; the former Tauri-WebView2 detach is no longer the active path.

## 2026-08-10 final dev:cdp evidence wave

- Cloud/Postgres full E2E passed on the fresh dev instance: authenticated host, `pane/invoke/fs/git/search/workspace/theme/teammate` capabilities, three paged directory reads, enforced keybinding mode, and expected host offline cleanup.
- Physical desktop gates passed: WebView2 DPR `1`, two backing canvases; cross-volume copy completed, source-delete ACL rejected with Windows error 5, destination preserved; pane tree split/close and LAN broadcasts passed; teammate E2E `7/7`; multitab freeze passed with workspace counts `1 → 2 → 3`, worst long task `13ms`.
- Live PTY parser E2E passed after the harness waited for long PowerShell input echo: `10` binary frames, `4` `pty-meta` frames, UTF-8 decode, OSC 2 title, and OSC 7 CWD all passed. The earlier `6.5s` cutoff was a test harness race, not a parser loss.
- Mobile harness now uses kernel layout as the pane source of truth, retries the writer through the scrollback/live-handle gap, supports a separate external Chrome CDP target, bounds CDP calls, and closes both desktop/mobile sockets. The fresh external Chrome rerun loaded the authenticated mobile SPA/data plane; headless host correctly advertised no teammate capability; shell picker rendered 9 items and Git Bash switching changed `shell_kind` to `C:\WINDOWS\system32\bash.exe`; `GATE: PASS`. The earlier empty-list run was a harness/slow-response observation, not retained as a product failure.

### BUG-MOBILE-SHELL-RPC-TIMEOUT-01

Status: closed by external rerun. The prior run hit the 30-second `ws.listShells()` timeout while the host response arrived late. With the external clean-browser target and corrected lifecycle/activation harness, the UI rendered all 9 shell items; the same host-side `detect_available_shells` response was correlated and Git Bash switching was verified through the snapshot. No production transport change was justified.

Remaining acceptance gaps: non-integer/physical multi-DPR matrix beyond the verified DPR=2 run, mid-operation cross-volume ACL injection, and desktop-shell exit with kernel reattach.

## Physical DPR=2 rerun (2026-08-10)

- Fresh isolated `tauri:dev:cdp` launched with `RIDGE_CDP_DEVICE_SCALE_FACTOR=2`; WebView2 process command line carried `--force-device-scale-factor=2` and `--device-scale-factor=2`.
- After the app-ready marker became true, `scripts/cdp-dpr-e2e.mjs` passed: `dpr=2`, `innerWidth=1720`, `innerHeight=696`, `canvasCount=3`, `backingCanvasCount=3`; screenshot: `.iteration/artifacts/dpr/desktop-dpr2-shot.png`.
- The first cold probe timed out before mount and reported zero canvases; a direct readiness probe then observed the normal Ridge workspace and the bounded DPR probe passed. This closes the physical DPR=2 gate, while the harness cold-start timing classification remains tracked by `BUG-E2E-DPR-STARTUP-RACE-02`.

## Kernel reattach boundary (2026-08-10)

- `cargo test -p ridge-cli --test kernel_lifecycle_e2e -- --nocapture` completed the three integration cases: `3 passed; 0 failed`.
- Evidence covers detached kernel survival after the disposable `rdg` client exits, second-client attach to the same kernel PID, and PTY output-lease detach/reattach with cursor replay. It does not claim a Tauri WebView2 shell process kill and restart; that physical desktop-shell boundary remains open.

## NLM next query after auth refresh (2026-08-10)

- NLM auth was refreshed through the configured proxy `http://127.0.0.1:51081` and external CDP; verification returned `login_check_exit=0`, `notebook_list_exit=0`, `notebook_count=22`.
- Read-only query `ef19cdb84765` reused conversation `a47d3199-c1f9-47f1-927c-ff2c4875b77d` and returned three candidates: Codex high-rate frame replay, Explorer cross-volume partial-failure DTO/refresh, and mobile PWA recovery/keyboard anchoring. No note/source mutation occurred.
- CodeGraph recheck shows the Codex monotonic guard and replay branches already exist: `TerminalManager.applyDeltaFrame` increments per-pane `deltaFrameId`; worker `handleRequest` rejects invalid/stale `frameId`; `renderWorker.test.ts` covers accepted-then-replayed frames and renderer/kernel non-repaint. NLM's suggestion to add that guard is therefore stale against the current tree; no duplicate rewrite is justified.
- The remaining implementable gaps are evidence-bound: a recorded Codex/Claude PTY replay trace, physical cross-volume mid-operation ACL injection, and physical mobile PWA background/IME proof. The current iteration adds no speculative production behavior for these external gaps.

## Sonar final monitor / scan evidence (2026-08-10)

- Local SonarQube `26.7.0.124771` remains `UP`; session-authenticated monitor reports project coverage `56.7%`, line `57.0%`, branch `54.2%`, violations `835`, Quality Gate `ERROR` (`new_coverage=64.9%`, `new_violations=130`). These are server metrics, not local LCOV.
- Fresh authenticated scanner submission created CE task `5c57440f-6f4b-40a7-8568-dec11c91e6af`, but CE failed while indexing `projectmeasures`: Elasticsearch flood-stage watermark set the index read-only; server logs reported only about `6.8%` free disk. Scanner child processes were explicitly reaped; no token was stored.
- Serialized local V8 run passed with normalized LCOV: statements `12774/18608 = 68.65%`, branches `7066/11610 = 60.86%`, functions `2492/3536 = 70.48%`, lines `11514/15895 = 72.44%`. The Sonar project `>=80%`/Quality Gate target remains open; local coverage does not substitute for a successful CE analysis.
- Post-build monitor refresh remains consistent: Sonar `UP`, `coverage=56.7`, `line_coverage=57.0`, `branch_coverage=54.2`, `violations=835`, Quality Gate `ERROR`; the current C: free space is about `63.26 GB`, so no unbounded index/cache deletion was attempted.

## Final deterministic gates (2026-08-10)

- Full Vitest: `202` files, `1858 passed / 1 skipped`; focused remote/pane regressions: `36/36`.
- `pnpm check`: `0 errors / 0 warnings`; `pnpm build`: exit `0` (Vite warnings only); `cargo fmt --all -- --check`: exit `0`; `cargo test -p ridge-mcp --lib --quiet`: `90/90`.
- Changed CDP scripts pass `node --check`; CodeGraph final trace confirms shell discovery, pane selection/write, cloud host, and kernel command paths. No publish, push, tag, or Release was performed.

## BUG-CDP-START-VITE-TARGET-01（open, 2026-08-10）

- Re-running `pnpm tauri:dev:cdp` on Node `v25.9.0` built `ridge-cli` and desktop Rust successfully, then failed in `scripts/start-vite-dev.mjs:19` with `TypeError: Cannot read properties of undefined (reading '_events)`.
- The failure occurs before WebView2/CDP readiness, in the `beforeDevCommand` Vite launcher. Reproduce with the same command; next iteration should make child-process creation/exit handling compatible with the current Node runtime, then rerun `cdp:smoke` and the desktop E2E matrix.
