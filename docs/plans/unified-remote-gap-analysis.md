# 统一远控架构 —— 未完成任务 Gap 分析（交接版）

> 配套文档：`unified-remote-architecture-handoff-final.md`（原始计划 S0–S8）、`orchestration-log.md`（GM 编排权威记录 + 决策 D-S1-1/D-GM-1..11）、`s1-migration-ledger.md`（handler 迁移台账）、`s8-shim-audit.md`（shim 审计）。
>
> 状态时间：2026-06-04。本文件只列**未完成 / 有 gap 的部分**，并精确指出 gap 在哪个文件、缺什么、属于哪类阻塞、如何收尾。**已完成且已验证的部分见 orchestration-log，不在此重复。**

## 总览：完成度与 gap 分类

| 子项 | 代码状态 | 运行时验证 | Gap 类别 |
|------|---------|-----------|---------|
| S0 契约 | ✅ 完成 | ✅ | 无 |
| S1 ridge-core 地基 | ✅ 完成 | ✅ LAN 实证 | 无（仅剩 handler 迁移台账，见 G4） |
| S2 Transport 分层 | ✅ 完成 | ✅ | 无 |
| S3 协议骨干（LAN host） | ✅ 完成 | ✅ LAN 实证 | 无 |
| S4 cloud（client+host+controller） | ✅ 代码完成 | 🟡 invoke/文件树 live 打通；终端待双端实测 | **G3, G5** |
| S5 headless ridge-cli MVP | ✅ 完成 | ✅ cargo test | 无（写命令迁移待 D-GM-9，见 G4） |
| **S6 公网下发部署** | ✅ 代码完成(`ridge-cloud@fff01da`) | ❌ **未部署** | **G1（硬阻塞）** |
| S8 fs 沙箱 | ✅ 完成 | ✅ cargo test | D-GM-9 headless 沙箱 no-op（见 G4） |
| **D-GM-10 E2EE 身份绑定** | ❌ 未实现 | ❌ | **G2（跨仓库安全硬化）** |

**核心功能无遗留**：统一架构 + LAN 全链路 + cloud invoke/文件树闭环均 live 实证。下列 gap 全部属于 ①外部/跨仓库阻塞、②预存在工具链问题、③需重建+finicky live 周期 三类，**非实现缺失**。

---

## G1 — S6 公网部署（硬阻塞：ridge-cloud 远端历史分叉）

**目标**：让 `app.9527127.xyz` 向公网浏览器下发桌面 controller SPA（CSP-fixed web-remote-dist），实现"任意浏览器 → cloud → 桌面 host"的零安装入口。

**Gap 在哪里**：
- 代码**已完成并本地提交**于 `C:\code\ridge-cloud`，commit `fff01da`：
  - `src/static_host.rs` — host-label 路由：`app.*` 子域 → `desktop-app/`；主域名 / `/api` / `/ws` 路由不变。
  - `src/router.rs`、`src/config.rs` — 接线 host-label 分流。
  - `Dockerfile` — 把 `desktop-app/`（= wind 的 `web-remote-dist` 产物）纳入镜像。
  - `desktop-app/` — CSP 已修正的 controller 产物副本。
  - `cargo check`(ridge-cloud) 0 err。
- **DNS + TLS 已就绪**：duckdns 通配 + `*.9527127.xyz` 证书；`curl app.9527127.xyz` = 200、cert valid。**无需额外 ops。**

**阻塞类别**：跨仓库 + 生产历史分叉（非代码问题）。
- `git push dokku main` 被拒（non-fast-forward）。
- fetch 后实测：**本地 clone 与已部署 `dokku/main` 历史分叉**（ahead 4 / behind 1、根 commit 不同 = E 组曾对生产仓 force-push / re-init）。
- cherry-pick 探查证实：**deployed dokku/main 是更旧的单体代码**（`static_host.rs` 在 deployed HEAD 中不存在 → "deleted in HEAD"）。
- 强行 reconcile/force-push 风险：可能覆盖 E 组已部署的 `web/` 面板或破坏在线服务。**故未强推**；已恢复 E 组 stash、确认服务 health 200 完好。

**如何收尾（须用户/E 组决策）**：
1. 与 E 组对齐 `ridge-cloud` 的 **canonical 源**：是以本地 refactor 为准（含 static_host 模块化），还是以 deployed 单体为准。
2. 选定后二选一：
   - 若本地为准：在生产仓上 reconcile（备份 deployed `web/` → rebase/merge 本地到 dokku/main → `git push dokku main`）。
   - 若 deployed 为准：把 `fff01da` 的 S6 改动 port 到 deployed 代码结构上重新提交。
3. 部署后 chrome-devtools 验证 `app.*` 公网加载 controller。

> 注：本仓库（wind）侧 S6 所需的前端产物 `web-remote-dist` 已是 CSP-fixed 版本，`pnpm build:desktop-web` 可重产。S6 的 gap **纯在 ridge-cloud 部署侧**。

---

## G2 — D-GM-10 E2EE 公钥 ↔ 身份绑定（未实现，跨仓库安全硬化）

**目标**：cloud 链路的 X25519 公钥与设备配对身份 / JWT 做绑定校验（原计划 §5.5），防止 relay 被攻破时的 MITM。

**Gap 在哪里**：
- **当前实现 = relay-trust interim**：cloud E2EE（X25519 + ChaCha20-Poly1305）握手已工作（invoke/文件树 live 往返实证），但公钥**未与配对身份强绑定**——信任 relay 转发的对端公钥。
- 缺的是：
  - **wind 侧**：`src/lib/remote/cloud/` 握手处增加"对端公钥须匹配配对时登记的指纹"校验。
  - **ridge-cloud 侧**：`/device/activate` / room 加入时登记并下发对端公钥指纹（落在分叉的生产仓，与 G1 同一仓库）。
  - **S0 契约**：握手帧需新增公钥指纹字段（改契约 = 影响双端，需同步）。

**阻塞类别**：跨仓库（wind + ridge-cloud）+ 改契约的安全硬化；且 ridge-cloud 侧正是 G1 的分叉仓库，须先解 G1 的 canonical 源。

**如何收尾**：解 G1 后，在 S0 契约加公钥指纹字段 → ridge-cloud 配对登记/下发指纹 → wind 握手校验 → cloud e2e 复测 MITM 防护。relay-trust 在此之前为可接受 interim。

---

## G3 — `get_directory_children` 经云懒加载返回空（小 bug）

**目标**：cloud 模式下展开文件树子目录（懒加载）能正确返回子项。

**Gap 在哪里（已定位代码）**：
- `src/lib/stores/fileExplorer.ts:462-491` — `loadChildrenPage` 走 `invoke<DirectoryPage>('get_directory_children', {path, offset, limit})`（**真 invoke，非 mock**）。
- `:487` 的 `catch` 把运行时抛错**静默吞成 `{entries: []}`** → UI 显示"空目录"。
- 根树 `get_file_tree` 经云**正常**（live 实证渲染出 host 真实仓库树），只有懒加载子目录这一路返回空。
- **根因尚未确定**：怀疑 cloud invoke 路径下 `get_directory_children` 的参数传递 / 命令路由与 `get_file_tree` 有差异，导致 host 侧抛错被吞。

**阻塞类别**：需 live console 错误（须重建 host build/ + finicky 双端连接才能看到被吞的真实错误）。

**如何收尾**：
1. 临时改 `:487` catch 把错误 `console.error` 出来（而非静默吞），或经云透传错误。
2. 重建 + 双端连接，展开 `docs` 子目录，读 controller console 的真实错误。
3. 按错误（参数名 / 路由 / 沙箱）修 host 侧 `get_directory_children` 的 cloud 路径。
4. 顺带修 `:487` 的静默吞错（违反"never silently swallow errors"），至少 log。

---

## G4 — 剩余 handler 迁移到 ridge-core（增量，非阻塞）

**目标**：把 git / terminal / workspace / pane 等命令也下沉到 `packages/ridge-core`（S1 的完整收口），完成 D10 全量屏缓冲与 fs **写**命令迁移。

**Gap 在哪里**：详见 `s1-migration-ledger.md`。当前已迁移：settings/theme（S1）、fs 只读 + search + tree（S5）。**未迁移**：
- git / terminal / workspace / pane handler —— 绑 **D11 领域模型**，需先定义 pane/workspace 的运行时无关抽象。
- **全量 D10 屏缓冲**（当前仅 scaffold，D-GM-6 决定切 S5/后续）。
- fs **写**命令 —— **前置硬门 D-GM-9**：headless 沙箱当前是 no-op（`OutsideSandbox` 仅在非空根时生效，headless 无根 = 不限制），写命令迁移前必须先让 headless 沙箱真正 enforce 根范围，否则 ridge-cli 写能力无边界。

**阻塞类别**：增量工作（Rust，`cargo check` 可验，无外部阻塞），留后续会话。**不影响现有功能**（src-tauri 仍持有这些 handler 的现行实现，行为不变）。

**如何收尾**：按 ledger 逐 handler 迁移；fs 写命令前先实现 D-GM-9 headless 沙箱 enforce。

---

## G5 — live 双端终端 e2e 实测（接线完成，待验证）

**目标**：实测验证 cloud 模式下终端 PTY 双向流（输出 host→controller、输入 controller→host）。

**Gap 在哪里**：
- **代码已接通**（commit `26b3207`，svelte-check 0 / cloud vitest 60 passed）：
  - 输出：`src/lib/remote/cloud/cloudPaneSource.ts` 订阅 host `pty-output-{ws}-{pane}` Tauri event → `0x10` 帧（与 LAN RawBytes 字节级一致）→ CloudPanel 注入。
  - 输入：天然打通 —— 桌面终端 `RidgePane.svelte:784` / `manager.ts:1777` 的 `invoke('write_to_pty',{paneId,data})` 在 cloud 模式被隧道成 JSON-RPC invoke → `cloudHostBridge` 既有 invoke 路由 → host 本地写 PTY。
- **未做的只是 live 实测**：开一个终端 pane，确认 PTY 输出经云回显、按键经云写入。

**阻塞类别**：需重建（host build/ 须含 `cloudPaneSource`）+ finicky 双端编排（host+controller 须同时在线，host 信令空闲会超时断；时序敏感）。

**如何收尾**：重建 → 重新配对设备（premium 账号 `s4-test@ridge.test` / `S4testpass!2026` 已 premium + username `s4test`）→ 启 host（`--remote-debugging-port=9222` + 注入 cloud auth localStorage）→ 静态服 fresh web-remote-dist → controller `?cloudHost=<device>&u=s4test` 连入 → 开终端 pane 验证双向。

**附带小项 G5b — 首帧 scrollback（cosmetic）**：D10 控制端订阅后才见后续输出、不回放历史 scrollback。需全量 D10 屏缓冲（见 G4）。纯 cosmetic，不影响功能。

---

## G6 — wdio / perf 原生壳 about:blank（预存在工具链问题，非本次回归）

**Gap 在哪里**：
- `pnpm e2e:shell`（wdio + tauri-driver 驱动原生 app）首跑 0/10 = "never left about:blank"。
- **我引入的部分已修**（commit `7be7381`）：workspace 重定位后 `wdio.conf.ts` 仍指旧 `src-tauri/target/release/ridge.exe`、perf 脚本进程过滤仍按 `src-tauri\target\` → 已全部改为根 `target/release`。
- **预存在、非本次回归的部分**：即便指向正确新二进制仍 about:blank，根因是 `wdio.conf.ts §1.35` 注释记录的 **WebView2 user-data 独占锁 / BiDi 导航挂起**（与已安装 `C:\Program Files\ridge\ridge.exe` 共享 identifier）。重新启用 `WEBVIEW2_USER_DATA_FOLDER` 隔离后仍挂（139 次轮询）→ 更深的 tauri-driver / WebView2(148) 自动化导航挂起。
- perf（`e2e:perf`）走同一 wdio 壳 → 同 about:blank 阻塞 → **未取得新基线**。

**阻塞类别**：预存在 WebView2 / tauri-driver 工具链问题（前人调试中），超出 unified-remote 范围。**app 运行时正确性已由 Phase A 手动 + chrome-devtools 全 UI + LAN WSS e2e 全绿实证覆盖。**

**如何收尾**：属 tauri-driver / WebView2 自动化壳的独立调试任务，与本计划解耦。

---

## 收尾路线建议（按依赖排序）

1. **用户本机 rebuild 复核** S1/S3/S5 桌面回归 + LAN 远控（Phase A 已 GM 实证，此为用户独立确认）。
2. **push**：wind develop（27 commit）、ridge-cloud origin（GitHub 备份，clean ff）—— 本次已执行。
3. **解 G1**：与 E 组对齐 ridge-cloud canonical 源 → 部署 S6（解锁后 G2 的 ridge-cloud 侧也可动）。
4. **解 G2**：E2EE 身份绑定（依赖 G1 的仓库就绪 + 改契约）。
5. **小项专会话**：G3（懒加载空）、G5（终端 live 实测）—— 均需重建 + finicky 双端连接，宜单独开会话集中做。
6. **G4 / G6**：增量迁移 / 工具链调试，独立排期，不影响现有功能。
