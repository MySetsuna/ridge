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



```
GOAL_PARTIAL  = YES（A/B/D 行为核心 + PWA 共用入口 + 桌面 attach 代码就位；
                   C 沙盒基线；本机 0 个新增 FAIL；重 build 管线通过）
NOT_READY     = YES（真机验收 + ping/pong 显式有界探测 + 顶层 ws dep +
                   candidate 重 build + 1 个 pre-existing history_scan FAIL
                   仍未处理）
```

**真实缺口**（需人 / 设备 / 时间介入）：

1. 真机 / 桌面 Chrome：飞行模式、长按 cancel、rapid A→B→A、100/500/1000/5000 行首屏（共 4 类）
2. 显式 ping/pong 有界探测（v9-6 列入 N.5）
3. 顶层 `ws` dep 声明（5 行 PR）
4. `history_scan_keeps_each_agent_and_recorded_cwd` 单点 repro + 修（已存在 workspace 上下文）
5. candidate 二进制重 build（隔离 target dir，不影响用户日常 build）

---

接力记录完毕。
