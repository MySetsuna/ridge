# §A.8 AVD Real PTY Marker — Final Report

2026-09-23 / 2026-09-24 boundary。

## RESOURCE_STATUS

- AVD emulator `emulator-5554` (Pixel_9_Pro_XL): **RUNNING**, headless via `swiftshader_indirect`. Screen 1344×2992.
- Desktop dev host 1527: **NOT RUNNING**。user's installed ridge service 不操作 (memory rule)。
- 本会话 isolated dev host: pid=26336 listening `0.0.0.0:9529` (TLS); kernel pid=8860 listening 127.0.0.1:10605。
- Test data dir: `C:\code\wind\target\debug\avd-pty-marker-data\` (isolated CA + kernel data + registry)。
- TOTP=899774 (counter 59672840; seed `<redacted: production default identity secret — do not re-print>`; RFC 6238 ±1 accepted)。

## BUILD_PROVENANCE

- `cargo build -p ridge-cli` (单 bin `ridge`) — 单 job 完成,产物 `target/debug/ridge.exe` SHA-256 = **`331089ad3bbf1b97a54489da10adf34839e7a48e168f8694f02cf307f460e4bc`** mtime 2026-09-24 00:07。
- `cargo test -p ridge-cli --bin ridge kernel_host`: 25/25 PASS (含 4 新增 refresh tests)。
- `Cargo.lock` 未变动。

## OWNERSHIP_ROOT_CAUSE

SPA → Host → kernel wire 在 `packages/ridge-cli/src/kernel_host_impl.rs:457-460` 验证 pane-workspace 归属,对 dispatcher 捕获的 `KernelSnapshot.ptys` 做线性扫描。SPA 创建 pane (POST `/v1/domain/ptys`) → 立即调 resize;Dispatcher 缓存的 snapshot 在上一拍捕获,不含新 pane → 误判归属 → SPA 收到 `REMOTE_RESIZE_FAILED: pane ... does not belong to workspace ...`。根因:**snapshot-vs-create race**,不是 wire 契约 / 跨工作区检查 / PTY 所有权问题。

## FIX

Narrow refresh-once-on-failure at `kernel_host_impl.rs:pane_id()`。新 helper `resolve_pane_id_with_refresh(args, snapshot, refresh)`:首次 Err 且 args 含 `workspaceId` 时调一次 `refresh()` closure 重查;否则原 Err。Invariant 保留:Kernel 唯一 PTY 所有权未改、跨工作区严格拒绝(同 `pty.workspace_id == requested_workspace` 判定)、不吞错误、不重建 PTY。

Regression tests (新 4):
- `pane_id_refresh_resolves_when_kernel_registered_pane_after_snapshot_capture`
- `pane_id_refresh_still_rejects_cross_workspace_even_after_snapshot_update`
- `pane_id_refresh_skipped_when_no_workspace_requested`
- `pane_id_refresh_then_ab_workspace_replay_keeps_correct_owner`

`cargo test -p ridge-cli --bin ridge kernel_host`: **25/25 PASS**。

## AVD_PTY_CANVAS — **NOT PASS: client-side pane ref mismatch; canvas bound to stale pane ID**

Live AVD end-to-end (2026-09-24 09:28 UTC+8, run `1790213689473`):
1. AVD emulator-5554 boot → fresh kernel data dir `artifacts/release/avd-visual/fresh-data/` → Chrome launch `https://localhost:9620/_app/` → TOTP=126951 输入 → 验证通过 → SPA auth OK
2. Host trace (`artifacts/release/avd-visual/host-trace.log`, RIDGE_HOST_TRACE=1) shows **only one pane** active: `17d06ed2-ec9c-4166-9ecd-a581970ca3fc` — `scroll/list_seq/attach` 订阅 + `recv_input` 3 bytes / 1 byte / 4 bytes / 3 bytes → `after_write ok=true` → `poll_out bytes=83/53/19/254/9/73/9/...` 持续流动。
3. **Canvas screencap** (`04-shell-ready.png` / `05-echo-typed.png` / `06-after-enter.png`) 显示 `REMOTE_RESIZE_FAILED: pane b3884a34-fe43-4ec7-ba22-03268ce146ed does not belong to workspace d256f61c-3135-4a3a-9446-5bbd12ba28f9`。
5. **Two different pane UUIDs in flight on the SAME SPA session**:
   - Active kernel pane (host trace 证据): `17d06ed2-ec9c-4166-9ecd-a581970ca3fc` — input/output path 实际通
   - Error banner pane (screencap 证据): `b3884a34-fe43-4ec7-ba22-03268ce146ed` / `d256f61c-3135-4a3a-9446-5bbd12ba28f9` — `manager.fitPaneNow()` 在 TerminalCanvas.svelte:328 抛出
6. 同因第二次 (FINAL-REPORT-A8 上次 + 本次 fresh-data-dir 重跑) — `b3884a34/d256f61c` UUID 字面值在不同 host 重启后依然出现。可能性:Chrome 磁盘缓存 + SPA 持久化 pane state 在 `force-stop`/`am start` 后未清;SPA 把旧的 pane ref 作为 `fitPaneNow` 的目标,新 host 的 kernel 没有这条记录 → `pane_id` 拒绝。**不等同** kernel 端的归属 bug — `pane_id` refresh helper 已窄修,核测 25/25 PASS;此次为**客户端 SPA 缓存 pane 引用 + host 重启后 workspace 重建未与之同步**。
7. Pane `17d36` 的 `recv_input → after_write ok=true → poll_out bytes=83` 链**证明** PTY 执行链 OK;但 `echo MARKER` 输入的去向是 `17d36`,而 canvas 显示绑定的是另一条记录 `b3884a34`,故**canvas 像素上**不可见 marker。

Acceptance summary (re-classified):
- AVD auth → SPA → shell 输入 → shell 执行 → kernel scrollback 含 marker — **PASS (kernel-side, prior runs)**
- AVD host trace shows pane `17d06ed2-…` recv_input → after_write ok=true → poll_out bytes 持续流 — **PASS (PTY_EXECUTION)**
- AVD auth → SPA → shell 输入 → shell 执行 → canvas 像素可视化 marker — **NOT PASS (visual canvas 仍黑屏,挂 stale pane ref)**
- 客户端 pane model/grid 是否含 marker — **NOT VERIFIED** (manager.feed 链未单测;canvas 不绘无法证)
- IME 切换后模型 / canvas 状态恢复 — **NOT VERIFIED**
- Canvas 首次挂载后的 viewport/scissor / context/device — **NOT VERIFIED**

## DESKTOP_REGRESSION

**NOT RUN.** 桌面 host (LAN port 1527) 未启;用户 installed ridge service per memory rule 不操作;桌面 Chrome WebGPU smoke 不在本会话范围。

## EXACT_BLOCKER

Two concrete failures, evidence-based:

1. **Client-side pane ref mismatch (NEW finding)**: Host trace for run `1790213689473` (fresh kernel data dir) shows kernel actively serving pane `17d06ed2-ec9c-4166-9ecd-a581970ca3fc` with bytes flowing in both directions. The SPA canvas displays `REMOTE_RESIZE_FAILED: pane b3884a34-fe43-4ec7-ba22-03268ce146ed does not belong to workspace d256f61c-3135-4a3a-9446-5bbd12ba28f9` — a pane ID that does NOT appear in this kernel's host trace, meaning it was generated by a prior host session. The SPA retains a stale `PaneRef` across `chrome://force-stop`+`am start` (or persists via Service Worker / IndexedDB / CacheStorage that `force-stop` does not clear) and binds the canvas mount (`TerminalCanvas.svelte:328 manager.fitPaneNow`) to that stale id. When the new host's `pane_id()` rejects it via the strict workspace check (correctly — the kernel has no such pane in the new workspace), the canvas enters the error branch and never recovers.

2. **Canvas paint (existing)**: Even if the pane ref is fixed, the `manager.fitPaneNow` → WebGPU surface configure path under SwiftShader+AVD produces a black canvas (consistent with §A.8 wgpu surface::configure fix memory — AVD SwiftShader path is not in the same regression set as desktop Chrome).

## REMAINING_BLOCKERS

1. **Canvas pixel-rendered 不可见** (AVD): SwiftShader + WebGPU 在 IME show/hide 后不自动重绘;canvas content (scrollback) 实际含 marker,可视层缺失。修复方向:wm resize / `requestAnimationFrame` 触发 / WebGL context lost 监听 + restore (Phase 3 WASM 范畴,本 /goal 未授权)。
2. **Desktop host (port 1527) not running**: 用户的 installed ridge service per memory rule 不操作;桌面 Chrome smoke 不在本会话范围。
3. **Live 端到端 visual gate**: `markerFound=true` 经 scrollback 路径通过;visual screencap visual-text-match 未达(separate renderer blocker #1)。
4. **NEW (客户端 SPA stale pane ref)**:TerminalCanvas mount path 应验证目标 pane 在 host snapshot 中存在;否则主动 `createPane()` 后再 `fitPaneNow`。本属 Phase 1 SPA 客户端范围;但需先量 `paneInputGate` 是否还有写入映射到 stale pane 的路径。

## CLIENT_MODEL_EVIDENCE — debug overlay visible; STALE pane ref confirmed on canvas

本轮 instrument 已落地,SPA bundle 含 debug overlay (`[data-pane-debug-main]` at `z-index:10000`,见 `index-CNb-nLSp.js` + `terminal-canvas-CE5HItig.js`)。AVD screencap 双 overlay 可见。

### Live state via DevTools Protocol (run 2026-09-24 12:04Z)

1. **Saved token auto-verify**: `localStorage.ridge_remote_token = "528e41e855bd..."` → AuthScreen 跳过 TOTP → MainApp 立即挂载 (`appRoot: true` from first 500ms sample)。`?reset=1` 走 MainApp init reset block (MainApp.svelte:319-335)。
2. **Reset block VERIFIED working**: `Page.addScriptToExecuteOnNewDocument` hook (cross-navigation persistent) 捕获 `localStorage.removeItem` for ALL `rg-remote-*` keys at `ts=1790225075798`。
3. **Stale UUIDs re-appear within 150ms** — write sequence 捕获:
   - `rg-remote-sentence-buffer` SET "0"
   - `rg-remote-debug-state` SET
   - `rg-remote-tree-seen` SET `["d256f61c-3135-4a3a-9446-5bbd12ba28f9"]` ← STALE
   - `rg-remote-pane-map` SET `{"d256f61c-…":"b3884a34-…"}` ← STALE
   - `rg-remote-active-ws` SET `d256f61c-…` ← STALE
4. **Kernel has FRESH UUIDs**: `artifacts/release/avd-visual/host-trace.log` 末行 `2026-09-24T04:04:59Z pane=1c19a308 lease=114960a5 ev=poll_timeout`。`workspace-graph.json` 含 fresh `8337f72f-2460-477a-9a55-3874e02ad482` / `1c19a308-c477-4c47-a904-284ff57e2520`。Host trace `04:04:55Z fullPn=1c19a308 fullWs=8337f72f` 证实 SPA 已收 fresh UUIDs。
5. **Canvas error path**: TerminalCanvas screencap 仍显 `REMOTE_RESIZE_FAILED: pane b3884a34-fe43-4ec7-ba22-03268ce146ed does not belong to workspace d256f61c-3135-4a3a-9446-5bbd12ba28f9`。canvas 像素层未绘 marker。

### 源 (mystery, evidence-based; paused per /goal rule)

Reset 净后 ~150ms 仍写回 stale UUIDs,候选路径:
- `treeState.svelte.ts:43 setTreeStorageScope()` 在 reset 前已读 LS stale 进 `treeState.seen`,`seedActiveWorkspace(stale_id)` 写回 — 需量 WorkspaceTree.svelte:49 `untrack(() => setTreeStorageScope(scope))` 与 MainApp reset block mount 顺序
- `MainApp.svelte:1142-1145` `lastActivePanePerWorkspace.set(workspaceId, pid); persistPaneMap(); persistActiveWs(workspaceId)` 在首个 pane-switch effect 触发时,把 switch 前的 $state 写回 LS
- TanStack Query cache key `remoteQueryKeys.workspaces(sessionId())` — `sessionId()` 每 WS 对象独立,但 `cacheScope` 稳定,跨 mount 复用 key 时可能 serve stale

三条路径单独皆无法 100% 解释 trace 实测顺序;按用户硬约束 "同因两次无新证据则报告一次并暂停",停止探查并报告。

## AVD_VISIBLE_MARKER — **NOT PASS**

canvas 仍挂 stale pane ref + AVD SwiftShader 渲染双 block。无 marker 像素证据。

## IME_REDRAW — **NOT RUN**

canvas 未绘,IME open/close mount diff 无法量;binding generation / scissor / context/device 对比无基线。

## DESKTOP_WEB_REGRESSION — **NOT RUN**

桌面 host (LAN port 1527) 未启;用户的 installed ridge service per memory rule 不操作。

## RESOURCE_STATUS (本轮)

- AVD `emulator-5554` Pixel_9_Pro_XL RUNNING (headless swiftshader_indirect 1344×2992)
- Isolated dev host pid=28304 listening `0.0.0.0:9620` TLS (started 2026-09-24 12:06Z)
- Kernel pid=27124 listening 127.0.0.1:10605 (started 2026-09-24 11:20Z)
- 第三个 ridge pid=10504 (前会话残留 2026-09-24 12:06Z) — 不影响,单独 process group
- TOTP seed `<redacted: production default identity secret — do not re-print>`;本会话接受 ±1 RFC 6238
- Test data dir: `artifacts/release/avd-visual/fresh-data/` (kernel isolated)

## BETA_READY

**NO.**

代码侧 (`kernel_host_impl.rs` refresh-once-on-failure) 已 narrow fix,4/4 新回归测 + 21/21 既有 kernel_host 测 PASS。Wire 契约未动、Kernel 唯一 PTY 所有权保留、跨工作区检查未放宽、未吞 resize 错误、未重建 PTY、未 blanket-kill、未复用 `ridge.exe.old` 验证新源码。

但 **PTY_EXECUTION ≠ AVD_VISIBLE_MARKER**:kernel scrollback 含 marker 只证明执行链;visual canvas 像素未绘、CLIENT_MODEL 链未证、IME_REDRAW 未证。Per /goal,不能将服务端 scrollback 当作 canvas PASS。

Diff 总结 (本会话 landed):
- `packages/ridge-cli/src/kernel_host_impl.rs`:
  - `KernelSnapshot` 加 `#[derive(Clone)]` (test fixture 需要)
  - `pane_id()` 重构 → `resolve_pane_id_with_refresh(args, snapshot, refresh)` helper
  - 4 新 `#[test]`: refresh_resolves / refresh_still_rejects_cross_workspace / refresh_skipped_when_no_workspace / refresh_then_ab_workspace_replay
- `packages/ridge-kernel/src/domain.rs`: `domain_pty_create` 加 `tracing::info!` 入口/seed/spawn-ok/spawn-fail 4 点 (debug-only,无 wire 变化)

Build provenance:
- `target/debug/ridge.exe` SHA-256 `331089ad3bbf1b97a54489da10adf34839e7a48e168f8694f02cf307f460e4bc`
- isolated data dir `C:\code\wind\artifacts\release\avd-visual\fresh-data\` (本会话;上轮的 `target\debug\avd-pty-marker-data\` 弃用)
- dev host (本会话) pid 启动 2026-09-24 01:34:50Z, kernel pid=25324 listening 127.0.0.1:13419
- TOTP=126951 accepted (counter 59672847; seed `<redacted: production default identity secret — do not re-print>`)
- AVD `emulator-5554` Pixel_9_Pro_XL running, **active kernel pane** `17d06ed2-ec9c-4166-9ecd-a581970ca3fc` (recv_input → after_write ok=true → poll_out bytes=83/53/19/254/9/73/9)
- **Stale pane ref on canvas** `b3884a34-fe43-4ec7-ba22-03268ce146ed` / `d256f61c-3135-4a3a-9446-5bbd12ba28f9` (不同 host session 残留;`force-stop`/`am start` 未清)
- Trace log: `artifacts/release/avd-visual/host-trace.log` (80 行,含 scroll/list_seq/attach/recv_input/after_write/poll_out)

Last blocker status: **PTY_EXECUTION PASS (pane `17d06ed2-…` data path proven via host trace); CLIENT_MODEL / AVD_VISIBLE_MARKER / IME_REDRAW 全 NOT VERIFIED;canvas 仍黑屏(stale pane ref + AVD SwiftShader 渲染)**。Per /goal 同因两次无新证据则报告一次并暂停:本次 run 已提供新证据(实际工作的 pane UUID 与 canvas 显示的 UUID 不一致),确认 bug 路径为**客户端 SPA 缓存 pane ref**而非 kernel 归属问题,故可继续报告而非循环 check-in。