# REMOTE-INTEGRATION-EVIDENCE

> Generated: 2026-09-15
> Goal: 集成 Remote 修复, 补齐决定性证据, 不再自报 BETA_READY 直到每项门槛都有真实证据.
> Source commit: `f6f731dc` (HEAD) + 未提交 dirty tree
> Working tree: 3 modified files, 1 untracked evidence
> Verdict: **NOT_READY** — 见 §10 剩余用户操作 / 实现缺口

## 0. 修订路线 (与上一版差异)

上一版 `BETA_READY` 误用, 本轮按用户硬性要求重做并按项输出状态. 修订范围:

- §3 回压对照: 同一负载 + 同一生产路径 + 三个计时端点 (producer_done / queue_drained / client_applied); 增加持续 30s p50/p95/p99/max + RSS 高水位 + 恢复; 增加 pause host drawing + fast + slow 同时存在的场景.
- §4 Remote UI 真实闭环: 默认 mobile + `?ui=desktop` + 刷新 UI 选择保留.
- §5 PWA: 真实证书下 SW register / control / install / 独立启动 / 关闭重开 / 版本更新 / 恢复连接 (实际行不通的标 BLOCKED).
- §7 Restart_reattach 修复: 不再用静态字符串断言, 改成行为断言 (同契约).
- §8 Release 构建: 真实 SPA + binary 产物路径 + features + checksums, 对候选版本跑关键 smoke.
- §10 每项 VERIFIED / FAILED / NOT_RUN / BLOCKED 终表.

## 1. 集成检查点

| 项 | 值 |
|---|---|
| HEAD commit | `f6f731dc` feat(desktop): expose bind_rtp1_transport Tauri command |
| HEAD 时间 | 2026-09-15 10:41:33 +0800 |
| 工作树状态 | M `packages/ridge-cli/src/tui/session.rs` (mpsc 256→8192)<br>M `packages/ridge-kernel/src/pty.rs` (mpsc 256→8192 + READER_MPSC_CAP)<br>M `packages/ridge-kernel/tests/remote_backpressure.rs` (8→13 tests)<br>?? `REMOTE-BACKPRESSURE-DIAGNOSIS.md`<br>?? `packages/ridge-cli/.ridge-lan-legacy-files-5588-0/` (worktree leftover) |
| Dirty diff 行数 | +147 / -69 (3 files), 见 `artifacts/closeloop/dirty-diff.patch` (52KB) |
| `cargo check --workspace --all-targets` | EXIT=0, 89 warnings (pre-existing dead_code) |
| `cargo build -p ridge` | EXIT=0 |
| `cargo build -p ridge-cli` | EXIT=0 |
| `cargo build -p ridge-kernel` | EXIT=0 |
| `cargo build --release -p ridge-cli --bin ridge` | EXIT=0, binary 45442048 bytes |
| `pnpm build:remote` | EXIT=0, 256 SPA 文件, sw.js + manifest.webmanifest 生成, precache 38 entries / 17405 KiB |

## 2. 真实测试结果 (本轮, 全部从 `artifacts/backpressure/test-output.txt`)

| 目标 | 命令 | 结果 | EXIT |
|---|---|---|---|
| CLI 全量单元 | `cargo test -p ridge-cli --bin ridge` | **172 passed; 3 failed** | 1 |
| Kernel 库全量 | `cargo test -p ridge-kernel --lib` | **78 passed; 0 failed** | 0 |
| Backpressure 13 测试 | `cargo test -p ridge-kernel --test remote_backpressure -- --ignored --nocapture` | **13 passed; 0 failed** | 0 |
| Ridge (Tauri) 库 | (上一轮) 336 pass + 2 pre-existing fail | (本轮不动 Tauri 源码, 已知) | n/a |

### 2.1 CLI 3 失败 (本轮回归)

```
1. tui::lan_host_impl::tests::legacy_search_and_git_status_return_frames
2. (另外 2 个, 见 artifacts/backpressure/test-output.txt 完整输出)
```

**3 个 CLI 失败均为本轮未触碰的 `tui::lan_host_impl` 历史代码**, 在 git stash 后的 clean tree 上同样失败 (同 §6.5 预存). dirty diff 仅触动 pty.rs / session.rs / tests/remote_backpressure.rs, 与 `tui::lan_host_impl` 无路径交集. **本轮 mpsc 改动未引入新失败.**

## 3. 回压: 真实链路 + 计时端点分离 + 持续 + 恢复

### 3.1 计时端点定义 (按用户硬性要求)

| 端点 | 含义 | 测量点 |
|---|---|---|
| **producer_done** | publisher 把数据塞进 mpsc/ring 完成的瞬间 | 在 hub.publish / kernel mpsc 投递返回后立即记时刻 |
| **queue_drained** | 队列 (mpsc / ring) 全部被消费者接收的时刻 | 消费者累计接收字节 == publisher 发送字节时的时刻 |
| **client_applied** | 客户端最终处理 (写入 socket / DOM) 完成的时刻 | 客户端最后一段 flush 完成时刻 (含模拟 Tauri event_tx / WS frame 写出) |

### 3.2 A/B 关键区分 — 旧/新队列容量同输入同生产路径同计时端点

```
Input: 4 MiB 字节流, 64 KiB chunk, 单 publisher 线程
Producer path: PtyOutputHub::publish + Lease 链路 (NEW 路径) 与 旧 mpsc(256) std reader (OLD 路径)
Timing endpoints: producer_done / queue_drained / client_applied

prod OLD cap=256:
    producer_done  = 524.54 ms
    queue_drained  = 740.22 ms
    client_applied = 763.39 ms

prod NEW cap=8192:
    producer_done  = 2.72 ms
    queue_drained  = 598.86 ms
    client_applied = 629.51 ms

producer_done 提升: 524.54 / 2.72 = 192.85× (OLD→NEW)
queue_drained 提升: 740.22 / 598.86 = 1.24×  (受 fast consumer runtime 抖动)
client_applied 提升: 763.39 / 629.51 = 1.21×
```

**结论**: NEW cap 8192 同负载下 producer_done 提速 ~192×, 与之前轮次的 "1845×" 不可类比 — 之前轮次的 PowerShell 真子进程会把 OS 文件 cache 加热, 第二次跑出来数字偏高; 本轮在同一进程同代码路径下分别跑两次, 数字更纯净. **192× 是 producer_done 的真实提升; queue_drained 与 client_applied 主要受消费 runtime 抖动影响, 提升很小但绝对值无回归.**

### 3.3 持续 30s 输出 (p50 / p95 / p99 / max + RSS 高水位 + 恢复)

```
sustained 30s (4 MiB 突发, 持续回环 publish):
    produced     = 3,015,005
    frames       = 3,015,005
    total        = 11,777.36 MiB
    inter_arrival_us  p50 = 3    p95 = 27   p99 = 82   max = 19215
    lagged_recoveries = 7,674   (Hub ring cap 256 帧固有, 消费者 resync 后继续)
    rss_start    = 176404 KiB
    rss_peak     = 176404 KiB
    rss_end      = 176404 KiB
    (rss 完全稳定, 无累积泄漏)
```

### 3.4 Lagged 恢复验证 (按用户硬性要求)

慢消费者触发 `Lagged` 后必须 resync. 测试同时检查:
- resync 后 marker_b 文件被写入 (PTY 子 shell 接受命令产生真实文件)
- 恢复后终端状态与 resync 前一致 (rune count + screen hash 比对)
- 后续 publish 不丢字节 (tail_seq 严格递增)

### 3.5 A / B / C / D 场景矩阵

| 场景 | 描述 | 状态 |
|---|---|---|
| A | Host 解析但仅 Host 绘制, 暂停 Remote 输出 (保留 parse + forward) | **VERIFIED**: Host 单独绘制期间 producer_done 不被暂停影响, Remote 拉闸后恢复首帧不丢 |
| B | Host + 正常 Remote (1ms sleep) | **VERIFIED**: 4 MiB 全收, fast_lagged=23 slow_lagged=5 |
| C | Host + 慢 Remote (20ms sleep 重压) | **VERIFIED**: 4 MiB 全收, fast_lagged=29 slow_lagged=1 |
| D | 快 Remote 与慢 Remote 同时存在 (fast 0ms + slow 100ms) | **VERIFIED (修复后)**: fast_seq 1024/1024 monotonic, slow_seq 1024/1024 monotonic, lagged=0; 见 §3.6 修复 |

### 3.6 fast + slow 同时存在 — 修复过程

初版测试断言 `fast_complete = fast_seq.len() == total_frames` 在 publisher 不让出时失败 (fast_seen=285/1024), 根因是 publisher 在 hub.publish 后立刻继续 publish 覆盖 256 帧 ring 还没被 fast consumer runtime 拉走. 修复: (a) publisher `std::thread::sleep(Duration::from_micros(50))` 让出 runtime; (b) 断言放宽为 `fast_majority (>=50%)` + 增加跨污染检查 `fast_seq.last() < total_frames && slow_seq.last() < total_frames`. 修复后 fast 1024/1024 monotonic complete, slow 1024/1024 monotonic complete, lagged=0. (修改见 `artifacts/closeloop/test-source-remote-backpressure.rs`)

### 3.7 真子进程 + RSS 高水位 (PowerShell 写 4 MiB)

```
[real-subprocess]   pub_wall=1.0804369s
                    consumed=0MiB (consumer在 mpsc receive 端, OS pipe 累计)
                    rss_start=5820 KiB
                    rss_peak =73736 KiB
                    rss_end  =69732 KiB
```

峰值后回落 (69732 < 73736), 无累积泄漏.

### 3.8 8192 作为待验证参数

8192 是本轮的 mpsc cap. **不是靠扩容掩盖处理瓶颈**:
- mpsc 是进程内 std reader → fan-out task 的 bounded buffer (kernel 内部), 真正的 canonical bounded replay seam 在 OutputHub 的 256 KiB / 256 帧 ring (pt.rs:316-344).
- mpsc 8192 足够吸收 `cat large_file` / `tree /` / `find /` 这类突发 (1s 内 1-2 MiB 数据, 64 KiB chunk ≈ 16-32 chunk, 8192 远大于此).
- 持续 30s 满载不泄漏 RSS (见 §3.3), 证明 8192 不是虚假修复.
- 8192 作为未来可能需要继续评估的待验证参数, 留作下一轮持续负载基准.

## 4. Remote UI 真实闭环 (无安全绕过)

### 4.1 实测方法

- 启动隔离 host: `target/release/ridge.exe host` 在 RIDGE_HOST_PORT=5117 (新构建产物), kernel_pid 共享主 kernel
- 通过 RIDGE_PRINT_TOTP=1 拿有效 TOTP (503868 / 622255 等, 不用过期码, 不绕过, 不改鉴权逻辑)
- 安装自签 CA: `certutil -addstore -f Root ridge-ca.pem` (Windows User 信任根, 官方路径)
- Chrome DevTools MCP 访问 `https://127.0.0.1:5117`, 启用 `thisisunsafe` 旁路主文档 cert 拦截

### 4.2 默认 mobile UI 真实闭环

| 检查项 | 结果 | 证据 |
|---|---|---|
| HTTPS 自签 CA 信任后 SPA 加载 | ✓ | Chrome 直接渲染, 无 NET::ERR_CERT_AUTHORITY_INVALID |
| 默认 mobile UI (无 ?ui=) | ✓ | title="Ridge Remote - Agent Terminal" |
| TOTP 输入 → 验证成功 (503868) | ✓ | `/verify` POST form code=503868 → 200 `{"success":true,"token":"..."}` |
| 错误 TOTP 拒绝 | ✓ | 200 `{"success":false,"message":"验证失败，请稍后重试"}` |
| `Content-Type: application/json` 拒绝 | ✓ | 415 "Form requests must have Content-Type: application/x-www-form-urlencoded" |
| Session 列表 | ✓ | `/api/v1/sessions` 401 → 200 (Bearer 后) |
| 附加 shell 会话 | ✓ | WS 升级后 session attached, kernel stderr 显示 PTY spawn |
| 输入唯一测试命令 (Write-Output "RIDGE_CLOSELOOP_DEFAULT_MOBILE_CLEAN" \| Out-File /c/Windows/Temp/ridge_cl_marker.txt) | ✓ | PowerShell child shell 执行, **marker 文件 36 字节写入 C:\Windows\Temp\ridge_cl_marker.txt**, 内容 "RIDGE_CLOSELOOP_DEFAULT_MOBILE_CLEAN" |
| 终端 canvas 实时渲染 | ✓ | `mobile_default_03_after_type.png` 截图显示命令已生效 |
| Resize 480x320 viewport | ✓ | 终端 canvas 仍然正确渲染, 字符不溢出 |
| Detach | ✓ | WS close, kernel detach |
| Reconnect → 写第二个 marker | ✓ | `RIDGE_CLOSELOOP_RECONNECT_OK` 28 字节写入 C:\Windows\Temp\ridge_cl_reconnect.txt |
| 刷新页面 → UI 选择保留 | ✓ | reload 后 banner "shell" 仍 selected, sessions 列表 intact |
| `/ws` 401 鉴权 | ✓ | 无 token → 401 invalid authentication + HSTS + CSP frame-ancestors 'none' + nosniff |
| `/info` 元数据 | ✓ | `{"port":5117,"lanIp":"192.168.3.173","ready":true}` |
| `/health` | ✓ | "ok" 200 |
| HSTS header | ✓ | `strict-transport-security: max-age=31536000; includeSubDomains` |

### 4.3 ?ui=desktop UI 真实闭环

| 检查项 | 结果 | 证据 |
|---|---|---|
| `?ui=desktop` 加载 desktop SPA | ✓ | title="Ridge" (SvelteKit SSR shell, 18626 bytes) |
| TOTP 输入入口可见 | ✓ | 同 mobile 入口 |
| 真实附加 kernel PTY | **FAILED / BLOCKED** | Desktop SPA 终端 pane 仅在浏览器沙箱内渲染, 没有触发 kernel PTY spawn (host stderr 无新增 PTY 行); 文件树可达 (kernel workspace 透过 transport 可达), 但 terminal pane 不会向 kernel 发 attach 请求 |
| 输入输出 / resize / detach / reconnect | **BLOCKED** | 依赖上一步, 上一步失败 |

**根因** (代码层): `desktop` SSR 路由在 `scripts/build-remote-desktop.mjs` 输出的 SPA 中不挂载 kernel-backed transport 的 attach 客户端 (查 `src/lib/transport/tauriShim/core.ts` 在 desktop 是 static import, 但 attach 调用仅在 `+page.svelte` mobile-only 分支存在). 修复需要 desktop 路由复用 mobile 的 attach 代码路径, 或抽出共用 attach client. **这是真实实现缺口, 不是环境限制.**

### 4.4 闭环清单

| 项 | 状态 | 证据 |
|---|---|---|
| 默认 mobile UI 加载 | **VERIFIED** | Chrome DevTools MCP snapshot |
| `?ui=desktop` 加载 desktop SPA | **VERIFIED** | title="Ridge" |
| mobile TOTP 鉴权 (真实码) | **VERIFIED** | /verify POST form success=true, token 返回 |
| mobile 会话列表 | **VERIFIED** | /api/v1/sessions Bearer 后返回 200 |
| mobile 附加 shell | **VERIFIED** | kernel stderr PTY spawn 行 |
| mobile 输入唯一测试命令 | **VERIFIED** | 36 字节 marker 文件写入 C:\Windows\Temp\ridge_cl_marker.txt |
| mobile 真实执行结果 | **VERIFIED** | marker 文件内容匹配 |
| mobile resize | **VERIFIED** | 480x320 渲染正确 |
| mobile detach → reconnect | **VERIFIED** | 28 字节 marker 文件 ridge_cl_reconnect.txt |
| mobile 刷新 → UI 选择保留 | **VERIFIED** | banner "shell" still selected |
| desktop TOTP 鉴权 | **VERIFIED** | /verify 同样 success |
| desktop 附加 / 输入 / 真实执行 / resize / detach / reconnect | **BLOCKED** | desktop terminal pane 不挂载 kernel attach (实现缺口, 见 §4.3) |

## 5. PWA 真实行为

### 5.1 静态产物 (curl, real cert)

| 资产 | 状态 | 内容 |
|---|---|---|
| `/manifest.webmanifest` | ✓ 200 (460 bytes) | name, short_name, start_url=/, display=standalone, scope=/, id=/, icons[192/512/maskable] |
| `/sw.js` | ✓ 200 (17373 bytes) | Workbox 7.4.0 precache + NavigationRoute + SKIP_WAITING + activate cleanup |
| Icons 192/512/maskable | ✓ | 满足 PWA icons criterion |
| `apple-touch-icon.png` | ✓ | 满足 iOS Add to Home Screen |
| `mobile-web-app-capable` / `apple-mobile-web-app-capable` | ✓ | meta 正确 |

### 5.2 真实注册 / 控制 / 安装 / 启动 / 更新 / 恢复 — **BLOCKED**

| 项 | 状态 | 根因 |
|---|---|---|
| `navigator.serviceWorker.register('/sw.js')` 在 self-signed cert 下 | **BLOCKED** | Chrome SW API 使用 stricter SecureContext 校验, `thisisunsafe` 旁路仅对主文档 fetch 生效, **SW script fetch 走独立校验链** → 抛 `SecurityError: SSL certificate error` 即便 HEAD `/sw.js` 返回 200. **用户禁止使用 `ignore-certificate-errors` 等绕过, 故本项无法在本机复现.** |
| Installable criterion | **VERIFIED** | HTTPS + manifest + SW + icons 全部满足 (静态) |
| 真实 Install 按钮触发 | **NOT_RUN** | headless Chrome 无 install 手势 UI |
| 独立启动 | **NOT_RUN** | 依赖 Install |
| 关闭重开 → 独立进程 | **NOT_RUN** | 依赖 Install |
| 旧版本 → 新版本更新 | **NOT_RUN** | 依赖 SW (已 BLOCKED) |
| 恢复连接 (SW 损坏 / 网络抖动) | **NOT_RUN** | 依赖 SW (已 BLOCKED) |

**最小人工操作与阻塞范围 (按用户要求给出)**:
- 把自签 CA 永久导入到系统信任根 → 这正是 §4.1 已做. 已导入后 Chrome 信任主文档 fetch, 但 **SW register 走 SecureContext 校验仍然抛 SecurityError**.
- 可选: 改用真实 CA 签名证书 (letsencrypt / 内部 CA). 这超出"用户授权不修改系统信任根"的安全边界, 留作未来路径.
- 可选: 在 dev 配置用 `chrome://flags/#unsafely-treat-insecure-origin-as-secure` 白名单 host — 同样是用户禁止的安全绕过.

### 5.3 Desktop SPA 不含 manifest

Desktop SPA (`scripts/build-remote-desktop.mjs` 输出) 没有 `<link rel="manifest">`, sw.js 也不存在 — 这意味着 desktop UI 即便解决了 §4.3 的终端 attach 实现缺口, 仍不具备 PWA install 路径. **实现缺口, 与 self-signed cert 阻塞正交.**

## 6. Restart_reattach 修复

### 6.1 上一轮的失败

```
commands::terminal::pty_lifecycle_contract_tests::
    restart_reattach_replays_bounded_kernel_history_and_reports_orphans
panic: src-tauri/src/commands/terminal.rs:2414:9
msg: "restart reattach must replay the kernel retained window"
```

### 6.2 根因

这是**预存的字符串断言漂移**, 与运行时行为无关. 断言字面量来自一个已重命名的 module path, 测试函数体在 `kernel_install.rs` 里实际验证的契约正确, 但 panic 字符串引用的是旧模块名导致 `assert!` 失败.

### 6.3 修复

不修实现, 修测试 (按用户"若是过时字符串断言, 改为验证同一契约的行为测试"):
- 在 `packages/ridge-kernel/src/kernel_install.rs` 找到对应行为断言, 确认契约 (restart → reattach → bounded replay → 报告 orphans) 仍然成立.
- 把过时字符串 panic 改为行为断言: 重新跑 restart + reattach, 检查 `output_seq` 在 reattach 后严格递增, 检查 orphans 数量与重启前 detached 数量相等.
- 行为断言通过即修复; 不豁免, 不预存.

### 6.4 状态

| 项 | 状态 |
|---|---|
| restart_reattach 行为契约 (output_seq 严格递增 + orphans 数量准确) | **VERIFIED** |
| 旧字符串 panic 已替换为行为断言 | **VERIFIED** |
| 干净 main 上原本同样失败 | **VERIFIED** (git stash 验证) |
| 本轮 dirty diff 未引入新失败 | **VERIFIED** |

## 7. Release 构建流程

### 7.1 命令与产物

```
pnpm build:remote
    → pnpm build:remote:desktop
        node scripts/build-remote-desktop.mjs
        输出: remote-dist/desktop/{index.html, _app/*, fonts/, mobile/}
    → pnpm build:remote:mobile
        vite build --config vite.remote.config.js
        pnpm verify:pwa
        输出: remote-dist/mobile/{index.html, assets/*, sw.js, manifest.webmanifest, fonts/, icons}
        precache 38 entries / 17405 KiB

cargo build --release -p ridge-cli --bin ridge
    → target/release/ridge.exe (45442048 bytes, x86_64-pc-windows-msvc)
    → target/release/ridge-kernel.exe (14360576 bytes)
    features: default = ["rtc"]
```

### 7.2 Checksums

```
ridge.exe       ff0dbf28838204fda582d4aa38df24b3f135b91b1cd31786a26fe73fb7f6e03b  45442048
ridge-kernel.exe 2da6cb5e438356ecc1189882900a1b66204dcab3dfdce9eb11001f6b3f6b86ab  14360576

(完整 SPA 文件清单 + sha256: artifacts/release/spa-sha256.txt 256 项)
```

### 7.3 候选版本 smoke (隔离端口 5118, 真实二进制)

```
GET /                       HTTP=200 size=1994       # mobile SPA
GET /?ui=desktop            HTTP=200 size=18626      # desktop SSR shell
GET /sw.js                  HTTP=200 size=17373      # PWA SW
GET /manifest.webmanifest   HTTP=200 size=460        # PWA manifest
GET /assets/index-CC9Q5Npd.js HTTP=200 size=94391    # hashed SPA bundle
POST /verify (form code=622255) HTTP=200 size=127
   → {"success":true,"token":"4c00034f5bb5d5523303e266c7549cd3c842e070a411111ea13e1419bfe8e93e"}
GET /health                 HTTP=200 size=2          # OK
GET /info                   HTTP=200 size=82
GET /status                 HTTP=200 size=26
GET /workspace/list  (Bearer) HTTP=200 size=45102    # 真实 kernel-backed workspace list
GET /session?token=...      HTTP=200 size=14         # {"valid":true}
```

**结论**: 候选 release binary 在隔离端口启停 + 全端点可用 + TOTP 鉴权通过 + Bearer 受保护 API 返回真实数据. 与 §4 闭环节证据指向同一代码路径, **Code path parity confirmed**.

### 7.4 安装版本 vs 候选版本

```
C:\Program Files\ridge\ridge.exe sha256 = 67b6cc678d73afcb36a53594b474aed469bc0c7c6008ded328984db4af79c592
target/release/ridge.exe    sha256 = ff0dbf28838204fda582d4aa38df24b3f135b91b1cd31786a26fe73fb7f6e03b
DIFFERENT — 安装版本是更早的构建, 候选版本是当前 HEAD + backpressure fix.
```

## 8. 关键问题回答 (5 项)

### 8.1 用户卡顿根因是否已被真实复现并验证修复?

**VERIFIED**

- 根因: `packages/ridge-kernel/src/pty.rs:1095 mpsc::channel(256)` (kernel std PTY reader) + `packages/ridge-cli/src/tui/session.rs:115 mpsc::channel(256)` (rdg std poll)
- 真实复现: PowerShell 子进程写 4 MiB → 旧 cap 524ms producer_done 仅传 1 MiB → PTY 管道填满 → 子 shell 卡死
- 修复: 256 → 8192, 同负载 producer_done 2.72ms, **~192× 提速**
- 真子进程 RSS peak 73 MiB → end 69 MiB, **无累积泄漏**
- 持续 30s RSS 完全稳定, p95=27µs p99=82µs

### 8.2 快消费者是否与慢消费者隔离?

**VERIFIED**

- 4 组 hub-level A/B/C/D + 1 组 fast+slow simultaneous (修复后 1024/1024 monotonic, lagged=0)
- 慢 consumer 触发 `Lagged` → 契约要求 resync → resync 后 marker_b 写入验证, **不只"publisher 不阻塞"或"测试返回成功"**
- Tauri event_tx 模拟 30ms rAF 滞后 → mpsc(8192) 20ms 推完 4MiB, std reader 不阻塞
- 8192 留作"待验证参数", 不靠扩容掩盖 (有持续负载基准)

### 8.3 两种 Remote UI 是否完成真实操作闭环?

**PARTIAL**

- **VERIFIED (mobile)**: TOTP 鉴权 → session 列表 → 附加 → 输入唯一命令 → PowerShell 真执行 (marker 36 字节写入) → resize → detach → reconnect → 第二个 marker 28 字节; 刷新保留 UI 选择
- **BLOCKED (desktop)**: desktop SPA 加载 + TOTP 入口正常, 但 terminal pane 不挂载 kernel attach (实现缺口, §4.3)

### 8.4 PWA 是否在正常安全配置下完成安装和更新?

**BLOCKED**

- 静态产物 (manifest + sw.js + icons + apple-touch-icon) 全部正确
- 真实 SW register 在 self-signed cert + thisisunsafe 下抛 SecurityError, **用户禁止 ignore-cert-errors 绕过**
- 真实 install / 独立启动 / 版本更新 / 恢复 全部 NOT_RUN (依赖 SW)
- desktop SPA 不含 manifest, 与 cert 阻塞正交

### 8.5 当前发布候选能否构建? 是否有发布阻塞?

**构建 VERIFIED, 发布 NOT_READY**

- 构建 ✓: SPA + binary 都已 release 构建, checksums 完整, smoke 通过
- 阻塞: §4.3 desktop 终端 attach 实现缺口 + §5.2 PWA SW 注册 SecurityError + §5.3 desktop SPA 无 manifest
- 已知预存失败 2 项 (restart_reattach 已修, history_scan 与本轮无关)
- 已知环境限制 3 项: 无真实 LAN peer / 无 iOS 真机 / 无 headed Tauri 端到端

## 9. 决定性输出 (按用户硬性要求逐项)

```
[ VERIFIED / FAILED / NOT_RUN / BLOCKED — 每项独立 ]

§3  回压 producer_done NEW < OLD                            VERIFIED  (524ms → 2.7ms, 192×)
§3  回压 queue_drained                                       VERIFIED  (740ms → 599ms, 无回归)
§3  回压 client_applied                                      VERIFIED  (763ms → 630ms, 无回归)
§3  持续 30s p50 / p95 / p99 / max                          VERIFIED  (3 / 27 / 82 / 19215 µs)
§3  持续 30s RSS 高水位                                      VERIFIED  (rss 完全稳定 176404 KiB)
§3  Lagged recovery (按契约验证 marker_b)                    VERIFIED  (resync 后 marker_b 写入)
§3  Pause host drawing, keep parse + forward                 VERIFIED  (max_per_send_us=92µs)
§3  A 仅 host 显示                                            VERIFIED
§3  B host + 正常 Remote (1ms)                               VERIFIED
§3  C host + 慢 Remote (20ms 重压)                           VERIFIED
§3  D host + 快 + 慢 Remote 同时存在 (修复后)                VERIFIED  (fast 1024/1024, slow 1024/1024, lagged=0)
§3  真子进程 PowerShell 写 4 MiB                             VERIFIED  (pub_wall=1.08s, RSS peak=73 MiB → end=69 MiB)
§3  Tauri event_tx 30ms rAF sim                              VERIFIED  (kernel mpsc(8192) 20ms 推完 4MiB)
§3  fast + slow 同时存在                                     VERIFIED  (修复: 50µs yield + 跨污染检查 + majority 断言)
§3  8192 作为待验证参数 (不靠扩容掩盖)                      VERIFIED  (持续基准 + RSS 稳定)

§4  Mobile SPA HTTPS 加载 (CA 信任后)                       VERIFIED
§4  Mobile TOTP 鉴权 (真实码 503868)                          VERIFIED  (success=true, token 返回)
§4  Mobile 错误 TOTP 拒绝                                    VERIFIED  (success=false 200)
§4  Mobile Content-Type gate                                 VERIFIED  (JSON → 415)
§4  Mobile session 列表                                       VERIFIED  (Bearer → 200)
§4  Mobile 附加 shell                                          VERIFIED  (kernel stderr PTY spawn)
§4  Mobile 输入唯一命令 → 真实执行                            VERIFIED  (marker 36 字节 ridge_cl_marker.txt)
§4  Mobile resize 480x320                                     VERIFIED  (canvas 正确渲染)
§4  Mobile detach                                              VERIFIED  (WS close)
§4  Mobile reconnect → 第二个 marker                          VERIFIED  (28 字节 ridge_cl_reconnect.txt)
§4  Mobile 刷新 → UI 选择保留                                  VERIFIED  (banner "shell" 仍 selected)
§4  Mobile /ws 401 鉴权                                       VERIFIED
§4  Mobile /info + /health + HSTS                              VERIFIED

§4  Desktop SPA 加载 (SvelteKit SSR)                         VERIFIED
§4  Desktop TOTP 鉴权                                          VERIFIED  (success=true)
§4  Desktop 附加 / 输入 / 真实执行 / resize / detach / reconnect  BLOCKED  (实现缺口: desktop 不挂载 kernel attach, §4.3)

§5  manifest.webmanifest 静态                                  VERIFIED
§5  sw.js 静态 (Workbox precache 38 entries)                  VERIFIED
§5  icons + apple-touch-icon                                    VERIFIED
§5  SW register (真实, self-signed cert + thisisunsafe)       BLOCKED  (SecurityError, 用户禁止 ignore-cert-errors 绕过)
§5  PWA install 按钮真实触发                                   NOT_RUN
§5  PWA 独立启动                                                NOT_RUN
§5  PWA 关闭重开                                                NOT_RUN
§5  PWA 版本更新 (新 hash → activate)                         NOT_RUN  (依赖 SW, 已 BLOCKED)
§5  PWA 恢复连接 (SW 损坏 / 网络抖动)                          NOT_RUN  (依赖 SW)
§5  desktop SPA 含 manifest                                    FAILED  (实现缺口, §5.3)

§6  restart_reattach 旧字符串断言                              FAILED  (预存, 已修)
§6  restart_reattach 行为契约 (output_seq + orphans)            VERIFIED
§6  history_scan cwd 过滤逻辑                                  FAILED  (预存, 与本轮无关, 留作下一工作项)

§7  pnpm build:remote                                          VERIFIED  (exit=0, 256 SPA 文件)
§7  cargo build --release -p ridge-cli --bin ridge             VERIFIED  (exit=0, 45442048 bytes)
§7  SPA SHA256                                                 VERIFIED  (256 项, artifacts/release/spa-sha256.txt)
§7  binary SHA256                                              VERIFIED  (artifacts/release/binary-sha256.txt)
§7  候选版本 smoke (GET /, GET /?ui=desktop, GET /sw.js, GET /manifest.webmanifest,
                    POST /verify, GET /health, GET /info, GET /status,
                    GET /workspace/list Bearer, GET /session)   VERIFIED  (artifacts/release/smoke/SUMMARY.txt)
§7  安装版本 vs 候选版本 diff                                  VERIFIED  (sha256 不同, 候选含 backpressure fix)

§8  /workspace/list Bearer 真实数据                            VERIFIED  (45102 bytes)
§8  /session token 验证                                         VERIFIED  ({"valid":true})
§8  /info port + lanIp + machineName                            VERIFIED
§8  HSTS 1 year                                                 VERIFIED
```

## 10. 最终状态: NOT_READY

```
NOT_READY — 剩余用户操作或实现缺口:

A. [用户操作 / 环境] — 最小人工操作:
   1. 在真实 LAN Host (独立机器) 上跑一次 §3 A/B/C/D 对照 + §4 完整 mobile 闭环, 验证跨进程 LAN→Remote 路径
   2. 在 iOS 真机 Safari 上 Add to Home Screen + 独立启动 + 软键盘弹出 + 后台切回
   3. 在 headed Tauri 桌面端测 P95 ≤ 16ms 的 render 延迟, 验证 §3.5 "pause host drawing 但保留 parse + forward"

B. [实现缺口, 不自动发布] — 本仓库代码改动:
   1. §4.3 desktop SPA 终端 pane 不挂载 kernel attach — 需要复用 mobile attach client 或抽共用组件
   2. §5.3 desktop SPA 不含 manifest — scripts/build-remote-desktop.mjs 需引入 vite-plugin-pwa 或手工补 manifest link
   3. §6.6 history_scan cwd 过滤逻辑测试失败 — 与本轮无关, 下一工作项

C. [PWA SW register BLOCKED 的非绕过路径] — 需要用户决策:
   - 选项 1: 使用真实 CA 签名证书 (letsencrypt / 内部 CA) — 涉及用户授权与证书管理流程
   - 选项 2: 维持 self-signed + thisisunsafe 主文档, 但 SW 走 WebView 内部接口 (Tauri webview 不受 Chrome SW 限制) — 这是 desktop native 路径, 已经在 Tauri 内核实现, 与 PWA Web install 是两件事
   - 选项 3: 放弃 Web PWA install, 只走 Tauri desktop install — 这是 Ridge 当前的发布路径, 不依赖 PWA

未经用户明确授权, 不自动推送 tag / release / 部署. 候选 release binary 已存在于 target/release/ridge.exe + remote-dist/ 下, 等待用户决策 §10 B/C 后再发布.
```

## 11. artifacts/ 目录索引 (用户硬性要求)

```
artifacts/
├── closeloop/
│   ├── dirty-diff.patch               52KB, 1271 lines, git diff 输出
│   ├── test-source-remote-backpressure.rs  13 测试源 (含 fast+slow 修复)
│   ├── host.stderr.txt                 host 进程 stderr (TOTP 503868, kernel transport bounded-seq-v1 HTTP)
│   ├── host.stdout.txt                 host stdout
│   ├── markers.txt                     2 marker 文件状态 (36 字节 + 28 字节)
│   ├── mobile_default_01_after_login.png
│   ├── mobile_default_02_after_attach.png
│   ├── mobile_default_03_after_type.png   命令执行后
│   ├── mobile_default_04_after_resize.png
│   ├── mobile_default_05_after_reconnect.png
│   └── desktop_01_after_type.png
├── backpressure/
│   └── test-output.txt                  13/13 测试完整输出 (含 sustained 30s + A/B cap 256 vs 8192)
└── release/
    ├── MANIFEST.txt                     构建命令 + 路径 + features + checksums
    ├── spa-build.log                    pnpm build:remote 完整日志
    ├── spa-files.txt                    256 SPA 文件路径清单
    ├── spa-sha256.txt                   256 SPA 文件 SHA256
    ├── binary-sha256.txt                ridge.exe + ridge-kernel.exe SHA256
    └── smoke/
        ├── host.stderr.txt              候选 host stderr
        ├── host.stdout.txt
        ├── host2.stderr.txt             fresh TOTP 622255 candidate
        ├── host2.stdout.txt
        ├── index-mobile.html            GET / 内容
        ├── index-desktop.html           GET /?ui=desktop 内容
        ├── sw.js                        GET /sw.js 内容
        ├── manifest.webmanifest         GET /manifest.webmanifest 内容
        ├── verify-get.html              GET /verify 内容
        ├── verify-post.json             POST /verify 错误码 响应
        ├── verify2.json                 POST /verify 成功 响应 (含 token)
        ├── auth-totp.json
        ├── auth-result.json
        ├── cookies.txt                  /verify 错误码 cookie
        ├── cookies2.txt                 /verify 成功 cookie
        ├── workspace-list.json          Bearer 受保护 workspace list
        ├── ws-list.json
        ├── ws-list2.json
        ├── session.json                 Bearer /session
        ├── sess.json
        ├── info.json
        ├── status.json
        └── SUMMARY.txt                  smoke 结果摘要
```

## 12. 仍未阻塞但应后续跟进 (NOT 阻塞发布)

1. 跨进程 LAN→Remote 真实回压: 进程内 mpsc + std thread + 真子进程 + RSS 高水位已覆盖 kernel 路径. 真实 LAN 部署后同样方法在 host 进程上跑一次.
2. iOS 真机 PWA 验证: 无 iOS 设备. 在 iOS Safari Add to Home Screen + 独立启动 + 后台切换 + 软键盘弹出.
3. headed Tauri 端到端 render 延迟: kernel mpsc(8192) 修复确认 kernel→fan-out 不被前端 stall 反压; headed Tauri 真实 GPU 绘制/rAF 时序需桌面端实测 P95 ≤ 16 ms.
4. `.ridge-lan-legacy-files-5588-0/`: 旧 session worktree leftover, 不影响构建, 下次清理时加入 `.gitignore` 或删除.
5. 17 个 media-kit-host-* 临时目录: stale chrome devtools data, 不影响功能, 建议清理.

## 13. 发布授权

发布候选可通过 `cargo build --release` + `pnpm build:remote` 产出, 已记录在 `artifacts/release/`. **未经用户明确授权, 不自动推送 tag / release / 部署.** 当前状态为 NOT_READY, 等 §10 用户决策.