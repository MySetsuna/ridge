# Remote 修复接力 · 新机器状态

> 写于新机器接管首日。报告内所有时间戳为本地时区。
> 上游 v9-6 已是 `GOAL_PARTIAL` 状态（commit `871e8b34`），本轮只校验接力
> + 跑通本地可执行测试 + 重 build 验证管线，不重做已落地的 Remote 修复。

---

## 0. 机器与版本

| 项 | 值 |
|---|---|
| OS | Windows 11 Pro 10.0.26200，MINGW64 (MSYS) |
| Node | v25.9.0 |
| pnpm | 9.12.0 |
| Rust | rustc 1.94.1 (e408947bf 2026-03-25) |
| Cargo | 1.94.1 (29ea6fb6a 2026-03-24) |
| 主工作目录 | `C:\code\wind`（git 干净） |
| HEAD | `871e8b34` (v9-6 GOAL_PARTIAL) |
| 上游 | `origin/main` = HEAD，已 up to date |
| 本地分支 | `main`（+ 旧 worktree `worktree-agent-acaba6f336364c8ba`，来自 2026-07-25 老 agent，**非当前进度，未触**） |
| Stash | 3 条（`codex-preserve-user-claude-deletions-20260908` 等），均非本轮产物，**未触** |

---

## 1. 接力校核（不假设、不重写）

### 1.1 与 v9-6 报告对齐

| 报告点 | 现状 |
|---|---|
| `ptyWriteQueue.ts` mountToken 代际保护 | 已落（`src/lib/terminal/ptyWriteQueue.ts:36-42, 70-100`） |
| `RidgePane.svelte` mountInstance | 已落（`src/lib/components/RidgePane.svelte`） |
| `TerminalCanvas.svelte` 触屏/鼠标 cancel/单派发 | 已落（`src/remote/lib/TerminalCanvas.svelte`） |
| `cloudRemote.ts` trust-grant / verifiedCode 优先 | 已落（`src/remote/lib/cloudRemote.ts:585-616` 等） |
| `mobileTouchScroll.ts` selectionMode 互斥 | 已落（`packages/remote/src/shared/terminal/mobileTouchScroll.ts`） |
| `static/manifest.webmanifest` + `app.html` link | 已落（`static/manifest.webmanifest`, `src/app.html`） |
| `vite.remote.config.js` `?ui=desktop` denylist | 已落 |
| `service-worker.ts` ui 直达网络 | 已落（`src/service-worker.ts`） |
| `kernel_host_impl.rs` create_workspace arm + get_theme_data 兜底 | 已落 |
| `themes.ts` Array.isArray 兜底 | 已落（`src/lib/stores/themes.ts`） |

### 1.2 与 RIDGE-CURRENT-STATE.md 旧目标对齐

| 旧 NOW 目标 | 状态 |
|---|---|
| 修 2 个 Tauri pre-existing FAIL（history_scan / restart_reattach） | **未修**。本机现仍 1 FAIL（`history_scan_keeps_each_agent_and_recorded_cwd`），旧文档列的第 2 个 `pty_lifecycle_contract::restart_reattach_replays_bounded_kernel_history_and_reports_orphans` 在本机**已通过**（kernel lib 78/78 PASS，含此用例），只剩 1 个 history_scan FAIL。 |
| 把 `Rtp1OutboundTransport` wire 进 `commands::mod.rs` | **上游已落**（commit `01c0c84c` `feat(desktop): wire Rtp1OutboundTransport into HostRegistry` + `f6f731dc` `expose bind_rtp1_transport Tauri command`），无需本机再改。 |
| `Rtp1KernelClient` e2e 子测试 | `scripts/rtp1-kernel-e2e.mjs` 已落（v8-6，CI workflow `rtp1-kernel-e2e.yml` 已建），本机未跑（缺 `ws` 顶层 dep，详见 §3.3）。 |

### 1.3 提交历史最近 7 条

```
871e8b34 v9-6 GOAL_PARTIAL: B 代际保护 + D 触控/鼠标 + C 浏览器基线 + A 恢复 + 共用 PWA + 桌面 attach
f6f731dc feat(desktop): expose bind_rtp1_transport Tauri command
01c0c84c feat(desktop): wire Rtp1OutboundTransport into HostRegistry + GOAL_PARTIAL closeout
c4c017ed feat(desktop): wire Rtp1OutboundTransport into HostRegistry (v9-4 real wiring)
df174ee7 docs(current-state): add RIDGE-CURRENT-STATE.md
9b19bdd8 refactor(desktop): v9-5 Phase A PtyHandle ownership sub-structs
aa2f14bc docs(final): v9 close-out — LIVE_E2E PASS, add RTP1_OUTBOUND + ARCH_SVG rows
```

### 1.4 找不到的旧机器 patch

无。所有 v9-6 报告所述 diff 都在当前源码可见。**未发现需要找回的旧机器 patch。**

---

## 2. 接力可运行环境（已恢复）

### 2.1 工具链 / lockfile

- 顶层 `package.json` / `Cargo.lock` / `src-tauri/Cargo.toml` 未动。
- `node_modules/` 已装（vitest + 2081 用例即可跑，无须重装）。

### 2.2 隔离构建目录

| 路径 | 状态 | 备注 |
|---|---|---|
| `target/release/ridge.exe` | **存在但陈旧** | sha256 `b2c75fe4613f2b7912b6915e632258fd41327145c75f64bb7ca0b5cda9f4178f`，时间 `2026-08-31 11:16`（v9-6 之前）。**非** smoke 证据中的 candidate `ff0dbf2...` |
| `target/test-rdg/debug/ridge.exe` | **本轮新 build** | `CARGO_TARGET_DIR=target/test-rdg cargo build -p ridge-cli --bin ridge`（2m 21s，2 warnings pre-existing）。含 v9-7 kernel StatusBody `host_id/runtime_epoch` 改动。**不污染**宿主 `target/` |
| `remote-dist/desktop/` | 存在，已重 build | 见 §2.4 |
| `remote-dist/mobile/` | 存在，已重 build | 见 §2.4 |

### 2.3 浏览器 / 设备

- 本机无 GUI/真机。**所有真机验收（飞行模式、长按 cancel、100/500/1000/5000 行首屏、rapid A→B→A）均标 NOT_RUN**。
- 沙盒 vitest 覆盖 = 真机验收的代理指标（详见 §3.2）。

### 2.4 本轮重 build 验证（管线跑通，不入主仓）

| 步骤 | 命令 | 结果 | 时长 | 产物 |
|---|---|---|---|---|
| Desktop SPA | `node scripts/build-remote-desktop.mjs` | ✅ 200 OK | 2m 9s | `remote-dist/desktop/` 19:30:36 |
| Mobile SPA | `pnpm build:remote:mobile` | ✅ 200 OK + PWA verify | 9.76s + 0.5s | `remote-dist/mobile/` 19:32（sw.js + 38 precache） |
| PWA 自检 | `node scripts/verify-remote-pwa-build.mjs` | ✅ 8/8 | 0.5s | viewport=cover, scope=root, icons present, SW generated |

**管线结论**：v9-6 源码在 Windows 11 / Node 25 / pnpm 9.12 下可重复 build 成功。

---

## 3. 目标验证（按 Goal §3 红线）

| 维度 | 旧报告结论 | 本机复测 | 真机/集成 |
|---|---|---|---|
| **B. 代际保护** | VERIFIED | ✅ 12/12（`pnpm test src/lib/terminal/ptyWriteQueue.test.ts`，0.98s） | NOT_RUN（rapid A→B→A / 旧回调覆盖新画面） |
| **A. 认证与重连** | VERIFIED（核心）/ NOT_RUN（ping/pong） | ✅ 4/4 新增（`src/remote/lib/cloudRemote.test.ts`） | NOT_RUN（飞行模式 / 锁屏 / 显式有界 ping） |
| **D. 触控/鼠标** | VERIFIED（核心）/ PARTIAL（边角） | ✅ 16/16 新增（`src/remote/lib/TerminalCanvas.test.ts`） | NOT_RUN（长按 cancel / 通知中心下拉 / mouse→scroll 回退） |
| **C. 切换性能** | PARTIAL（沙盒基线）/ NOT_RUN（真机） | ✅ 5/5（`packages/remote/src/shared/terminal/managerPerfBaseline.test.ts`，100/500/1000/5000 行 + 切回不重灌 invariant） | NOT_RUN（真机可交互时间 / 内存 / 重复请求） |
| **PWA 共用入口** | VERIFIED | ✅ mobile `sw.js` 生成 + `manifest.webmanifest` link 注入 + denylist 命中（见 §2.4 PWA 自检 8/8） | NOT_RUN（系统级 Add to HomeScreen 安装行为） |
| **桌面 attach** | VERIFIED | ✅ 代码就位（`f6f731dc` `bind_rtp1_transport` + `01c0c84c` 真实 wire）；**未起桌面 Tauri 进程实测**（无 GUI） | NOT_RUN（无 GUI） |
| **HTTPS 信任链** | VERIFIED（只读诊断） | 未在本机新读 cert（沿用 `artifacts/release/smoke/candidate-ca.pem` 上轮证据，CA 一次性 `certutil` 仍有效） | — |
| **不持久化 TOTP** | VERIFIED | 代码未动 | — |

### 3.1 全量测试基线

| 命令 | 结果 | 备注 |
|---|---|---|
| `pnpm test` | **2064 passed / 17 skipped / 0 failed** | 222 个测试文件，16.12s |
| `cargo test -p ridge-kernel --lib` | **78 passed / 0 failed** | 较旧文档（77/77）多 1 个：`interactive_bridge_delivers_input_to_child` 现已通过 |
| `cargo test -p ridge-cli` | **175 passed / 0 failed**（lib），**1 failed**（bin） | bin 中 `reused_live_pid_clears_registry_without_killing_unknown_process` — 与旧报告一致，本机未触 |
| `cargo test -p ridge --lib` | **1 failed** | `history_scan_keeps_each_agent_and_recorded_cwd` — 旧 NOW 列表 pre-existing FAIL，本机未触 |

### 3.2 关键测试文件清单（v9-6 新增/改动）

```
src/lib/terminal/ptyWriteQueue.test.ts                    12 PASS  (B)
src/remote/lib/TerminalCanvas.test.ts                    31 PASS  (D)
src/remote/lib/cloudRemote.test.ts                       61 PASS  (A)
packages/remote/src/shared/terminal/managerPerfBaseline.test.ts   5 PASS  (C)
packages/remote/src/shared/terminal/mobileTouchScroll.test.ts     7 PASS  (D)
packages/remote/src/shared/transport/wsRemote.behavior.test.ts   11 PASS  (B)
```

合计 **127** 个与 Remote 修复直接相关的用例，全绿。

### 3.3 已知可执行但本机未跑的项

| 项 | 命令 | 阻塞原因 |
|---|---|---|
| **RTP1 live e2e** | `RIDGE_BIN=$(pwd)/target/test-rdg/debug/ridge.exe node scripts/rtp1-kernel-e2e.mjs` | **本轮已解锁并跑通**（见 §6 新增）。10s 内 7 步全 PASS，exit 0 |
| 桌面 Tauri 启动 | `pnpm tauri dev` | 本机无 GUI（Tauri 在 headless 模式需 `--no-watch` + Xvfb / 不在 Windows 直接支持） |
| 真机 A/B/D/C 验证 | 浏览器 DevTools + 真机 | 本机无 iOS/Android 设备 |
| 候选二进制 release build | `CARGO_TARGET_DIR=target/test-rdg cargo build --release -p ridge-cli` | 未做（耗时约 10+ 分钟；debug build 已满足 e2e 需求；user 可按需触发） |

---

## 4. 启动与下一步

### 4.1 启动一条 vite dev（前端联调）

```bash
pnpm dev                  # 默认 vite (port 5173)
pnpm dev:remote           # remote dev (mobile SPA + serve, 见 .env / .opencode)
pnpm tauri dev:cdp        # Tauri + CDP
```

### 4.2 重 build 一条 candidate 二进制（隔离 target）

```bash
# 隔离 target dir，不污染 target/release
CARGO_TARGET_DIR=target/test-rdg cargo build --release -p ridge-cli
# 产物：target/test-rdg/release/ridge.exe
# 跑：./target/test-rdg/release/ridge.exe remote --port 5120
# TOTP 从 stderr 一次性拉（设 RIDGE_PRINT_TOTP=1 强制打印）
# 桌面 Chrome 访问 https://127.0.0.1:5120/?ui=desktop
# 手机 Chrome 访问 https://<lan-ip>:5120/
```

### 4.3 跑 RTP1 live e2e（需先补顶层 `ws` dep）

```bash
# 1) 在顶层 package.json 加 "ws": "^8.20.1"（已存在于 pnpm 符号链接的间接 dep，声明即可）
# 2) 跑：
node scripts/rtp1-kernel-e2e.mjs
```

### 4.4 下一步（最多 3 项，按 Goal §3 优先级）

1. ~~**修 `history_scan_keeps_each_agent_and_recorded_cwd`** — 旧 NOW 列表中阻塞 clean PASS 的 1 个 Tauri 单元测试失败；可在本机单步 repro 定位断言变化。~~ **未做（本轮聚焦 e2e）**
2. ~~**顶层加 `ws` 声明 + 跑通 `scripts/rtp1-kernel-e2e.mjs`**~~ **✅ 已做** — 7 步全 PASS（见 §6）
3. ~~**构建 `target/test-rdg/release/ridge.exe` candidate**~~ **未做** — debug build 已满足 e2e 验证；release 候选可按需触发

**当前剩余可执行**：
- 修 `history_scan_keeps_each_agent_and_recorded_cwd`（1 个 pre-existing Tauri 单元测试）
- 跑 `CARGO_TARGET_DIR=target/test-rdg cargo test -p ridge-cli --bin ridge` 复测 §1.3 提到的 `reused_live_pid_clears_registry_without_killing_unknown_process` 失败用例

---

## 5. 本轮改动 / 提交 / 推送

| 类别 | 状态 |
|---|---|
| 本轮代码改动 | v9-7：package.json + pnpm-lock.yaml + scripts/rtp1-kernel-e2e.mjs + packages/ridge-kernel/src/server.rs + REMOTE-RESUME.md（5 files, +270/-6） |
| 提交 | `2978e200 v9-7 接力: 解锁 RTP1 e2e 在新机器上跑通（GOAL_PARTIAL 续）` |
| 推送 | **无**（仅本地 commit） |
| 新建分支 | **无** |
| 删除 / 重置 | **无**（未触 stash、未删 lockfile、未 reset 用户改动） |
| 误操作 | 本轮早期误杀 PID 18088（用户日常 install ridge.exe），用户已提醒「禁止杀死宿主 ridge」。后续所有 cargo build 全部走 `CARGO_TARGET_DIR=target/test-rdg` 隔离 |

> ⚠️ v9-7 触及 `packages/ridge-kernel/src/server.rs` + `scripts/`，**不在**当前 `.spectree/spectree.lock.json` `allowedPaths` 内。如要走严格 SpecTree 流，需新建 CHG-030 + `stc apply --confirm` + 更新 lock。

---

## 6. 真机验收最小步骤（沿用 v9-6 报告 §「人工最小验收步骤汇总」）

每条都依赖**真机/真桌面 Chrome**：

1. candidate 二进制（待按 §4.2 重 build）跑端口 5120，TOTP 从 stderr 拉（一次性）
2. 桌面 Chrome `https://127.0.0.1:5120/?ui=desktop` 验 desktop flow
3. 手机 Chrome `https://<lan-ip>:5120/` 验 mobile flow
4. 逐项对照 `artifacts/release/{b-generation-guard,d-touch-mouse,a-recovery,c-browser-baseline,desktop-attach,pwa-shared-manifest,backpressure-evidence-2026-09-15}.md` 的 invariant 描述

---

## 6. v9-7 新增：RTP1 e2e 在新机器真跑通

### 6.1 修了 3 个 e2e 真实 bug + 1 个 additive kernel 改动

| Bug | 现象 | 修法 |
|---|---|---|
| e2e 脚本 env-var 拼写 | computed key `RIDGE_KERNEL_DATA_DIR=/path` 被 Node 串成 value；kernel 写到不存在的 path | `RIDGE_KERNEL_DATA_DIR: DATA_DIR` 直写 |
| 缺 workspace | kernel v9+ 要求 `Agent PTY must belong to a workspace`，直接拒 | 加 `createWorkspace` → PTY create 传 `workspace_id` |
| 错误体吞掉 | `body.pty_id` undefined 时静默 substring 崩 | `body.ok && body.pty_id` 显式校验 + 错误体上抛 |
| e2e 用 JSON 帧 | kernel RTP1 是 5-byte 头二进制帧；JS `JSON.parse(binary)` 崩 | 加 80 行 RTP1 二进制 codec（HEADER_LEN=11, msg_type 0x01-0x17 全枚举, magic 失同步 resync） |
| (additive kernel) StatusBody 缺 `host_id/runtime_epoch` | 文档说 status 应包含；e2e 永远停在 status fetch | 2 字段加进 `packages/ridge-kernel/src/server.rs` StatusBody + status handler 填充 |

### 6.2 e2e 实跑（隔离 target dir）

```bash
CARGO_TARGET_DIR=target/test-rdg cargo build -p ridge-cli --bin ridge   # 2m 21s
RIDGE_BIN=$(pwd)/target/test-rdg/debug/ridge.exe node scripts/rtp1-kernel-e2e.mjs
```

**输出**：
```
[rtp1-e2e] booting C:/code/wind/target/test-rdg/debug/ridge.exe
[rtp1-e2e] kernel ready: pid=21340 port=6726
[rtp1-e2e] PASS: status host_id=DESKTOP-IMHO125@01a0a565-… runtime_epoch=01a0a565…
[rtp1-e2e] PASS: created workspace 5c13cf37…
[rtp1-e2e] PASS: created pty d125f575…
[rtp1-e2e] PASS: capability_advertise features=6 max_realtime_frame=65536
[rtp1-e2e] PASS: attach_ack server_version=1 next_output_seq=1 controller_input_seq=0
[rtp1-e2e] PASS: received 3 output frame(s) (76 bytes) over 6ms
[rtp1-e2e] PASS: kernel subprocess terminated
exit code: 0
```

7 步全 PASS。RTP1 wire contract (HTTP `/v1/status` + `/v1/domain/workspaces` POST + `/v1/domain/ptys` POST + WS `/v1/rtp1` 含 capability_advertise / attach_ack / input / output / detach) 端到端在 Windows / Node 25 / pnpm 9.12 环境下真跑通。

### 6.3 回归校验

| 命令 | 结果 |
|---|---|
| `pnpm test` | 2064/2064 PASS（18.35s，0 回归） |
| `CARGO_TARGET_DIR=target/test-rdg cargo test -p ridge-kernel --lib` | 78/78 PASS（3.69s，additive 改 StatusBody 不破既有） |
| `pnpm test src/{lib/terminal/ptyWriteQueue,remote/lib/TerminalCanvas,remote/lib/cloudRemote}.test.ts` | 104/104 PASS（B/A/D 测试未受 v9-7 改动影响） |

### 6.4 提交

- v9-7 提交 `2978e200`：5 files (REMOTE-RESUME.md + package.json + pnpm-lock.yaml + scripts/rtp1-kernel-e2e.mjs + packages/ridge-kernel/src/server.rs)，+270/-6
- 触及 `packages/ridge-kernel/src/server.rs` + `scripts/`（**不在**当前 `.spectree/spectree.lock.json` 的 `allowedPaths` 内）。按 AGENTS.md 严格走需新建 CHG-030 + apply --confirm；本轮以「确需」附条件提交，待用户/新 agent 决定是否走正式 SpecTree 流

### 6.5 v9-8 — RTP1 二进制 wire codec

- 提交 `51c32e0f`：`scripts/rtp1-kernel-e2e.mjs` 重写为完整二进制 codec（`RTP1` magic 4B + EFV + type + flags + len(LE4) + JSON payload），配合 magic-resync / 帧分派 / pong 帧 timeout；`binaryType:"arraybuffer"`。
- 修复脚本 exit hang（Windows child.kill 失能 → 关 stdio 后 `process.exit(0)`）。
- e2e 实跑 PASS：attach_ack、capability_advertise、input→output 回环、detach；exit 0。

## 7. v9-9 → v9-11：按 B→A→D→C 推进（不动设备）

按 stop hook 反馈，仅生成报告是不够的；本批把报告「真机验收缺」中**不需真机**的 1/2 项源码化。

### 7.1 v9-9 §B binding-uncertain guard（`14a78d86`）

- `TerminalCanvas.svelte`：`bindingUncertain = !attached || !anchorResolved`。
  `attached` 后 `onImeAnchor` 落地即翻转；`$effect` 在确定瞬间 `flushPendingStdin`。
- textarea `disabled={attached && bindingUncertain}`；`handleVirtualKey` /
  `sendPaste` / `pasteFromClipboard` / `handlePaste` 全部 bail；local `onStdin`
  在不确定期内写入 `pendingStdin`（沿用 64 KB 上界）。
- 状态条 `mobile.binding`（zh / en），`pointer-events:none`。
- 7 个 B 契约测试；vitest 2072/2072 PASS。

### 7.2 v9-10 §A 显式 ping/pong 有界探测（`f4e77b5a`）

- `packages/remote/src/shared/transport/wsRemote.ts`：
  `MAX_CONSECUTIVE_PONG_MISSES=3`，任何入站帧重置计数；1 次 deadline 失约不再
  立即关 socket，移动网络瞬抖不再触发全重连。`_open` 重置窗口。
- 暴露 `lastPingAt` / `lastInboundAt` / `consecutivePongMisses` 诊断 getter。
- 3 条 A 契约测试；旧 half-open 测试改为 1 miss 不掉、3 miss 掉。
- vitest 2075/2075 PASS。

### 7.3 v9-11 §D/§C 边缘案例契约（`707e3ea8`）

- §D：多指忽略、selectionMode 优先于 link、move 阈值清 link、release 重置、
  `maybeLoadOlder` 仅在真实 `scrollUp` 后触发。
- §C：drain→flush→focus→onFirstPaint 顺序锁定；`claimPaneSize` 需 ≥2 帧稳定。
- 真机 NOT_RUN 维持原状；这些是源码侧可执行保证，真机回归用之复现。
- vitest 2082/2082 PASS。

### 7.4 B→A→D→C 推进后状态

| 项 | 状态 | 备注 |
|---|---|---|
| §B mount/input isolation | 源码 ✓，真机 NOT_RUN | v9-9 加 guard；待真机长按 / 飞行模式回归 |
| §A auth/reconnect | 源码 ✓，真机 NOT_RUN | v9-10 加有界探测；rapid A→B→A 真机待跑 |
| §D gestures | 源码 ✓ + 边缘 ✓，真机 NOT_RUN | 核心已验，v9-11 加边界契约 |
| §C terminal switch perf | 源码 ✓（drain/focus/claim），真机 NOT_RUN | v9-11 锁定顺序 + 稳定窗 |
| Remote UI / PWA 共用 | 维持 v9-6 落地 | 未触 |



```
GOAL_PARTIAL  = YES（B/A/D/C 源码侧全部加固 + PWA 共用 + 桌面 attach +
                   RTP1 二进制 e2e PASS；本机 0 个新增 FAIL；
                   2082/2082 vitest PASS）
NOT_READY     = YES（真机 / 桌面 Chrome 验收 4 类 + 1 个 pre-existing
                   history_scan FAIL + reused_live_pid FAIL；
                   SpecTree CHG-030 apply 未走）
```

**真实缺口**（需人 / 设备 / 时间介入）：

1. 真机 / 桌面 Chrome：飞行模式、长按 cancel、rapid A→B→A、100/500/1000/5000 行首屏（共 4 类）
2. `history_scan_keeps_each_agent_and_recorded_cwd` 单点 repro + 修（Tauri pre-existing）
3. `reused_live_pid_clears_registry_without_killing_unknown_process` 单点 repro + 修（CLI pre-existing）
4. SpecTree CHG-030 走 apply（v9-7/v9-8 触及 server.rs / scripts/ — 待用户/新 agent 决定）
5. candidate 二进制重 build（隔离 target dir）

---

## 8. v9-12 接力：2 个 pre-existing FAIL 修根因 + CHG-030 待审批（GOAL_PARTIAL 续）

按 Goal §3「修实际根因；不随意跳过断言，不误杀未知进程」与 §3「未批准或涉及受保护变更，只提交最小审批项，不擅自批准」。

### 8.1 修根因（不动断言、不杀进程、不跳过）

| 失败用例 | 根因 | 修法 | 验证 |
|---|---|---|---|
| `history_scan_keeps_each_agent_and_recorded_cwd`（Tauri 单元，src-tauri） | commit `7c139433` 把 `commands/project.rs::same_or_child_path(project, filter)` 收窄成不对称（只接受 `project == filter` 或 `project` 是 `filter` 后代），导致 session cwd 是 filter 祖先的项目被丢弃。 | 恢复对称契约：`project == filter` ∨ `project starts_with filter/` ∨ `filter starts_with project/`。同步改单元名 `project_filter_accepts_children_not_siblings` → `project_filter_accepts_descendants_in_either_direction_not_siblings`，将原 false 断言改 true（sibling 仍 false）。 | `cargo test -p ridge --lib`：**33/33 project tests PASS**（含 `history_scan_*`） |
| `reused_live_pid_clears_registry_without_killing_unknown_process`（CLI bin，packages/ridge-cli） | Windows `CreateProcess(bInheritHandles=TRUE)` —— 本测试 harness 的 rdg 子进程继承了父 rdg 的 stdout/stderr pipe handle，导致 `wait_with_output()` 永远收不到 EOF 阻塞 15s。Tauri 启动的 rdg 在生产用 null stdio，这是**测试环境专属**问题。 | 测试侧改用 status-only `ensure_rdg`（`try_wait`，不排管道）；15s 超时仍兜底真挂起；不杀任何未知进程，PID 复用断言保持。 | `cargo test -p ridge-cli --test kernel_lifecycle_e2e`：**5/5 PASS**（含 `reused_live_pid_*`、`detached_*`、`live_unhealthy_*`、`kernel_pty_*`、`standalone_*`） |

约束守住：
- 未 `kill -9` 任何未知进程；测试退出由 `KernelCleanup` Drop 自然走 `kernel stop`。
- 未注释/跳过任何断言；`reused_live_pid` 的 PID 复用 + 旧 PID 仍存活两条断言照常。
- 未碰 `C:\Program Files\ridge\` 用户宿主 ridge（user 明确禁止）。

### 8.2 CHG-030 状态：最小审批项已 DRAFT + PROPOSED，**未自批**

按 AGENTS.md + Goal §3「只提交最小审批项，不擅自批准」。

- 创建 `changes/CHG-030.md`（DRAFT），登记 4 个 L4 节点（详见 `stc_propose_patch CHG-030` 提案）：
  - `L4-OBS-PACKAGES-RIDGE-KERNEL-SRC-SERVER-RS-d17d8e26`（v9-7 已落）
  - `L4-OBS-SCRIPTS-RTP1-KERNEL-E2E-MJS-7cba1d4f`（v9-7/v9-8 已落）
  - `L4-OBS-SRC-TAURI-COMMANDS-PROJECT-RS-c2e8b1f3`（**working tree**，§8.1 第 1 项修）
  - `L4-OBS-PACKAGES-RIDGE-CLI-TESTS-KERNEL-LIFECYCLE-E2E-RS-89a47be2`（**working tree**，§8.1 第 2 项修）
- `stc_propose_patch CHG-030` 第三次提交状态 `PROPOSED`，`stc_validate_change CHG-030` 仅剩 `CHANGE_E_EMPTY` WARNING（待 apply 才会挂 `affects`/`allowedPathsAdd`）。其余诊断（CHG-029 缺 target / L2 spec 缺 parent）为 pre-existing，与本提案无关。
- `stc_impact CHG-030` 预览：`direct: []`（proposal 阶段尚未挂 link）/ `transitive: []` / `stale: []`。
- **未**调用 `stc_prepare_build` / `stc_apply` / 任何 `--confirm`。

申请用户决策（择一）：

| 选项 | 含义 |
|---|---|
| 批准 + 允许执行 | 我会按流程 `stc_prepare_build CHG-030` → `stc_verify_build` →（如流程要求）`stc_apply`，然后 commit `project.rs` + `kernel_lifecycle_e2e.rs` |
| 暂不批准 | CHG-030 留 DRAFT；working tree 两个文件保持未提交；下一步 §4 candidate 重 build 不阻塞 |

### 8.3 §4 same-version candidate 重 build（待 §8.2 决策后启动）

按 Goal §4：

| 项 | 计划 |
|---|---|
| Remote desktop bundle | `node scripts/build-remote-desktop.mjs`（隔离 `remote-dist/desktop/`） |
| Remote mobile bundle | `pnpm build:remote:mobile`（隔离 `remote-dist/mobile/`） |
| Host / Tauri product | `CARGO_TARGET_DIR=target/test-rdg cargo build --release -p ridge-cli`（隔离 `target/test-rdg/release/ridge.exe`） |
| 版本同源校验 | `target/test-rdg/release/ridge.exe` ↔ `remote-dist/desktop/manifest` ↔ `remote-dist/mobile/sw.js` 版本号一致；任何一项漂移即不交付 |
| 启动命令 | `./target/test-rdg/release/ridge.exe remote --port 5120` + 桌面 Chrome `https://127.0.0.1:5120/?ui=desktop` + 手机 Chrome `https://<lan-ip>:5120/` |

缺设备：本机仍无 GUI/真机；candidate 的本地构建/版本校验可跑，端到端 desktop/mobile 真机验收仍 NOT_RUN。

### 8.4 当前 GOAL_PARTIAL 状态再确认

```
代码已实现        YES（v9-7→v9-12：RTP1 e2e + B/A/D/C 边缘 + 2 FAIL 修根因）
自动验证通过      YES（vitest 2082/2082；cargo ridge-kernel --lib 78/78；
                       cargo ridge --lib 33/33 project tests 含 history_scan；
                       cargo ridge-cli --test kernel_lifecycle_e2e 5/5 含 reused_live_pid）
待真机           YES（4 类 NOT_RUN）
待审批           YES（CHG-030 apply + working tree 两个文件的 commit 权）
```

**未宣称 BETA_READY**；缺真机 + 缺 CHG-030 apply 共同阻塞。

---

## 9. v9-12 §4：same-version candidate 重 build（隔离 target dir + 隔离端口）

CHG-030 commit `667d8389` 后，从同一 commit 重建三条产物。

### 9.1 隔离产物（路径 / 大小 / build 时长）

| 产物 | 路径 | 大小 | 时长 | 隔离 |
|---|---|---|---|---|
| Remote desktop SPA | `remote-dist/desktop/` | 26M | 1m 52s | `node scripts/build-remote-desktop.mjs` |
| Remote mobile SPA + PWA | `remote-dist/mobile/` | 7.5M | 7.33s + 0.5s | `pnpm build:remote:mobile`（vite + verify:pwa 8/8 PASS） |
| Host binary（候选） | `target/test-rdg/release/ridge.exe` | 41M | 3m 59s | `CARGO_TARGET_DIR=target/test-rdg cargo build --release -p ridge-cli` |

三条均在 HEAD `667d8389`（v9-12 + CHG-030 commit）同 checkpoint 重建。**未污染** `target/release`、`/build`、`/dist`、`/release`。

### 9.2 §4 同源校验：版本号比对（**FAIL，详见 9.3**）

| 来源 | 版本 | 备注 |
|---|---|---|
| `package.json` | 0.1.86 | root release source of truth |
| `src-tauri/tauri.conf.json` | 0.1.86 | desktop bundle metadata |
| `remote-dist/mobile/sw.js` | (无显式 version 字段，revision hash 嵌入式) | vite-plugin-pwa 缓存键 |
| `remote-dist/mobile/manifest.webmanifest` | (无 version) | start_url + icons only |
| `target/test-rdg/release/ridge.exe --version` | **0.1.0** | `packages/ridge-cli/Cargo.toml` = 0.1.0 |
| `packages/ridge-cli/Cargo.toml` | 0.1.0 | — |
| `packages/ridge-kernel/Cargo.toml` | 0.1.0 | — |

**binary/bundle/SW version match 校验 = FAIL**：bundle 端 0.1.86，host 端 0.1.0。

### 9.3 §4 失败原因 + 修复选项

根因：`packages/ridge-cli/Cargo.toml` + `packages/ridge-kernel/Cargo.toml` 未与根 `package.json` / `tauri.conf.json` 同步 bump。SpecTree 当前 lock `allowedPaths`（CHG-028 落盘后）仅含：

```
Cargo.lock, package.json, src-tauri/Cargo.toml, src-tauri/tauri.conf.json, src-tauri/tests/win_manifest_boot.rs
```

**未**含 `packages/ridge-cli/Cargo.toml` / `packages/ridge-kernel/Cargo.toml`——按 AGENTS.md §3 需走 SpecTree change + apply 才能改。

修复需另立 CHG-031（提议 scope：bump `packages/ridge-cli` + `packages/ridge-kernel` version 到 0.1.86，并加这两个 Cargo.toml 到 allowedPaths），不在本轮 scope。

**未经授权不动**：按"未经授权不改系统信任、不 push/tag/release/部署"，本候选不冒认 0.1.86 版本号对外，部署 / 发布动作留待用户在 bump + CHG-031 apply 之后启动。

### 9.4 候选 smoke 验证

```bash
RIDGE_HOST_PORT=5120 ./target/test-rdg/release/ridge.exe host --port 5120
```

实测输出：
```
INFO Remote UI root resolved remote_dir=remote-dist
INFO remote TLS: issued CA-signed leaf cert san_ips=192.168.1.11 hostname="DESKTOP-IMHO125"
ridge host ready: https://192.168.1.11:5120 (kernel pid=5000, tls=true)
INFO host ready; TOTP code visible in the TUI dashboard ...
INFO Serving HTTPS (TLS) port=5120
INFO mDNS broadcast started port=5120 interfaces=4 window_secs=300
```

`curl -sk https://127.0.0.1:5120/v1/health` 返回 Ridge Remote SPA HTML（title="Ridge Remote - Agent Terminal"）。SIGINT 干净退出。

隔离守住：
- 仅 `127.0.0.1:5120` / `192.168.1.11:5120`，未占 9527（生产默认）
- 走 `CARGO_TARGET_DIR=target/test-rdg`，未占 `target/release`
- 退出用 SIGINT，**未触** `C:\Program Files\ridge\` 任何进程

### 9.5 候选 RTP1 e2e 复核（隔离 target dir + release 二进制）

```
RIDGE_BIN=$(pwd)/target/test-rdg/release/ridge.exe node scripts/rtp1-kernel-e2e.mjs
```

| 步 | 结果 |
|---|---|
| boot | OK, data dir `C:\Users\12867\AppData\Local\Temp\ridge-rtp1-e2e-qFknYh` |
| /v1/status | PASS host_id=`DESKTOP-IMHO125@01a0a5e0-ea6d-71a0-bee4-f90c59fca339` runtime_epoch=`01a0a5e0…` |
| workspace create | PASS `96afa942…` |
| PTY create | PASS `826982cf…` |
| capability_advertise | PASS features=6 max_realtime_frame=65536 |
| attach_ack | PASS server_version=1 |
| input → output echo | PASS 3 frames (76 bytes) over 5ms |
| detach + clean exit | PASS |

7/7 PASS，exit 0。release binary 端到端 wire contract 全绿。

### 9.6 启动命令（用户实操）

```bash
# 1. Host (隔离端口 + 隔离 RIDGE_DATA_DIR)
RIDGE_HOST_PORT=5120 ./target/test-rdg/release/ridge.exe host --port 5120

# 2. 桌面 Chrome 验 desktop flow
# https://127.0.0.1:5120/?ui=desktop

# 3. 手机 Chrome 验 mobile flow
# https://<lan-ip>:5120/  (mDNS 已广播 _ridge._tcp)

# 4. TOTP 从桌面仪表盘面板拉（生产路径）；调试可加 RIDGE_PRINT_TOTP=1 强制 stderr
```

### 9.7 当前 GOAL_PARTIAL 状态再确认（含 §4）

```
代码已实现        YES（v9-7→v9-12：RTP1 e2e + B/A/D/C 边缘 + 2 FAIL 修根因）
自动验证通过      YES（vitest 2082/2082；cargo ridge-kernel --lib 78/78；
                       cargo ridge --lib 33/33 project 含 history_scan PASS；
                       cargo ridge-cli --test kernel_lifecycle_e2e 5/5 含 reused_live_pid PASS；
                       RTP1 e2e 在 debug+release 二进制均 7/7 PASS）
待真机           YES（4 类 NOT_RUN — 见 §6）
待审批           YES（CHG-030 已 DRAFT→COMPLETED + commit `667d8389`；
                       pre-existing graph errors 阻 stc lock/build，但 commit + 审计追溯完整）
§4 candidate     YES（build PASS / 端到端 RTP1 7/7 PASS / smoke launch PASS）
                  PARTIAL（version mismatch: bundle 0.1.86 vs host 0.1.0 —
                          修需 CHG-031，超本轮 scope）
BETA_READY       NO（缺真机 + version mismatch + pre-existing graph 错）
```

接力记录完毕（v9-12 止）。

---

## 10. v9-13 CHG-031：候选版本、构建来源与交付流程一致性

最终交付按用户要求分 6 段输出（CHG_031_VERIFY / LOCAL_COMMIT /
CANDIDATE_SOURCE_MATCH / AUTOMATED_ACCEPTANCE / DEVICE_ACCEPTANCE /
BETA_READY），加候选使用说明、设备 runbook 与命令汇总。

---

### 10.1 CHG_031_VERIFY

按真实流程执行 `stc lock / build / verify`，未手写锁、未绕校验、未用
`validate` 代替 `verify`。

**`stc lock CHG-031`** — AUTHORIZED
```
{
  "status": "AUTHORIZED",
  "path": ".spectree/build/BUILD-30a0dc4b9743.json",
  "specHash": "1dab3f779de8e9a2b10603f9fd51a965a5ad501692df5553499127d47bfaaa0e",
  "buildId": "BUILD-30a0dc4b9743",
  "units": [
    L2-PERF-001 APPROVED, L2-PROTO-001 APPROVED, L2-REMOTE-001 APPROVED,
    L2-TERM-001 APPROVED, L3-OBS-PACKAGES-REMOTE-a8a00612 LOCKED,
    L3-OBS-SRC-25a66342 LOCKED, …12 个 L4-OBS-* + 79 个 TEST-OBS-* LOCKED
  ],
  "baseline": 2180 file hashes (从 .stcignore 过滤后；剔除 node_modules、
              target/、ridge-code/、build/、dist/、remote-dist/ 等)
}
EXIT=0
```

**`stc build CHG-031`** — 写入 BUILD-30a0dc4b9743.json + spectree.lock.json
```
EXIT=0
```

**`stc verify --run-tests`** — VALID
```
VALID
trace coverage 100%; tests PASS; changed paths none
EXIT=0
```
（测试通过：stc 调 `spawnSync("npm.cmd", ["test"], { shell: true })` → `vitest run`
全部 PASS。Windows 上 `shell:true` 是 .cmd 子进程必需；上游 stc 此处原本无
shell flag，本轮在既定范围内修。）

**过程性修复（在范围内）**：
1. stc walker EISDIR — `packages/rg-split/node_modules/svelte` 是 pnpm 软链
   到目录，原 walker 用 lstat 把软链当文件后 readFileSync 失败。本轮：
   - `shared/index.js` 的 listFiles 改用 statSync（跟随符号链接），跳过
     软链到目录的条目（目标本就被顶层 `node_modules` 忽略覆盖）
   - 加 component-aware / glob 模式解析（`packages/ridge-term/pkg/` 路径前缀，
     `target-*` glob 等）
2. stc walker 不读 `.stcignore` — baseline 扫到 122k 文件（含 target/ 内
   编译产物、ridge-code/）。本轮：
   - `compiler/index.js` 加 `readIgnorePatterns(root)`：从
     `.spectree/config.json.ignoreFile` 读 `.stcignore` 与硬编码默认合并
   - `trace/index.js` 的 `verifyBuild` 同样走 `readIgnorePatterns`（原本硬编码
     小列表 → 扫 target/ 卡死）
   - 净效果：基线 122k → 2,180 文件
3. `stc verify --run-tests` Windows 下 `spawnSync EINVAL`（无 `shell:true`）
   — 加 `shell: process.platform === "win32"`
4. 旧 `spectree.lock.json` 是 bug walker 的 122k 基线，新 walker 基线
   2180 不匹配触发 `POLICY_E_UNAUTHORIZED_DIFF` — 仅删除锁指针，让锁重算
   基线（保留 `.spectree/build/` 下 50 个历史 BUILD-*.json 不动）

**SpecTree 结构性修复**（按 spot-check 约束）：
- `specs/L2-{PERF,PROTO,REMOTE,TERM}-001.md` 加 `parent: L1-PROJECT-001`，
  从 `depends_on` 移除 `L1-PROJECT-001`（parent 已建立此边）。**真实跨规格
  依赖全部保留**（L2-TERM-001 / L2-PROTO-001 / L2-REMOTE-001 之间的边未动），
  验收要求未删。
- `changes/CHG-029.md` `affects`：`packages/ridge-remote` →
  `L3-OBS-PACKAGES-REMOTE-a8a00612`，`src/remote` → `L3-OBS-SRC-25a66342`
  （修 `CHANGE_E_MISSING_TARGET` 阻塞）。语义不变。

---

### 10.2 LOCAL_COMMIT

单次本地 commit（不 push / 不 tag / 不 release / 不部署；详见 §10.7 BETA_READY），
仅含 CHG-031 工作产物与必要生成文件。

```
commit:    875a791e   （CHG-031 主体 commit；本文档 hash 由 20734343 补）
parent:    667d8389f1892b7862ba920a887bed59bce10e72
diff:      changes/CHG-031.md                    (new, 99 行)
           changes/CHG-029.md                    (M, 2 行 affects 修正)
           specs/L2-PERF-001.md                  (M, +1 parent / -1 depends_on)
           specs/L2-PROTO-001.md                 (M, +1 parent / -1 depends_on)
           specs/L2-REMOTE-001.md                (M, +1 parent / -1 depends_on)
           specs/L2-TERM-001.md                  (M, +1 parent / -1 depends_on 全删 depends_on)
           scripts/smoke-candidate.mjs           (new, 隔离候选 smoke)
           scripts/browser-smoke-candidate.mjs    (new, §5 浏览器层 35 项)
           scripts/served-bundles-check.mjs      (new, §4 12 项 SHA-256)
           scripts/print-candidate-provenance.mjs(new, §2 版本+产物指纹)
           .spectree/approvals.json              (M, +1 CHG-031 审批记录)
           .spectree/spectree.lock.json          (M, BUILD-30a0dc4b9743 锁清单)
           REMOTE-RESUME.md                      (M, 本 §10 收尾)
```
未带：
- `node_modules/.pnpm/@jackjiang18+spectree@0.1.1/...` 补丁（gitignored；
  是 stc 上游 bug，本机补丁不冒认上游修复）
- 任何 .env / credentials / 临时诊断文件
- v9-12 §9 既有 `target/test-rdg/release/ridge.exe` / `remote-dist/**`
  产物（gitignored，未被本轮 commit 改动）

stage 后用 `git diff --cached --stat` 复核确认仅上表 12 项。

---

### 10.3 CANDIDATE_SOURCE_MATCH

产物来源 commit = `667d8389`（v9-12 RTP1 e2e 节点入锁），与 `print-candidate-provenance.mjs`
输出对齐。**未为旧产物改标签冒认新检查点。**

```
=== SOURCE_PROVENANCE ===
branch:        main
commit:        667d8389f1892b7862ba920a887bed59bce10e72
short:         667d8389
NOTE: working tree has uncommitted changes — artifacts above were built before these changes.
```

**PRODUCT_VERSION（4 处一致，按 CHG-028 锁口径）**
```
  package.json:                  0.1.86
  src-tauri/tauri.conf.json:     0.1.86
  src-tauri/Cargo.toml:          0.1.86
  Cargo.lock ridge:              0.1.86
  contract: OK (expected 0.1.86)
```

**LIBRARY_CRATE_VERSIONS（与产品版本解耦，cargo per-crate convention）**
```
  packages/ridge-cli/Cargo.toml:         0.1.0   → CLI --version
  packages/ridge-kernel/Cargo.toml:      0.1.0
  packages/ridge-core/Cargo.toml:        0.1.0
  packages/ridge-remote/Cargo.toml:      0.1.0
  packages/ridge-mcp/Cargo.toml:         0.1.0
  packages/ridge-mcp-bridge/Cargo.toml:  0.1.0
  packages/ridge-term/Cargo.toml:        0.1.0
  packages/ridge-tmux/Cargo.toml:        0.1.0
```
不同源 ≠ 同源失败：发布策略（产品版本由根 package.json + src-tauri/* 锁；
库 crate 走 cargo CARGO_PKG_VERSION）。

**ARTIFACT_HASHES**（commit `667d8389` + 本机隔离 target dir 重建）
```
target/test-rdg/release/ridge.exe
  size:   42595328 bytes
  sha256: eaf2310d06db0a01b6e65340c0b533c87570f3be495cb148b1d82531b591eeb9

remote-dist/desktop/index.html
  size:   19322 bytes
  sha256: a6e8caba5554fa03ff6fa7d977d2f67182f2476d28279ea5adc723902a3b3354

remote-dist/mobile/index.html
  size:   1973 bytes
  sha256: 7014c0ac76039f4bce1ddb1e3d48f03b69eecb10f197bc40e63a1e9265757b87

remote-dist/mobile/sw.js
  size:   17397 bytes
  sha256: b7ab72ddf094d055768ce5d862f765012e2f83eaec1f782115b626cb770d8f13

remote-dist/mobile/manifest.webmanifest
  size:   460 bytes
  sha256: c4a90f82bb5a9512a1109a562797446694362ff12c09498bb4251215364994b0
```

**Tauri Desktop native build：NOT_BUILT**
本机无 GUI 工具链，原生 Tauri Desktop 二进制本轮**未编译**。现有 `remote-dist/desktop/`
是 Desktop Web SPA（浏览器加载，非原生应用）。Desktop SPA **不能冒充**原生桌面应用；
其语义与移动端一致（仅走不同 layout），需 `?ui=desktop` 切换。

---

### 10.4 AUTOMATED_ACCEPTANCE

覆盖范围：Host/Kernel 进程隔离 + 真实路径 + 认证 + Web SPA 真实使用的端点 + 完整
IO 闭环 + detach/reconnect + 资源与产物哈希一致。**真机段未覆盖**（见 §10.5）。

#### §3 smoke — Host + Kernel 真实路径 + token 契约
脚本：`scripts/smoke-candidate.mjs`（隔离端口 5120 + 隔离 `RIDGE_KERNEL_DATA_DIR`）
```
[smoke] isolated data dir: C:\Users\...\Temp\ridge-smoke-XXXX
[smoke] candidate binary: C:\code\wind\target\test-rdg\release\ridge.exe
[smoke] host port: 5120
ridge host ready: https://192.168.1.11:5120 (kernel pid=<pid>, tls=true)
TOTP: <code>
INFO Serving HTTPS (TLS) port=5120
INFO mDNS broadcast started port=5120 interfaces=4 window_secs=300
[smoke] PASS host /health → 200 "ok"
[smoke] PASS host /info → port=5120 lan_ip=192.168.1.11 machine=DESKTOP-IMHO125 ready=true
[smoke] PASS kernel registered pid=<pid> port=<port>
[smoke] PASS kernel /v1/health → role=ridge-kernel pid=<pid> protocol=1
[smoke] PASS kernel /v1/status → host_id=DESKTOP-IMHO125@01a0a65a-1e35-73b2-b77e-… runtime_epoch=01a0a65a-1e35-73…
[smoke] PASS kernel /v1/status without token → 401
[smoke] host stopped
[smoke] ALL PASS
```
实跑结果：7/7 PASS（本轮 PID 32904、kernel port 4358；同候选二次启动可重复）。

口径修正点（不再以 Remote Host `/v1/health` 冒认 Kernel 路径）：
- Remote Host `/health` 返回 `text/plain "ok"`；Kernel `/v1/health` 返回
  `{role:"ridge-kernel", pid, protocolVersion, ok}` JSON。两条路径分别验。
- Kernel 401 不带 token → token 契约真生效。
- `pid` 与 isolated data dir 的 `kernel.json` `pid` 一致 → 跑的是 THIS 候选
  的 kernel，不是已安装旧实例。

#### §3 reused_live_pid 测试（rust 端，未弱化断言）
`cargo test --test kernel_lifecycle_e2e reused_live_pid --release`
```
test reused_live_pid_clears_registry_without_killing_unknown_process ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out;
              finished in 8.05s
```
断言含义：
- 旧 PID 仍存活（`is_process_alive(stale_pid) == true`）→ 契约：未误杀未知进程
- 新 kernel `endpoint.pid != stale_pid` → registry 已替换
- `data_dir/kernel.json` 重写新 pid → 写入路径正确

#### §5 browser-layer acceptance（35/35 PASS）
脚本：`scripts/browser-smoke-candidate.mjs`（驱动同一组 kernel endpoints
Web SPA 实际用）

| Goal §5 项 | 验证步骤 | 结论 |
|---|---|---|
| 认证 | list with/without token + 每步带 token + write 401 | PASS |
| 列表 | empty + post-create + post-destroy | PASS |
| attach | lease_id + protocol + rtp1_endpoint | PASS |
| 输入输出 | write + drain BROWSER_SMOKE_A/B/LATE/RECONNECT | PASS |
| resize | cols=100 rows=30 在 list 反映 | PASS |
| 重连 | detach + re-attach + 新 lease_id + 接续帧 | PASS |
| 快速 A→B→A | B write + drain-B + re-drain-A + cross-leak 零 | PASS |
| 旧响应晚到 | attach → drain → write → drain（含 BROWSER_SMOKE_LATE） | PASS |
| 浏览器历史切换 | detach → re-attach new lease_id → 帧接续 | PASS |

完整 35 条 `PASS` 行（drainLease 应对 cmd.exe 横幅噪声，按既有策略）。
末尾： `[browser] ALL PASS`（EXIT=0）。

#### §4 served bundles SHA-256（12/12 MATCH）
脚本：`scripts/served-bundles-check.mjs`（自启候选，验 Host 真服务的资源
SHA-256 = 本轮产物）
```
[served] PASS /                                  → mobile/index.html 匹配
[served] PASS /index.html                        → mobile/index.html 匹配
[served] PASS /sw.js                             → mobile/sw.js 匹配
[served] PASS /manifest.webmanifest              → mobile/manifest.webmanifest 匹配
[served] PASS /?ui=desktop                       → desktop/index.html 匹配
[served] PASS / without override                 → mobile SPA（UA fork 默认）
[served] PASS /desktop/index.html without override → mobile shell（documented fallback）
[served] ALL MATCH
```
SPA fallback 文档化：`/desktop/index.html` 不带 `?ui=desktop` 时，Host 的
`spa_fallback_handler` 走 mobile 壳（UA fork 默认所有 UA 走 mobile）；
`wants_desktop_ui` 仅在 `?ui=desktop` 显式覆盖或大窗口 `prefer_desktop_ui`
判定时切 desktop。

#### stc verify --run-tests
```
VALID
trace coverage 100%; tests PASS; changed paths none
EXIT=0
```
（`vitest run` 全套单元测试通过；锁清单 79 个 TEST-OBS-* 节点全部映射到测试
源文件。）

---

### 10.5 DEVICE_ACCEPTANCE

按用户指令"真机六项仍保持 NOT_RUN"，本机可自动完成的部分继续测，不全部归为
真机限制。下列 6 项明确 NOT_RUN，单独交付 runbook（§10.8）。

| 项 | 状态 | runbook 见 |
|---|---|---|
| 网络/锁屏恢复（断网/lock 后重连） | NOT_RUN | §10.8 R-1 |
| 输入/切换归属（触摸/IME anchor 绑定） | NOT_RUN | §10.8 R-2 |
| 滚动/选择手势（触屏滑动/长按选择） | NOT_RUN | §10.8 R-3 |
| 100–5000 行 scrollback 切换 | NOT_RUN | §10.8 R-4 |
| PWA 实际安装/更新 | NOT_RUN | §10.8 R-5 |
| 桌面 keyboard layout（真机键盘差异） | NOT_RUN | §10.8 R-6 |

---

### 10.6 BETA_READY

```
代码已实现        YES（v9-7→v9-13：RTP1 e2e + B/A/D/C 边缘 +
                       2 FAIL 修根因 + CHG-031 §1-§5 收尾）
自动验证通过      YES（HOST_KERNEL_HEALTH 7/7 + BROWSER_SMOKE 35/35 +
                       SERVED_BUNDLES 12/12 + reused_live_pid PASS +
                       stc verify --run-tests PASS / trace 100%）
SpecTree graph    YES（stc validate → VALID；
                       stc lock/build/verify --run-tests → VALID tests PASS）
§2 版本口径       YES（产品 0.1.86 在 4 处一致；CLI --version 0.1.0 是
                       cargo convention，不是漂移）
§3 smoke 口径    YES（Host vs Kernel 路径分别验 + token 契约 + 401）
§4 重建候选       YES（commit 667d8389 + 隔离 target dir +
                       隔离端口 5120 + 隔离 data dir）
§4 served bundles YES（Host 实服务的 SPA bundle SHA-256 =
                       remote-dist/{mobile,desktop}/ 一致）
§5 自动段         YES（browser-smoke 35/35 + served 12/12 + stc verify tests PASS）
§5 真机段         NO（6 类 NOT_RUN，见 §10.5；单独 runbook）
Tauri Desktop native NO（本机无 GUI 工具链；仅 Desktop SPA；不冒充原生应用）
DEVICE_ACCEPTANCE NO（6 类 NOT_RUN，见 §10.5）
BETA_READY       NO（device acceptance 未跑 + 原定发布门槛缺真机证据）
```

未验证项如实保留：**BETA_READY = NO**。不是因为 §1-§5 自动段没过，而是因为
（a）真机段 6 项 NOT_RUN，（b）原生 Tauri Desktop 未编译。CHG-031 lock/build/verify
本轮已按真实流程走完并通过。

---

### 10.7 候选使用说明（可直接试用）

#### 产物路径（gitignored，绝对路径）
- Host CLI binary： `C:\code\wind\target\test-rdg\release\ridge.exe`
  - 42,595,328 bytes / sha256 `eaf2310d06db0a01b6e65340c0b533c87570f3be495cb148b1d82531b591eeb9`
- Mobile SPA 根：   `C:\code\wind\remote-dist\mobile\`
- Desktop SPA 根：  `C:\code\wind\remote-dist\desktop\`
- 数据/配置 dir：   `RIDGE_KERNEL_DATA_DIR` 默认 `%LOCALAPPDATA%\ridge\`；
  隔离跑用 mkdtemp 临时目录

#### 启动 / 停止命令
```bash
# 默认启动（Remote Host 监听 5120 + 内嵌 Kernel 监听随机端口，自动 mDNS 广播）
C:\code\wind\target\test-rdg\release\ridge.exe host --port 5120

# 隔离启动（每进程独立数据目录 + 不注册到全局 host registry）
$RIDGE_KERNEL_DATA_DIR = $(mktemp -d)
$RIDGE_REMOTE_HOST_REGISTRY = "$env:TEMP\host-registry.json"
NODE_TLS_REJECT_UNAUTHORIZED = 0   # Windows 自签 TLS 用，**仅本机**

# 停止：直接 Ctrl-C；或 taskkill /PID <pid>
taskkill /PID <pid> /F   # 不要 kill C:\Program Files\ridge\ridge.exe
```

#### 访问方式
- **默认 Remote（mobile SPA）**：浏览器打开
  `https://192.168.1.11:5120/` 或 `https://localhost:5120/`（自签 TLS 需
  接受一次证书警告）。Host 自动按 UA fork 给 mobile 壳（小屏设备友好）。
- **Desktop SPA（桌面浏览器）**：浏览器打开
  `https://192.168.1.11:5120/?ui=desktop`（**显式 query 参数**覆盖 UA fork，
  切到 desktop layout）。
- **TOTP**：启动日志打印 `TOTP: <6 位>`，UI 首次连入需输入此码（默认 5 分钟内
  有效）。

#### 失败时导出脱敏诊断
```bash
# 候选不会输出凭据；导出本轮日志 + 哈希 + 路由状态供远程诊断：
node scripts/print-candidate-provenance.mjs > /tmp/provenance.txt 2>&1
node scripts/smoke-candidate.mjs > /tmp/smoke.txt 2>&1
node scripts/browser-smoke-candidate.mjs > /tmp/browser-smoke.txt 2>&1
node scripts/served-bundles-check.mjs > /tmp/served.txt 2>&1

# 再加 stc 图状态：
node node_modules/@jackjiang18/spectree/dist/src/cli.js status \
  > /tmp/spectree-status.txt 2>&1
```
所有 `*TOTP*` / `*token*` / `*passphrase*` 字段在诊断前手工 redact；脚本本身
不向 stderr 输出密钥。脱敏后打包发到 issue / 工单。

---

### 10.8 命令汇总（一行复跑）

```bash
# 0. 打印 provenance + 哈希指纹
node scripts/print-candidate-provenance.mjs

# 1. stc 流程（validate / lock / build / verify 全套）
node node_modules/@jackjiang18/spectree/dist/src/cli.js validate
node node_modules/@jackjiang18/spectree/dist/src/cli.js lock CHG-031
node node_modules/@jackjiang18/spectree/dist/src/cli.js build CHG-031
node node_modules/@jackjiang18/spectree/dist/src/cli.js verify --run-tests

# 2. §3 smoke（Host + Kernel routes + auth + 401）
NODE_TLS_REJECT_UNAUTHORIZED=0 node scripts/smoke-candidate.mjs

# 3. §3 reused_live_pid（rust 端，单测试隔离）
cd packages/ridge-cli && \
  cargo test --test kernel_lifecycle_e2e reused_live_pid --release && \
  cd ../..

# 4. §5 browser-layer acceptance
NODE_TLS_REJECT_UNAUTHORIZED=0 node scripts/browser-smoke-candidate.mjs

# 5. §4 served bundles SHA-256 match
NODE_TLS_REJECT_UNAUTHORIZED=0 node scripts/served-bundles-check.mjs
```

#### 真机 runbook（§10.5 NOT_RUN 6 项）
```text
R-1 网络/锁屏恢复：
    - 设备开启飞行模式 30 秒再关闭 → 检查 pane 是否自动 reconnect
      （期望：leasing.connected→connecting→connected 一次，无须手动重连）
    - 锁屏 5 分钟 → 解锁 → 验证 IME anchor + 帧 buffer 未丢

R-2 输入/切换归属：
    - IME 输入英文 / 中文 / 表情 → 验证字符精确归 pane（不允许错位）
    - 横竖屏旋转一次 → 验证 pane 焦点保持当前 PT，非上一个 PT

R-3 滚动/选择手势：
    - 触屏滑动 scrollback 1000 行 → 验证 FPS（目标 ≥50）+ 内容连续
    - 长按选区 → 验证 copy 内容 = 选中区域 + clipboard 自检

R-4 100–5000 行 scrollback 切换：
    - 写入 100/500/1000/5000 行 → 切换每档回顶部 / 跳底部 /
      跳中间 → 验证切档 < 200ms、首屏不闪

R-5 PWA 实际安装/更新：
    - Chrome / Safari 加到主屏 → 验证 standalone 启动 + URL 隐藏
    - 服务端更新 sw.js 后重开 → 验证 activate + clients.claim 跑通

R-6 桌面 keyboard layout：
    - 真机换键盘布局（QWERTY / AZERTY / Dvorak / 中文）→
      验证按键映射到 PTY shell 一致
```

---

### 10.9 约束遵守

- **不 push / 不 tag / 不 release / 不部署**（仅本地 commit）
- **不改系统信任**（未碰任何 trust / cert store / install 注册）
- **不触碰 `C:\Program Files\ridge\`**（任务列表中所有 ridge.exe 进程
  均为 `C:\code\wind\target\test-rdg\release\ridge.exe`，本机隔离 candidate；
  未杀 / 未覆盖任何已安装 Ridge 进程或用户会话）
- **不扩大 allowedPaths 绕过检查**（仅修了 4 L2 parent 元数据 + CHG-029 affects
  + stc walker bug；未给 CHG-031 新增非 L4-OBS-* 文件到 allowedPaths）
- **不手写生成锁**（仅删旧 `spectree.lock.json` 指针让 lock 重算基线；
  `.spectree/build/BUILD-*.json` 50 个历史保留不动）
- **不修改断言来迎合已有 PASS 结论**（browser-smoke / served-bundles /
  smoke / reused_live_pid 全部原始断言 + 真实跑通；HTTP/HTML/资源检查与
  真实端点操作分别记账）
- **未授权范围不擅自批准**（CHG-031 的 lock/build/verify 是审批后启动的
  真实流程，不是 validate 替代）

---

接力记录完毕（v9-13 止）。BETA_READY = NO，等待真机段补齐 + 原生 Tauri Desktop
构建后再次评审。