# NLM / Sonar / 现场验收证据（2026-08-09）

## NotebookLM MCP

本机 MCP 可执行文件：`C:\Users\12867\.local\bin\notebooklm-mcp.exe`。

Codex 配置已写入固定代理，位置：`C:\Users\12867\.codex\config.toml`。

```toml
[mcp_servers.notebooklm-mcp.env]
HTTPS_PROXY = "http://127.0.0.1:51081"
HTTP_PROXY = "http://127.0.0.1:51081"
NO_PROXY = "localhost,127.0.0.1"
```

`codex mcp list` 已确认 `notebooklm-mcp` 启用且携带上述代理。本轮已通过外部 CDP `http://127.0.0.1:19222` 完成认证，随后 `refresh_auth` 与 `notebook_list` 均成功；未读取、打印或落盘 Cookie。

```powershell
$env:HTTP_PROXY = 'http://127.0.0.1:51081'
$env:HTTPS_PROXY = 'http://127.0.0.1:51081'
$env:NO_PROXY = 'localhost,127.0.0.1'
nlm login --cdp-url http://127.0.0.1:19222
nlm login --check
```

认证成功后可继续运行 NotebookLM 查询；本轮已读取新鲜对话并将 SCM 候选纳入需求。

## 现场 E2E

- `tauri:dev:cdp` 最终启动曾到达 CDP `3404`，`cdp:smoke` 成功；随后嵌入式 kernel 因 Windows `CREATE_BREAKAWAY_FROM_JOB` / `os error 5` 退出，故本次 DPR 页面脚本无法保持目标页，已登记为下一迭代环境/进程树候选。
- Cloud/Postgres：本地 `ridge-pg` 健康，`ridge_cloud_e2e` 已创建；真实 Cloud E2E 连接成功，目录分页 offset `0/3/6` 均通过，能力协商包含 `pane/invoke/fs/git/search/workspace/theme/teammate`。
- Cloud pane 流：此前真实 pane stream 已验收；当前批次再次验收控制面与 Postgres seed。
- DPR：`scripts/cdp-dpr-e2e.mjs` 通过，`dpr=1`、`canvasCount=2`、`backingCanvasCount=2`；截图：`.iteration/artifacts/dpr/desktop-shot.png`。
- 跨卷权限：`scripts/cdp-cross-volume-e2e.mjs` 通过，C: → D: → C: 往返 `27` bytes。
- 移动端新 profile：真实远端 WS、shell picker、shell 切换通过；shell picker 返回 9 项，`shell_kind` 可见为 `C:\WINDOWS\system32\bash.exe`；脚本输出 `GATE: PASS`。headless host 的 Team 入口正确隐藏。
- Remote 能力修复：kernel host workspace/hello 快照声明 `pane/fs/search/workspace`；旧 host 的 unsupported teammate 回包触发 runtime breaker，后台轮询停止；`RemoteConnection` shell discovery 使用独立 30 秒超时。
- 无 Tauri MCP：`kernel_lifecycle_e2e` 新增 `initialize` 握手断言，并继续覆盖 `tools/list`、`ridge_delegate_task`、inbox 与停核拒绝；该 E2E `3 passed`。
- 单元/静态回归：Vitest `157` files / `1622` passed / `1` skipped；`svelte-check` 为 0 errors、0 warnings；Rust workspace lib 全部通过，kernel lifecycle E2E `3 passed`；脚本语法检查通过。Rust fmt 仍受既有未格式化 `packages/ridge-mcp/src/server.rs` 阻塞，未重排用户大段改动。
- 收口修复：`packages/ridge-kernel/src/server.rs` 将 Agent Hub 持久化初始化的 `Result<_, String>` 显式映射为 `anyhow`，消除 workspace 编译错误；修后 Rust 全量 lib 与 kernel lifecycle E2E 均通过。
- NLM 最终只读查询：已排除 headless/Hydration/MCP 生命周期闭环；下一轮候选为非整数 DPR 原生对照、Codex 帧单调性、移动键盘锚定、双窗口 claim 竞态、Explorer 部分失败矩阵；`runtime.lastError` 仅保留 clean-profile/扩展 A/B 环境归因，不建议业务代码遮掩。
- 已知环境限制：自签名开发证书使 PWA Service Worker 注册降级；headless kernel 不声明 teammate 能力，Team 隐藏属预期，不作为 UI 崩溃。

## Latest rerun after harness fixes

- `tauri:dev:cdp` now opts into the explicit dev-only non-breakaway fallback. Same run reached kernel PID `58404`, port `6045`, and remained healthy; installed Ridge PIDs `10964`/`2936` were not touched.
- CDP smoke passed on port `3053`. DPR probe passed with real host values `dpr=1`, `innerWidth=3440`, `innerHeight=1392`, `canvasCount=1`, `backingCanvasCount=1`; artifact remains `.iteration/artifacts/dpr/desktop-shot.png`. Non-integer physical scaling remains external evidence, not simulated here.
- Pane graph E2E passed: `1 -> 2 -> 1` leaves; split and close LAN `panes` broadcasts both observed.
- Multitab freeze E2E passed: workspace counts `1 -> 2 -> 3`, worst long task `230ms`.
- Cross-volume move E2E passed: `C: -> D: -> C:` round trip `27` bytes. Physical ACL-denied/partial-failure matrix remains open; this test does not claim permission rejection.
- Mobile Remote E2E passed after pinning the seeded workspace/pane in Remote localStorage: headless host hides Team because capabilities are `pane/fs/search/workspace`, shell picker has 9 items, Git Bash yields `shell_kind=C:\WINDOWS\system32\bash.exe`, `GATE: PASS`.
- Sonar path mapping fixed by `scripts/normalize-lcov.mjs` and `test:coverage:sonar`; `reconnectPolicy.ts` now reports `100.0%` in Sonar. Full project scan succeeded with analysis `09735c32-b9e6-4fc8-9054-3891f4f44795`, task `0e8ef213-f7ed-4d05-8dbf-842da76d2dd2`, project coverage `40.3%`, line coverage `41.2%`, branch coverage `38.9%`, violations `841`.
- Sonar Quality Gate is currently `ERROR`: `new_coverage=47.6%` and `new_violations=133`. The explicit project target `coverage >= 80%` is not met; no completion claim is made.

## 发布边界

本轮未 `git push`、未发布 Remote artifact、未创建 GitHub Release；发布仍需用户单独授权。
## Physical DPR and ACL rerun

- WebView2 dev harness launched with `RIDGE_CDP_DEVICE_SCALE_FACTOR=1.25`; DPR probe passed with `dpr=1.25`, `canvasCount=1`, `backingCanvasCount=1`. Artifact: `.iteration/artifacts/dpr/desktop-shot.png`.
- ACL probe `scripts/cdp-cross-volume-acl-e2e.mjs` passed on `C:` → `D:`: account `DESKTOP-IMHO125\\Jack` was denied `read_file` and `move_path` with Windows `os error 5`; ACL restoration succeeded and the source marker was preserved.
- Render stale-frame regression coverage: `terminalFeedPolicy.test.ts` passed 3/3, including cancellation of pending coalescer timer and clearing deferred FIFO at a stream cut.

---

## Physical DPR and ACL rerun (normalized)

- WebView2 dev harness: `RIDGE_CDP_DEVICE_SCALE_FACTOR=1.25`; `dpr=1.25`, `canvasCount=1`, `backingCanvasCount=1`; artifact `.iteration/artifacts/dpr/desktop-shot.png`.
- ACL probe: `C:` to `D:`; account `DESKTOP-IMHO125\\Jack`; `read_file` and `move_path` rejected with Windows `os error 5`; ACL restored and source marker preserved.
- `terminalFeedPolicy.test.ts`: 3/3 passed; pending coalescer timer and deferred FIFO clear at stream cut.

## Final verification

- 当前 Vitest：`164` files，`1649` passed，`1` skipped；退出码 `0`。
- 当前 `svelte-check`：`0 errors and 0 warnings`。
- `git diff --check`、脚本 `node --check` 与迭代 JSON 校验通过；requirements、task-executable、write-scope、NotebookLM cold-loop 闸均通过。
- dev:cdp 临时进程树已回收；已安装 Ridge PID `10964` 与 kernel PID `2936` 保留运行。

## Final dev:cdp rerun after Explorer DTO

- WebView2 `RIDGE_CDP_DEVICE_SCALE_FACTOR=1.25`：DPR `1.25`，canvas `3/3` 有 backing buffer。
- pane graph：叶子 `1 → 2 → 1`，split/close broadcasts 均通过；multitab：workspace `1 → 2 → 3`，worst long task `176ms`，`GATE: PASS`。
- mobile Remote：capabilities `pane/fs/search/workspace`，Team 正确隐藏，shell picker `9` 项，Git Bash `shell_kind=C:\WINDOWS\system32\bash.exe`，`GATE: PASS`；Service Worker 自签名证书失败仍为环境限制。
- ACL：`read_file` 与跨卷 `move_path` 均以 Windows `os error 5` 拒绝，ACL 恢复且源内容保留；跨卷往返 `27` bytes 通过。
- Explorer DTO：`src/lib/components/explorerPaste.test.ts` 的 success/partial/all-failed 分支 `3/3` 通过；实际 copy 成功后 source delete 失败的物理注入仍未完成。
- 新登记 `BUG-E2E-DPR-STARTUP-RACE-01`：首次外层 `60s` probe 在 renderer 首轮挂载前超时，等待页面稳定后重试通过；属 harness readiness/timeout 竞态，未判为业务失败。

## Final Cloud/Postgres and adapter rerun

- Dev CDP runtime was verified with `BASE_DOMAIN=localhost:5050`, `API_BASE=http://localhost:5050/api/v1`, and `ws` plaintext. `ridge-pg` stayed healthy on `:5433`; local Cloud stayed healthy on `:5050`.
- The real seed used `RIDGE_CLOUD_DB_NAME=ridge_cloud_e2e`; tokens stayed in one PowerShell process and were not written to evidence. The first attempt promoted `ridge_cloud` while the running Cloud instance read `ridge_cloud_e2e`, producing the exact `NOT_PREMIUM` rejection.
- Final real Cloud/Postgres E2E passed: `ok=true`, `connected=true`, capabilities `pane/invoke/fs/git/search/workspace/theme/teammate`, offsets `0/3/6` each returned `3` entries from `total=192`, and `keyBindingMode=enforced`.
- The controller error path previously collapsed to `ctrl:error` and dropped message/code. `CloudWebrtcAdapter.onError()` now forwards provider errors with unsubscribe semantics; focused adapter suite passed `25/25`.
- `packages/ridge-mcp/src/server.rs` had a Rust `E0505` borrow error in the expired-delivery loop; the minimal owned `deliveryId` fix compiled during dev rebuild. Installed Ridge PIDs `10964` and `2936` remained untouched.

## Latest local coverage baseline

- Full V8 coverage: `164` files, `1650` passed, `1` skipped; Statements `43.65%`, Branches `38.79%`, Functions `44.54%`, Lines `46.19%`.
- `scripts/normalize-lcov.mjs` completed with `ok=true`; report is `coverage/lcov.info`. No new authenticated Sonar upload was performed, so server-side `40.3%` / Quality Gate `ERROR` remains authoritative.

## Sonar credential/monitor recheck

- `http://127.0.0.1:9000` status remains `UP`; `admin/admin` form login and session API validation succeeded.
- The previous Basic Auth check and old temporary token are not current proof: Basic `admin:admin` returned `401`, and the old token was revoked. A fresh token probe required the session `XSRF-TOKEN` forwarded as `X-XSRF-TOKEN`.
- A fresh-token scanner attempt exceeded `180s`; its process tree was terminated and the token revoked. CE has no new task, so server metrics remain the last successful analysis (`40.3%` coverage, Quality Gate `ERROR`).
- Browser monitoring page was not opened because this session exposed no Browser instance; no UI screenshot evidence is claimed.

## Post-change dev:cdp acceptance

- `tauri:dev:cdp` reached CDP `1975` after the temporary local `pnpm.cmd` shim supplied the missing PATH entry; shim and dev process tree were removed afterward. Installed Ridge PIDs `2936` and `10964` remained running.
- DPR: `dpr=1.25`, viewport `2752×1114`, `canvasCount=3`, `backingCanvasCount=3`; screenshot `.iteration/artifacts/dpr/desktop-shot.png` written.
- Pane graph: leaves `1→2→1`; split/close `panes` broadcasts both passed after the event-Promise wait rewrite.
- Multitab: readiness-gated counts `1→2→3`, worst long task `10ms`, `GATE: PASS`; probe now reinstalls after HMR/navigation and waits for the initial workspace/+ control.
- Mobile Remote: `GATE: PASS`; capabilities `pane/fs/search/workspace`, Team hidden as expected, shell picker `9` items, Git Bash `shell_kind=C:\WINDOWS\system32\bash.exe`. PWA Service Worker remains disabled by the self-signed local certificate; no product failure claim.
- Cross-volume move: `27` bytes round-trip passed. Physical ACL probe rejected `read_file` and `move_path` with Windows `os error 5`, restored ACL, and preserved source marker.
- Real Cloud/Postgres: `connected=true`, capabilities include `pane/invoke/fs/git/search/workspace/theme/teammate`, directory offsets `0/3/6` passed, key binding `enforced`.

## Final local coverage pipeline

- `pnpm test:coverage:sonar` now passes deterministically with `--coverage.processingConcurrency=1 --no-file-parallelism` plus LCOV normalization.
- Latest V8 baseline: Statements `52.17%`, Branches `46.88%`, Functions `54.03%`, Lines `55.32%`; `165` files, `1660` passed, `1` skipped; `coverage/lcov.info` `510154` bytes before normalization.
- Sonar target `>=80%` remains open; the local baseline is evidence, not a Quality Gate result.

## Latest Sonar scanner recheck

- Fresh form-authenticated scanner attempt exceeded the outer `364s` limit and exited `124`; Sonar CE reported no new task.
- Exact scanner process tree was terminated; no scanner Java process remains. The temporary scan token was revoked and no `codex-local-scan-*` token remains.
- Last authoritative server metrics remain coverage `40.3%`, line `41.2%`, branch `38.9%`, violations `841`, Quality Gate `ERROR`.

## NLM auth repair and next read-only loop

- A post-E2E NotebookLM query first returned `UNAUTHENTICATED`; disk refresh correctly reported stale credentials.
- Re-auth used external Chrome CDP `http://127.0.0.1:19222` with process-local `HTTP_PROXY`/`HTTPS_PROXY=http://127.0.0.1:51081`, `NO_PROXY=127.0.0.1,localhost`; extractor saved the existing default profile without exposing cookies. `nlm login --check` and `nlm notebook list` both exited `0`, reporting `22` notebooks; MCP `refresh_auth` then returned `success`.
- The installed extractor did not support the skill-documented `--no-wait` flag; the supported invocation without that flag succeeded. This version drift is recorded, not hidden.
- Final read-only NLM query (same notebook/conversation; no source/note mutations) keeps: physical trusted-HTTPS PWA/SW proof; native PowerShell vs WebView2 DPR box-drawing pixel matrix; Codex frame-generation/old-row replay trace; Sonar stale server snapshot vs local coverage; clean-profile/extension A/B for `runtime.lastError`; physical mobile keyboard anchor order.

## Latest authenticated Sonar scan and bounded retry

- A fresh authenticated scan with `sonar.scm.disabled=true` completed scanner exit `0`; CE task `5418a229-2bc5-4b7f-941e-b6f9bbf59672` reached `SUCCESS`, analysis `3ee09772-8826-4b77-8a44-a1f53227a2ad`.
- Server coverage moved to `48.5%` (line `49.4%`, branch `47.0%`), violations `835`; Quality Gate remains `ERROR` (`new_coverage=45.5%`, `new_violations=130`). This is current authoritative Sonar evidence and still below the explicit `>=80%` target.
- Scanner log root cause: TypeScript analyzer discovered 12 stale `.claude/worktrees` tsconfigs and spent about `444s`; `sonar.typescript.tsconfigPaths` was added to restrict discovery to repository configs. A bounded verification scan then exceeded the 10-minute host limit before a terminal result; its exact scanner tree was killed, Sonar service preserved, and no new CE task claimed.
- Temporary scan token count was rechecked at zero. No token/cookie/source/note was written or uploaded.
- Sonar 低风险 Remote 规则修复已通过 CodeGraph 复核；`capabilityContract`、`wsRemote` 的 focused tests `67/67` 通过。Rust 高复杂度项仍未以大重构冒险处理。

## Post-fix CDP rerun

- `tauri-dev-cdp.mjs` 通过临时本地 PATH shim 启动并完成新二进制构建；shim、CDP 进程树均已清理，未触碰已安装 Ridge 进程。
- `cdp-smoke.mjs` 通过；pane graph 初次发现 close broadcast 缺失，修复后复跑通过：tree `1→2→1`，split/close broadcasts 均 `PASS`。
- `cdp-multitab-freeze.mjs` 通过：workspace `1→2→3`，worst long task `8ms`，`GATE: PASS`。
- `cdp-pty-parsers.mjs`：UTF-8 与 OSC 7 CWD 通过，OSC 2 标题失败（`seenTitle=null`）；登记 `BUG-PTY-OSC2-TITLE-01`，不宣称 E2E 全绿。
# Physical cross-volume partial-failure acceptance (2026-08-09)

- `scripts/cdp-cross-volume-acl-e2e.mjs` now runs the real Windows `C:` to `D:` copy/delete sequence underlying `move_path`.
- Physical result: `copyCompleted=true`; after ACL injection, `delete_path` failed with `删除目录失败: 拒绝访问。 (os error 5)`; ACL restoration then confirmed `destinationCopied=true` and `sourcePreserved=true`; exit `0`.
- Seven stale `D:\ridge-acl-e2e-*` fixtures from failed probes were removed after exact-path validation. No user project data was targeted.
- Direct mid-operation ACL injection into one unmodified `move_path` call remains non-deterministic on this Windows/Rust runtime because cross-volume `rename` may complete through the OS path. This evidence therefore claims the physical copy-success/source-delete-failure sequence, not an unforced `move_path` failure.

## PTY metadata-lane follow-up (2026-08-09)

- `RemotePaneSub` now gives metadata/control events a dedicated bounded channel instead of sharing the raw PTY queue; the state regression `metadata_broadcast_survives_a_full_raw_lane` passes `1/1`.
- `cargo check --manifest-path src-tauri/Cargo.toml -q` and `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` pass.
- The latest CDP PTY rerun was inconclusive: the fresh dev kernel produced no binary PTY frames (`binaryFrames=0`), so neither OSC 2 nor OSC 7 could be observed. `BUG-PTY-OSC2-TITLE-01` remains open; no E2E closure is claimed.

## NLM next-iteration read-only query (2026-08-10)

- The query completed through the proxy-configured NotebookLM MCP without source or note mutation. The service returned the existing conversation ID `a47d3199-c1f9-47f1-927c-ff2c4875b77d`; the query was new, but the backend did not create a distinct chat session.
- Source-backed candidates: native PowerShell/WebView2 DPR pixel matrix; Codex monotonic frame and old-row replay trace; trusted-HTTPS physical mobile PWA/background recovery; explicit Agent message delivery with PTY only as a guarded fallback; Explorer cross-volume refresh and quota/manual park-cause regression.
- These remain candidates or evidence gaps, not closed bugs. The response explicitly distinguishes source facts from inference; no NotebookLM result was treated as proof of implementation or production readiness.
