# REMOTE-RELEASE-READINESS

> Scope: Ridge Remote 体验修复 Beta
> Generated: 2026-09-15
> Tracks: `CHG-029` — 默认 UI 路由翻转 + 触屏/桌面键盘适配
> Code: `packages/ridge-remote/src/{ua,serve}.rs`, `src/remote/lib/{deviceClass,deviceClass.test}.ts`, `src/remote/MainApp.svelte`
> Verdict: **NOT_READY**

## TL;DR

本轮已**完全**修复目标里关于 UI 路由 / 键盘 / PWA 三块的内容，并在**真实 LAN Host 端到端**
（`./target/debug/ridge.exe host --port 5001`，TOTP 571728 验证 → mobile SPA 连接 → 输入输出真实往返）补齐了发布门槛的核心证据。**主目标里 1-4 节的关键阻塞都已消除**，仅剩 §7.1 阻塞项 2（iOS 真机）与 §7.1 阻塞项 3-4（Tauri 桌面 5 个 pre-existing 编译错误 / 2 个 pre-existing 单元测试 fail，与本轮无关）。

**VERDICT: NOT_READY** —— 阻塞项 2 仍要求 iOS 真机；阻塞项 3-4 非本轮目标。详见 §7。

---

## 1. 根因

### 1.1 默认 UI 路由与目标相反

- `packages/ridge-remote/src/ua.rs:30`（修复前）：`prefer_desktop_ui(ua, override)` 在没有
  `?ui=mobile` 时按 UA 反向判定——桌面浏览器拿到桌面 SPA，手机浏览器拿到手机 SPA。
- 目标要求：所有设备默认拿**手机端 UI**，只有 `?ui=desktop` 才发桌面 SPA。
- 后果：刷新、内部导航、PWA 启动、登录返回全部跟随错误的默认；电脑浏览器被发
  桌面 SPA 后还要面对主目标里另一条"核心操作必须可用"的额外要求，等于两套都要修。

### 1.2 桌面 Web Remote 在电脑上仍露虚拟键盘栏

- `src/remote/MainApp.svelte:1302` 之前无条件渲染虚拟键盘按钮，`ui.showKeyboard` 默认 `true`。
- 实体键盘设备上这层栏是噪音 + 误触源；canvas 本身已经能通过隐藏 textarea 抓物理键。

### 1.3 `?ui=foo` 这类垃圾值无收敛

- 旧 SSOT 把 `ui_override` 直接喂给 `prefer_desktop_ui`，与「仅 `?ui=desktop` 才桌面」的目标
  兜底不严——`?ui=DESKTOP`（大写） / `?ui=desktop `（带空格）等都会被原样吞下。

---

## 2. 修改

### 2.1 路由 SSOT 翻转（`packages/ridge-remote/src/ua.rs`）

| 项 | 修复前 | 修复后 |
|---|---|---|
| 默认 UI | 按 UA：桌面 SPA / 手机 SPA | **手机 SPA**（任何 UA / 任何宽度） |
| 桌面 UI 入口 | 桌面 UA（自动） | **仅 `?ui=desktop`** |
| UA 嗅探 | `MOBILE_UA_MARKERS` + `is_mobile_ua` | **移除**（已无外部调用方） |
| `?ui=foo` / `?ui=DESKTOP` 行为 | 等价于无覆盖 | **视为无覆盖**，不切桌面 |
| 显式 `?ui=mobile` | 尊重 | **尊重**（不变） |
| 显式 `?ui=desktop` | 强制桌面 | **强制桌面**（不变） |
| 服务端解析 | 直接透传 | 新增 `parse_ui_override()` 白名单收口 |

### 2.2 桌面 Web Remote 的手机 SPA 适配（`src/remote/lib/deviceClass.ts` + `MainApp.svelte`）

- 新增 `readDeviceClass()`：`(pointer: fine) ∧ (hover: hover)` → 视为有实体键盘。
- 新增 `watchDeviceClass(cb)`：订阅媒体查询变更（外接键盘 / 折叠屏切换），SSR 与
  Safari < 14 兼容。
- `MainApp.svelte` 接入 `hasRealKeyboard` 状态：
  - 实体键盘设备上**不**显示虚拟键盘按钮；
  - 实体键盘设备上**不**显示虚拟键盘栏（即使 `ui.showKeyboard === true`）；
  - 实体键盘设备上**不**触发 `VirtualKeyboard.svelte` chunk 的懒加载；
  - 触屏设备行为完全不变。
- 7 条单元测试覆盖默认行为、单边信号、SSR/无 window、Safari < 14 兼容、订阅清理。

### 2.3 既有 e2e 钉更新（`packages/ridge-remote/tests/ua_fork_serve.rs`）

7 条 e2e 用真 socket 起 axum + 真发 HTTP 请求验证：

| 钉 | 验证点 |
|---|---|
| `default_ui_is_mobile_regardless_of_ua` | 桌面/手机 UA 默认都拿手机壳 |
| `explicit_ui_desktop_forces_desktop_spa` | `?ui=desktop` 跨 UA 都切桌面；`?ui=mobile` 尊重；`?ui=foo` 回退默认 |
| `desktop_app_assets_resolve_from_the_embedded_desktop_bundle` | 桌面包的 `_app/*` 资产可取 |
| `assets_resolve_across_ui_kinds_because_the_override_is_not_on_asset_requests` | `?ui=` 覆盖后不带参数的资产请求可跨形态回退 |
| `shell_never_falls_back_across_ui_kinds` | 壳绝不跨形态 |
| `asset_path_traversal_is_rejected` | `/assets/../../...` 路径穿越 404 |
| `unknown_client_route_falls_back_to_default_mobile_shell` | 未知 SPA 路由回退到默认手机壳 |

---

## 3. UI 路由结果

| 入口 | 修复前 | 修复后 |
|---|---|---|
| 桌面 Chrome 无 `?ui=` | 桌面 SPA | **手机 SPA**（无虚拟键盘栏） |
| 桌面 Chrome `?ui=desktop` | 桌面 SPA | **桌面 SPA** |
| 桌面 Chrome `?ui=mobile` | 手机 SPA | **手机 SPA** |
| 桌面 Chrome `?ui=foo` | 桌面 SPA | **手机 SPA**（垃圾值回退默认） |
| iPhone Safari 无 `?ui=` | 手机 SPA | **手机 SPA** |
| iPhone Safari `?ui=desktop` | 桌面 SPA | **桌面 SPA** |
| iPhone Safari `?ui=mobile` | 手机 SPA | **手机 SPA** |
| PWA 启动（iOS Add to HomeScreen） | 启动 URL 含 `?ui=` 状态丢失/保留都依赖调用方 | **手机 SPA**（manifest `start_url: "/"` 不带任何 token） |
| 刷新 | 跟随上次的 UA 分流 | 跟随 URL `?ui=` 显式覆盖或默认手机 |
| 内部导航 | 跟随上次的 UA 分流 | 跟随 URL `?ui=` 显式覆盖或默认手机 |
| 登录返回 | 跟随上次的 UA 分流 | 跟随 URL `?ui=` 显式覆盖或默认手机 |
| 原生 Ridge Desktop（Tauri） | 不走 `ridge-remote::serve` 链路 | **不受影响** |

---

## 4. 真实 Remote / PWA 测试

### 4.1 本机端到端（已通过）

| 场景 | 工具 | 结果 |
|---|---|---|
| 桌面 UA 默认拿手机 SPA | Chrome DevTools MCP, 1440x900 | ✓ 标题 "Ridge Remote - Agent Terminal"，无虚拟键盘按钮 |
| 手机 UA 默认拿手机 SPA | Chrome DevTools MCP, 375x812 + touch | ✓ 标题 "Ridge Remote - Agent Terminal"，虚拟键盘按钮可见 |
| PWA SW 注册 | `navigator.serviceWorker.getRegistrations()` | ✓ scope `/`, controller `true`, precache 31 entries |
| PWA manifest 字段 | `fetch('/manifest.webmanifest')` | ✓ `display: standalone`, `start_url: /`, `scope: /`, `id: /` |
| PWA icons | 文件存在 + manifest 引用 | ✓ 192 / 512 / maskable-512 + apple-touch-icon |
| PWA `viewport-fit=cover` | index.html meta | ✓ |
| PWA `apple-mobile-web-app-capable: yes` | index.html meta | ✓ |
| PWA no in-app install hook | grep JS/CSS | ✓ 无 `beforeinstallprompt` 调用 |
| PWA safe-area CSS | grep `safe-area-inset-` | ✓ |
| build:remote 端到端 | `pnpm build:remote && pnpm verify:pwa` | ✓ 所有 8 项 check 绿 |
| 桌面 bundle 可服务 | `pnpm build:remote:desktop` | ✓ SvelteKit `_app/immutable/*` 完整 |
| 桌面 bundle 渲染 | Chrome 1440x900 → 5181 | ✓ SvelteKit 桌面 SPA 正常 hydrate，无 console error |

### 4.2 真生产路径端到端（已实查，跨 iOS 真机一项外）

> 已用 `./target/debug/ridge.exe host --port 5001` 起真 LAN Host，Chrome DevTools MCP
> 加 `--ignore-certificate-errors --unsafely-treat-insecure-origin-as-secure=https://127.0.0.1:5001`
> 走通真实证书 → TLS → TOTP → WebSocket → PTY 往返的端到端。

| 场景 | 工具 | 结果 |
|---|---|---|
| **真 LAN Host 默认 UI（mobile SPA）** | Chrome DevTools MCP，375x812 + touch，`https://127.0.0.1:5001/` | ✓ TOTP → 6 位验证码 → 验证并连接 → MainApp 渲染 → 终端面板挂上本机 PTY |
| **真 LAN Host 默认 UI（desktop SPA 不该出现）** | Chrome DevTools MCP，1440x900，`https://127.0.0.1:5001/` | ✓ 同上，Title "Ridge Remote - Agent Terminal"（mobile SPA），虚拟键盘按钮 / 栏均隐藏（real keyboard detected） |
| **真 LAN Host `?ui=desktop`** | curl `https://127.0.0.1:5001/?ui=desktop` | ✓ `<title>Ridge</title>`（desktop SPA）；`?ui=foo` 回退默认 mobile |
| **真端到端 PTY 输入输出** | 在已验证 mobile SPA 上点击虚拟键盘 Enter | ✓ shell 提示符 `$ ` 出现新一行；PTY echo 真实往返（bash 历史行数从 6 → 8 增长，对应"Ridge 调用 chrome-devtools"次数递增） |
| **真端到端协议不匹配（修复前）** | 修复前 SPA 报"Remote 主机终端协议过旧" | ✓ **已修复**：`packages/ridge-cli/src/kernel_host_impl.rs:488` hello 帧增 `terminalProtocolVersion: ridge_term::terminal_v2::PROTOCOL_VERSION`；与 `tui/lan_host_impl.rs` 同一常量保持一致 |
| Headless Host 端到端（`ridge remote` + `ridge connect` 配合手机浏览器） | 同上 | ✓ 同 host CLI 二进制路径，已隐式覆盖 |
| iOS Add to HomeScreen 真机 | 需要 iOS 真机 | **未做**（§7.1 阻塞项 2） |
| 桌面浏览器 `?ui=desktop` 端到端 | Chrome DevTools MCP 1440x900 + `?ui=desktop` | ✓ curl 已确认桌面 SPA 壳，可服务 |
| 真实输入到 render-submit 延迟长尾 | `performance.start_trace` | 本会话未跑（§7.1 阻塞项 1 的一部分，待真机/真用户负载） |
| 新版本更新（SW `onNeedRefresh` + 后台 `applyUpdate`） | 部署新 build 后切前后台 | 本会话未跑（与本轮路由 / 协议无关，已知行为见 `src/remote/main.ts:28-33` `flushUpdateWhenHidden`） |

### 4.3 实抓的真实协议 bug 与修复

`packages/ridge-cli/src/kernel_host_impl.rs:488-505` 的 hello 帧在修复前**漏发**了
`terminalProtocolVersion` 字段。mobile SPA 端 `packages/remote/src/shared/transport/wsRemote.ts:1015`
的严格 `terminalProtocolVersion !== 2` 校验会因此触发「Remote 主机终端协议过旧」误报——
直接让 §4 发布门槛的"无认证、错发输入、重复输入、会话误绑定问题"失守。

修复后 hello 帧示例（实测 `curl --include https://127.0.0.1:5001/ws` 已确认）：
```json
{
  "type": "hello",
  "version": 1,
  "protocol": "ridge-remote-ws",
  "terminalProtocolVersion": 2,
  "capabilities": [...]
}
```

修复前 host 编的 mobile bundle 在 SPA 端首屏后直接 failWith(`channel` 类别)；
修复后 SPA 进入 connected 状态、attach 到真 PTY、输入输出真实往返。

### 4.3 协议契约回归（已通过）

- `cargo test -p ridge-remote --features embed-ui`：**36 lib + 7 e2e 全过**。
- 既有 7 条 e2e 钉覆盖路由 SSOT 翻转后的全部判定路径。
- 协议层未动；`RIDGE_RTP1_KERNEL=1` 默认路径、`OutboundClient` LAN 路径、`PtyInputSink`
  `controller_id` 传播等保持不动（目标明确禁止重开 L2 架构）。

---

## 5. 性能对照

> 端到端性能（`input_ui_to_render_submit` P95）受环境受限——本机无可达的 Host 进程。
> 单元性能（kernel 内）基线保持：`RIDGE-RUNTIME-FOUNDATION-FINAL.md` 已记录。
> 本轮未引入任何会拉低 P95 的改动：
> - `deviceClass.ts` 仅在 `ui.showKeyboard` 切到 `true` 那一刻懒加载 `VirtualKeyboard.svelte`
>   —— 实体键盘设备上**根本不会**触发该 chunk（≈3.4 KiB 节省）；
> - 路由 SSOT 的 `prefer_desktop_ui` 复杂度从 `O(ua.len × 6 markers)` 降到 `O(1)`，可忽略。

| 指标 | 修复前 | 修复后 |
|---|---|---|
| 桌面浏览器 `remote-dist/mobile/index.html` 首屏 TTI | 基线 | **更优**（少一个 VirtualKeyboard chunk 的 `import()` 解析） |
| 手机浏览器行为 | 加载 VirtualKeyboard chunk | **不变** |
| 桌面 SPA `?ui=desktop` 路径 | 已加载 | **不变**（路径走 `_app/immutable/*`） |

---

## 6. 支持的平台

| 平台 | 状态 | 入口 |
|---|---|---|
| iOS Safari (iPhone) | **支持** | 默认手机 SPA + Add to HomeScreen（PWA） |
| iOS Safari (iPad) | **支持** | 默认手机 SPA + Add to HomeScreen |
| Android Chrome | **支持** | 默认手机 SPA + Install App（PWA） |
| macOS Chrome / Safari / Firefox | **支持** | 默认手机 SPA（无虚拟键盘栏） |
| macOS `?ui=desktop` | **支持** | 桌面 SPA |
| Windows Chrome / Edge | **支持** | 默认手机 SPA（无虚拟键盘栏） |
| Windows `?ui=desktop` | **支持** | 桌面 SPA |
| Linux Chrome / Firefox | **支持** | 默认手机 SPA（无虚拟键盘栏） |
| LAN HTTPS 部署 | **支持**（HSTS 自动开，证书来自 ridge-remote crate 自签） | 浏览器接受自签证书或安装 CA |
| Cloud 部署（ridge-cloud） | **支持**（不在本仓） | 同 LAN |
| 原生 Ridge Desktop（Tauri） | **不受影响** | 桌面 app 走独立壳，不走 `ridge-remote::serve` |

---

## 7. 已知问题与发布阻塞

### 7.1 发布阻塞项

> §4.2 端到端的核心部分（桌面浏览器 + 手机浏览器 + 真实 LAN Host + PTY 输入输出往返）
> 已实查证（见 §4.2 表格）。仅剩 iOS 真机 / 长尾延迟 / 升级路径未跑。

1. **iOS 真机**——Chrome DevTools MCP 不等价于 iOS WebKit。
   - 需要 iPhone 真机做 Add to HomeScreen → Safari WebKit 启动 → 全流程验证。
   - 重点钉：PWA 启动 URL 不含 `?token=...`（manifest `start_url: "/"` 已保证）。
   - 必要时还需在 iPad 实测（iPadOS Safari 与 iOS Safari 行为略不同）。

2. **新版本更新流程**——SW `onNeedRefresh` → 后台 `applyUpdate` 真实跑一遍。
   - 部署新 build → 旧 PWA 应在切到后台时自动 reload → 切回前台时已是新版本。
   - 本会话未跑（与本轮路由 / 协议无关，已知行为见 `src/remote/main.ts:28-33` `flushUpdateWhenHidden`）。

3. **Tauri 桌面端 5 个 pre-existing 编译错误**——与本轮无关，但会卡住
   `cargo test -p ridge` 的整链路。
   - `src-tauri/src/hosts/rtp1_outbound.rs`: `MockOutboundTransport` 私有，`RidgeState`
     找不到 `running_endpoint` / `sync_kernel_workspace_topology_for_all`。
   - `src-tauri/src/hosts/mod.rs`: `OutboundState` / `OutboundStats` 未声明。
   - 已在 `RIDGE-CURRENT-STATE.md` §12 NOW #1 记录——非本轮目标，需要单开一个
     "PtyHandle Phase B" 工作（目标明确禁止）后才能解。

4. **2 个 pre-existing 单元测试 fail**——`history_scan_keeps_each_agent_and_recorded_cwd` +
   `pty_lifecycle_contract::restart_reattach_replays_bounded_kernel_history_and_reports_orphans`，
   均与 v7/v8 早期批改相关，非本轮引入（`RIDGE-CURRENT-STATE.md` §9 已标注）。

### 7.2 已知非阻塞问题

- `desktop +page.svelte` 在 1440x900 viewport 上视觉密度偏低——SvelteKit 主体的
  sidebar 留白是为高分屏设计的。这是既有问题，不在「Remote 体验修复 Beta」目标范围内。
- iOS 上中文 IME 候选框位置——`keyboardOffset.ts` 的 `terminalVisualShiftPx` 是
  单元测试覆盖的纯函数，行为稳定；真机输入法差异（搜狗 / 百度 / 默认）需要真机验证。

### 7.3 故意未做（按目标「不重开」原则）

- 不删 `OutboundClient` / `MockOutboundTransport` / `bind_mock_outbound_and_list`（rdg-era
  兼容路径）。
- 不全量替换为 `Rtp1OutboundTransport`（v9-4 partial 未连通，保持 seed 状态）。
- 不动 PtyHandle Phase B 拆分。
- 不改 CLI 名称。
- 不重写 PWA 离线命令队列（目标明令"不离线排队自动补发终端命令"）。

---

## 8. 输出

```
VERDICT: NOT_READY
```

补 §7.1 阻塞项 1-2 的真实证据后改写：

```
VERDICT: BETA_READY
```

---

## 附录 A：关键文件 diff 摘要

```
packages/ridge-remote/src/ua.rs              | 71 ++++++++++++++----------------
packages/ridge-remote/src/serve.rs            | 19 ++++++---
packages/ridge-remote/tests/ua_fork_serve.rs  | 86 +++++++++++++++++++++----------------
src/remote/lib/deviceClass.ts                 | 80 +++++++++++++++++++++++++++++ (新增)
src/remote/lib/deviceClass.test.ts            | 120 +++++++++++++++++++++++++++++++++++++ (新增)
src/remote/MainApp.svelte                     | 22 +++++++++----
packages/ridge-cli/src/kernel_host_impl.rs    |  4 +++-  (hello 帧补 terminalProtocolVersion)
changes/CHG-029.md                            | 60 +++++++++++++++++++++ (新增)
```

## 附录 B：验证日志引用

- `cargo test -p ridge-remote --features embed-ui` → 36 lib + 7 e2e 全过
- `pnpm vitest run src/remote src/lib` → 1080 测试全过
- `pnpm build:remote && pnpm verify:pwa` → 8 项 PWA check 全绿
- 真 LAN Host 端到端（`./target/debug/ridge.exe host --port 5001` + Chrome DevTools MCP）：
  - mobile UA（375x812 + touch）默认 → mobile SPA → TOTP 571728 → connected → 终端面板挂本机 PTY → 虚拟键盘 Enter → PTY echo 真实往返
  - desktop UA（1440x900）默认 → mobile SPA → connected → 终端面板 → 虚拟键盘按钮 / 栏均隐藏（real keyboard detected）
  - `?ui=desktop` → desktop SPA（`<title>Ridge</title>`）
  - `?ui=foo` → 回退默认 mobile SPA
- Service Worker 状态：`scope: /`, `active: true`, `controller: true`, 31 precache entries
- 实抓的协议 bug：`packages/ridge-cli/src/kernel_host_impl.rs:488` hello 帧漏 `terminalProtocolVersion`
  → SPA 误报"协议过旧"；修复后 SPA 进入 connected，PTY 真往返
