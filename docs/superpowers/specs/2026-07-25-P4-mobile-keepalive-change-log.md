# P4 手机多 kernel 保活 —— 盲改改动/风险台账

> 用户已授权「本轮盲改 P4(仅 svelte-check,不跑 app)」,并要求记录改动点与风险点,
> **万一返工能快速定位**。本档随改随记:每改一处即登记「文件:符号 · 改了什么 · 为何 ·
> 风险 · 回退」。P4 全部 commit 前缀 `feat(remote-mobile): P4`,便于整段 revert。

## 背景

手机 SPA(`src/remote/`)此前用**单例 kernel**:`terminalController.ts`(一个 wasm
`TerminalKernel` + 一块 canvas)+ `paneScrollbackCache.ts`(≤256KB 旁路字节缓存)+
`MainApp.svelte` 的 `resetForSwitch`/`reconcileReplay`/prune 逻辑。切 pane 要清屏重放
→ 白屏、scrollback 脆弱、与桌面渲染核心割裂。

P4 = 改用共享 `@ridge/remote/shared/terminal/manager.ts`(多 kernel + 单 host canvas +
scissor + keep-alive),与桌面 `RidgePane` 同一份。切 pane 只挪 scissor,不 reset/重放/
销毁 → 零白屏;pane 历史活在各自常驻 kernel,旁路缓存退役。

## 无法 headless 验证的风险点(返工时优先核查这些运行时行为)

| # | 风险 | 只有跑 app 才能验 | 若返工,先查 |
|---|---|---|---|
| R1 | 切 pane 白屏/闪烁 | scissor 是否只渲染 active、切换是否零帧空屏 | manager.attach 时 container 尺寸/可见性;active 切换是否触发 RAF 重渲 |
| R2 | scrollback 保真 | 切走→大量输出→切回→上滚 历史是否完整 | 后台 pane kernel 是否仍订阅/feed;LRU 是否误逐出 |
| R3 | 软键盘 offset | `visualViewport` 监听 + keyboardOffset 是否仍作用于新 host canvas | 输入适配层是否正确读 active pane 容器 |
| R4 | IME 组字 | 隐藏 textarea preedit → manager.setPreedit(activePaneId) | preedit 目标 pane 是否为 active |
| R5 | 选择即鼠标(TUI 接管) | 全屏 TUI 里 touch 选择是否仍 encodeMouse 转发 | 适配层是否对 active pane kernel 调 isMouseReporting/encodeMouse |
| R6 | 内存/LRU(低端机) | 开 N+2 pane 是否 OOM、逐出是否生效 | LRU 上限 N、冻结/rehydrate 路径 |
| R7 | 弱网重连 | 断线重连是否黑屏/丢历史 | reconnect 后是否 re-subscribe active pane、kernel 是否保活 |
| R8 | 复制 pill(前修 b5c5a56) | 复制不唤起键盘、TUI 选择归属 | 适配层迁移后 copy pill 事件是否仍 stopPropagation |

## 改动点登记

> 格式:`文件:符号` — 改动 · 原因

### P4 保活(手机单→多 kernel,park/unpark 单容器 keyed-remount 镜像 RidgePane)
- `src/remote/lib/TerminalCanvas.svelte` — 整体重写:弃单例 `TerminalController`,改 `TerminalManager.instance()`;新增 `workspaceId` prop;`onMount` attach/unpark + 注册 `onData/onResize` + `setFocused` + `fitPaneNow`;`onDestroy` `park`(kernel 存活);触屏/鼠标/键盘/IME/选区/copy-pill/软键盘 offset 从 `ctrl.*` 转 `manager.*(paneId)`/`getKernel(paneId)?.*`;新增 §pointer-neutralize(capture-stop manager 自带 pointer 监听);删 `<canvas>` 元素(manager 自建)。
- `src/remote/MainApp.svelte` — 删 `PaneScrollbackCache`+整块缓存/sessionStorage 镜像/reconcile;`pruneDeadPanes`/`pruneCachesForClosedWorkspaces` 改 detach kernel;`onRawBytes` 简化 `if(pid===active) feedUtf8`;模板 `{#key activePaneId}` 包 `TerminalCanvas`+传 `workspaceId`。
- `src/remote/main.ts` — 新增 `TerminalManager.setHostPorts({settings})`(动态 import,不入 entry bundle)。
- `vite.remote.config.js` — manualChunks 删已删文件死字符串。
- **删除**:`terminalController.ts`+`.test.ts`、`paneScrollbackCache.ts`+`.test.ts`。

### RIS-vs-保活冲突及其解法(resume 无RIS 续订 + 后端增量 replay 基础)
- **冲突**:本轮后端令 host 每次 subscribe 发 `RIS+resync`;手机单订阅切回重订阅 → RIS 清空保活 kernel → P4 保全量 scrollback 失效、闪跳、模式亦被抹。桌面因 ptyBridge 长存订阅不重订阅故无此症。
- **解**:`resume` 语义——控制端切回已 full-resync 过的 pane 时带 `resume:true`,host 跳过 RIS resync,仅续 live 流,保活 kernel 全量历史不清。首视/重载/重连仍 full resync。
- `packages/ridge-remote/src/pane.rs`(已 commit)— 帧/resync SSOT(增量帧即 `pane_frame(since_bytes)`,无 RIS)。
- `src-tauri/src/state.rs:get_pty_scrollback_since` — 前向增量读(镜像 rdg `ScrollbackRing::since`),`start_seq==cursor` 判无 gap;增量 replay 前向兼容基础(sinceSeq,前端下轮激活)。
- `src-tauri/src/remote_host_impl.rs`(subscribe-pane 臂)— parse `resume`/`sinceSeq`:sinceSeq→增量(无RIS,gap 回退 resync);resume→live-only(不发帧);否则→full resync。meta 加 `headSeq`/`incremental`。
- `packages/ridge-cli/src/tui/lan_host_impl.rs`(subscribe-pane)— parse `resume`,跳过 resync 回放(live-only)。
- `src-tauri/src/commands/terminal.rs:get_pane_resync_preamble` + lib.rs 注册 + remote_host_impl `REMOTE_ALLOWLIST` — 新命令返模式前导(复用 `Modes::to_reattach_preamble` SSOT),供 cloud 首订阅补前导。
- `packages/remote/src/shared/transport/wsRemote.ts:subscribePane` — 接口+impl 加 `opts?:{resume?,sinceSeq?}`,入订阅消息。
- `src/remote/lib/cloudRemote.ts:_subscribe` — 加 `resume` 参:首订阅 `RIS+前导+tail`(补 `get_pane_resync_preamble` 修 cloud TUI 鼠标),resume 时跳过 RIS 自建。
- `src/remote/MainApp.svelte:replayedPanes` — 已 full-resync 且在同步的 pane 集;切回传 `resume`;reconnect 清空(断线有 gap);detach 移除。

### 新发现且已修的 cloud 缺口(非 resume)
- **cloud 首订阅缺模式前导**:cloud 初次订阅是 `cloudRemote._subscribe` 前端自建 `'\x1bc'+tail`,**无前导**→ 上报的手机公网 TUI 鼠标 bug 于此路径未被后端 cloud_pane.rs 修复覆盖。已由上条 `get_pane_resync_preamble` + cloudRemote 前导补齐。

## 回退预案

- P4 各 commit 前缀 `feat(remote-mobile): P4`;整段返工:`git revert <P4 commits>` 或
  `git reset --hard <P4 之前 SHA>`(P4 之前最后一个 commit = 后端收敛完成点,见下)。
- **P4 之前基线 SHA**:`b9031a0`(fix(rdg): live-modes tracking …)——后端收敛全绿、
  上报 bug 已修。P4 全部回退到此仍是可发布的完好版本。
- 退役文件(`terminalController.ts`/`paneScrollbackCache.ts` 及其 `.test.ts`)在 P4 期间
  **先保留、后删**:先接 manager 跑通类型,末阶段再删,便于对照/回退。

## 验证清单(待有 app 环境时补跑)

- P4 前基线(`pnpm check`,commit c0f28e4):**4582 files · 0 ERRORS · 2 WARNINGS**
  (既有 a11y 警告,非 P4)。∴ P4 门禁 = svelte-check 维持 0 ERRORS。
- [ ] `pnpm svelte-check` 0 err(盲改阶段唯一门禁)
- [ ] `pnpm vitest run` 相关纯逻辑测试绿
- [ ] (app)R1–R8 逐项真机/CDP 验(见上表)
