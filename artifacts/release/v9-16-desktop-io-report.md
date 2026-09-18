# v9-16 Desktop Web Remote IO 闭环调查报告

- 日期（UTC）：2026-09-18
- HEAD：`02c732af`，工作树干净；`stc validate` → VALID（仅 3 条 pre-existing `CHANGE_E_EMPTY` WARNING）
- 范围：只修 Desktop Web Remote（`?ui=desktop`）输入输出闭环；未动 mobile 已通过路径；未碰 Native Tauri Remote
- 本轮仓库零写入（调查只读 + 临时复现测试创建→执行→删除，未留痕）

---

## DESKTOP_INPUT_PATH

**PASS（非断点）。** 链条逐跳验证：

- `src/lib/components/RidgePane.svelte:1327` — 本地路径 `invoke('write_to_pty', {workspaceId, paneId, data})`，经 `src/lib/transport/tauriShim/core.ts:56` → `bridge.invoke` → `RpcClient.request` → `packages/remote/src/shared/transport/lanWsAdapter.ts:205-213` `toWire` 转为 `{"type":"invoke-request","cmd":"write_to_pty","args":{...},"_reqId"}`。
- Host 侧 `packages/ridge-cli/src/kernel_host_impl.rs:583-591` 的 `invoke-request` 分支直接 `dispatch(cmd, args)`，`718-723` 的 `write_to_pty` 用 `pane_id()`（`220-265`，接受 `paneId`/`workspaceId`）+ `data` 写 PTY。形状完全对得上。
- 临时复现测试（真实 `RemoteConnection`+`LanWsAdapter`，仅 socket 边界伪造，跑后已删）断言 `toWire` 输出与上述 envelope **逐字节一致**，4/4 PASS。
- 旧结论 "`ptyFrameCount=0` ⇒ 输入没发出去"是测试正则误报：提交版 E2E 只匹配 `"method":"write_to_pty"`（JSON-RPC 原生形），而 LAN 腿 desktop/mobile 实际发送的是 `"cmd":"write_to_pty"`（`invoke-request` 形），计数恒为 0。v9-15 §12.1 的"wire shape 实证"在此点上是错的。
- 关于"mobile 用 0x11、desktop 不一致"：0x11（`encodeJsonFrame`，`packages/remote/src/shared/transport/cloudMux.ts:71`）只属于 **cloud/WebRTC 腿**。LAN 腿上 mobile（`paneRpcScheduler.ts:481` → 同一 `toWire`）与 desktop 发的是**同一种** `invoke-request`。不存在 LAN 输入形状分歧。

## DESKTOP_OUTPUT_PATH

**FAIL（断点，100% 确定性复现）。** 根因：

- Desktop 输出订阅：`ensurePtyBridge`（`packages/remote/src/shared/terminal/ptyBridge.ts:118`）→ `listen('pty-output-{ws}-{pane}')` + `invoke('register_pane_delta_channel')` → `core.ts:44-50` 转为 `bridge.subscribePane` → `bridge.ts:192-196` 只做 `rpc.notify('subscribe-pane', …)`。
- `notify` 经 `toWire` 变成 `{type:'subscribe-pane',…}` 发出 —— **host 侧正常**（`kernel_host_impl.rs:595-598` 启动 `start_subscription`，二进制 `pane_frame`（16B UUID + 原始字节，`packages/ridge-remote/src/pane.rs:36`）照常下发）。
- 但客户端 `RemoteConnection._handleBinaryMessage`（`wsRemote.ts:947-972`）要求 `paneKeysById` 命中且 size==1，否则静默丢帧。而 `_setPaneRef` **全仓仅一处调用**：`wsRemote.ts:1466` 的 `subscribePane()`（grep 核实仅 2 命中：定义 + 该调用）。bridge 的 `rpc.notify` 路径从不经过它 → 注册表恒空 → **全部二进制输出帧被丢弃** → `RidgePane` 经 `dispatchRawBytes`（`bridge.ts:221`）永远收不到字节 → 页面永远显示不出 marker。
- Mobile 走 `ws.subscribePane`（`MainApp.svelte:961` 经 `subscribePane` 注册），故正常 —— 这就是"mobile IO 正常、desktop IO PARTIAL"的完整解释。
- 复现证据（临时测试，真实产品代码，4/4 PASS 后已删）：mobile oracle（`conn.subscribePane` 后注入 host 格式二进制帧）收到 marker；desktop 路径（`bridge.attach` + `bridge.listen` + `bridge.subscribePane` 后注入**同一帧**）`received.length===0`，且抓到发出的 `{type:'subscribe-pane'}`（host 侧无辜）。
- `RidgePane.svelte:1786` 的 `!alive || !attached` 门和 `kernel_host_impl.rs:897` 的 4 个缺失方法（`list_workspace_save_info`/`get_shell_history`/`set_user_default_cwd`/`start_watching_paths`）经核查**均非阻塞**：前者在 `manager.attach` 后即置 `attached=true`（`1513`）；后者调用方全带 `.catch`（`+page.svelte:1401`、`fileWatcherSync.ts:97`），且 `hasCapability` 降级为灰显面板。`tauriShim` 的 `write_to_pty` 也不缺失（走通用 `bridge.invoke`）。不建议本轮碰这三处。

## DESKTOP_IO_E2E

**NOT_RUN（browser）/ 输出断点已由传输层复现覆盖。**

- 真实页面 E2E（`scripts/browser-ui-e2e.mjs` 提交版）本次执行在 CA 安装步超时（7min）：Playwright 启动后卡在 `certutil -user -addstore Root`，属 v25/v26 已记录的环境 flake（REMOTE-RESUME §12.1），与代码无关。候选 host 当时已正常起（`ridge host ready … kernel pid=25304`，TOTP 捕获成功），无残留候选进程（仅生产目录 2 个 ridge 进程，未触）。
- 附带发现：工作树遗留一份未提交的 E2E 改写（含 `extractSentData`/`echoedBack` 改进意图）**语法已坏**（`node --check` 报 `530行 Unexpected token 'else'`），无法执行。已将其备份至 `C:\Users\12867\AppData\Local\Temp\opencode\browser-ui-e2e.working.mjs` + `diff.txt`，工作树恢复到提交版干净状态。它的 `cmd`/`method` 双形状计数方向是对的，应纳入新 CHG 重做（见审批项）。
- 传输层复现（含 rapid A→B→A 路由正确性：注册后 B/A 各回各 pane、零串扰；desktop 路径两帧全丢——同根因）4/4 PASS。未用 mock output、未注入 UI、未绕 transport：注入的是 host 真实 `pane_frame` 二进制格式，走真实 `_handleBinaryMessage → onRawBytes → bridge.dispatchRawBytes` 全链。

## MOBILE_REGRESSION

**保持。** `pnpm test`：2082 passed / 17 skipped；4 个 mobile 直接相关文件 128/128 PASS（`TerminalCanvas` 46、`cloudRemote` 61、`mobileTouchScroll` 7、`wsRemote.behavior` 14）。唯一失败是 `scripts/stc-walker.test.mjs` 的 suite 收集错误（"No test suite found"，HEAD 提交即有，与本轮无关的 pre-existing 基建问题）。mobile 共享路径（`paneScheduler → invoke-request → 16B 二进制输出`）零改动。

## CHANGED_FILES

**无。** 本轮对仓库零写入：产品代码、脚本、文档、`REMOTE-RESUME.md`、`.spectree/` 均未动（`git status` 干净，`stc validate` → VALID，仅 3 条 pre-existing `CHANGE_E_EMPTY` WARNING）。临时复现测试创建→执行→删除，未留痕。按 AGENTS.md §3，任何超出当前锁 `allowedPaths`（仅测试文件集合，不含 bridge/adapter/脚本/E2E）的编辑都需新锁，故修复与 E2E 补强均未擅自实施（见审批项）。

## CANDIDATE_HASHES

同一 checkpoint（`src/src-tauri/packages/static` 自 `875a791e` 零 diff，无需重建），本轮实测 `print-candidate-provenance.mjs`：

- HEAD `02c732af`，工作树干净
- `target/test-rdg/release/ridge.exe` 42595328 bytes，sha256 `eaf2310d06db0a01b6e65340c0b533c87570f3be495cb148b1d82531b591eeb9`
- `remote-dist/desktop/index.html` sha256 `a6e8caba5554fa03ff6fa7d977d2f67182f2476d28279ea5adc723902a3b3354`
- `remote-dist/mobile/index.html` sha256 `7014c0ac76039f4bce1ddb1e3d48f03b69eecb10f197bc40e63a1e9265757b87`
- `remote-dist/mobile/sw.js` sha256 `b7ab72ddf094d055768ce5d862f765012e2f83eaec1f782115b626cb770d8f13`
- 产品版本 0.1.86（4 处一致），`ridge --version` 0.1.0 系 cargo per-crate 惯例（v9-13 口径，非漂移）

## REMAINING_GAPS

1. **产品修复（待审批，未实施）**：最小 shim，复用 mobile 契约、不新增 transport、不改 L2 线形（`subscribe-pane`/`invoke-request`/16B 二进制帧全部原样）：让 `LanWsAdapter` 在 `sendControl` 收到 `subscribe-pane` notification 时同步走 `conn.subscribePane()` 的注册语义（或等价地给 `ChannelTransport` 加注册钩子由 bridge 调用）。候选文件二选一：`packages/remote/src/shared/transport/lanWsAdapter.ts` 或 `src/lib/transport/tauriShim/bridge.ts`。预计 <20 行。**不动** `kernel_host_impl.rs:897`（4 方法非 IO 阻塞）、**不动** `RidgePane.svelte:1786`、不改 Native Tauri 路径。
2. **E2E 补强（待审批）**：修复并落地备份中的 E2E 改进（`cmd`+`method` 双形状计数、`echoedBack` 输出断言），再加页面显示断言（读 `TerminalManager` 内核文本含 marker）、resize、detach/reconnect 后二次 IO、rapid A→B→A 归属。无 mock、无 UI 注入。
3. **审批问题**：上述 1+2 触及锁外路径，需新 CHG + `stc lock/build/verify`。是否触"已冻结 L2 协议契约"？**否**——线形零改动，属 L1 适配层 bugfix。但仍需走锁（allowedPaths 限制），故停下请批，未跑 `stc apply --confirm` / 未跑 `stc complete`（无授权改动可吸收；CHG-031 的 pending proposal 原样保留，`stc next` 仍为 `stc apply CHG-031 --confirm`）。
4. 真机六类 + Native Tauri Remote E2E 维持 NOT_RUN/独立。
5. `REMOTE-RESUME.md` 本轮未更新（锁外，待修复 CHG 落地后补）。

## BETA_READY

**NO。** desktop 输出断点已定位到单行级根因并确定性复现，但修复未授权落地、真实页面 E2E 未重跑（浏览器 CA flake + E2E 待重做）。`stc complete` 本轮有意未执行（无授权改动；不擅自关闭 CHG-031）。

**请批（择一回复即可）**：A. 批准立新 CHG（scope：`lanWsAdapter.ts` 或 `bridge.ts` 单文件 shim + `scripts/browser-ui-e2e.mjs` E2E 补强），按 `propose → lock → build → verify → complete` 全流程走；B. 指定 shim 落点文件；C. 暂不修，保持 PARTIAL 现状。
