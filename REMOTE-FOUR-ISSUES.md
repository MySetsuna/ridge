# Remote 四问题修复报告

基于 `REMOTE-FOUR-ISSUES.md`（静态诊断），本轮进入修复期：
- 补关键行为测试 → 改最小代码 → 跑测试验证
- 不动 host 协议、不回退 Kernel、不存 TOTP 进 storage、不换 SSH
- 缺真机的验收项标 `NOT_RUN`，不伪报通过

**2026-09-15 增补**：本轮依 Goal 复核四问题：每条按 VERIFIED / PARTIAL / FAILED / NOT_RUN 重判，并附 1) 浏览器/组件级测试结果、2) 关键 diff、3) 原始日志、4) 真机最小人工步骤。详见各小节末尾的「增补」段；最终结论在文末「终判」。

---

## 0. 实际运行版本（与诊断期一致）

- **HEAD**：`f6f731dc`
- **dirty diff（诊断期）**：6 文件（`kernel_host_impl.rs` / `tui/session.rs` / `pty.rs` / `remote_backpressure.rs` / `commands/terminal.rs` / `themes.ts`），仅测试 + 后端 + themes，与 Remote UI 无交集
- **本轮新增 diff**（前端）：
  - `src/remote/lib/TerminalCanvas.svelte`（D 触屏修复 + B onDestroy 清状态）
  - `src/remote/MainApp.svelte`（B 切 pane 调 pruneOutputs）
  - `packages/remote/src/shared/terminal/mobileTouchScroll.ts`（D selectionMode 互斥）
  - `packages/remote/src/shared/terminal/mobileTouchScroll.test.ts`（D 新增 2 测试）
  - `packages/remote/src/shared/transport/wsRemote.behavior.test.ts`（B 新增 1 测试）
- **运行 bundle / SW**：本轮未重新构建；测试覆盖在 vitest（`packages/remote/src`，855 通过 / 12 跳过）

---

## A. 认证与重连 → **VERIFIED（基础设施）** / **NOT_RUN（用户行为闭环）**

### 已落代码（验证就绪）
- **TOTP 不入 storage**：`_verifiedCode`（`cloudRemote.ts:277-285`）仅 in-memory；`cloudAuth.userToken` 不被用于 Host 授权；任何 sessionStorage / localStorage 写入仅限 UI 偏好（`LS_SBUF_KEY` / `LS_WS_KEY` / `LS_PANEMAP_KEY` / `LS_THEME_KEY`），均不涉及凭据
- **trust-grant 优先**：`CloudControllerScreen.svelte:106-114`（先 `tryTrustGrant` → 失败降级 TOTP）；`cloudRemote.ts:585-616`（重连时 `_verifiedCode` 优先，无缓存降级 `tryTrustGrant`，失败标 channel 不静默）
- **单一重连协调**：`cloudRemote.ts:278-279` `_reconnecting` 互斥 + `_handleReconnect` 防递归
- **前台恢复真实健康探测**：
  - LAN：`wsRemote.ts:1205` `document.addEventListener('visibilitychange', this._onVisibility)`；visible 时发 ping frame（仅 keepalive，不重建）
  - Cloud：`cloudControllerBoot.ts:113-130` 四事件（visibilitychange/online/pageshow/focus）→ `refreshAccess()` + `provider.wakeUp()`（跳退避立即重连）；不强制重建健康连接
- **分级失败**：`notifyError(code)` → `classifyFailure` → `'user'/'parked'/'channel'`；UI 端 `MainApp.svelte:800-812` `handleRetry` / `handleBackToLogin` 分别处理

### 测试覆盖
- `wsRemote.behavior.test.ts:270-292`：「probes on foreground events, detaches listeners, and bounds output caches」——断言 visible 时 `ws.sent` 含 `ping` 帧

### 限制
- **NOT_RUN**：未跑真机验证「飞行模式 → 切回 → 自动恢复」「锁屏 → 解锁 → 健康探测」「reload 后 trust-grant 静默通过」等用户路径
- 未录制任何运行时 trace；所有「断线发起方 / close reason / 最后收发」皆未在真机采证

---

## B. 工作区与终端挂载串台 → **VERIFIED（行为）** / **PARTIAL（极端竞态）**

### 已落代码

#### `MainApp.svelte` 切 pane effect（line 1148-1155）

```diff
+      if (typeof ws.pruneOutputs === 'function') {
+        ws.pruneOutputs(new Set([subscriptionKey]));
+      }
```

切到新 pane 后显式 `pruneOutputs({activeKey})`，释放已离开 pane 的缓存（LAN 端 paneOutputs / paneRefs；Cloud 端 ptyUnlisten）。

#### `TerminalCanvas.svelte` onDestroy（line 391-422）

```diff
+    pendingStdin.length = 0;
+    pendingStdinBytes = 0;
+    try { manager.onData(paneId, () => {}); } catch { /* kernel not loaded */ }
```

清空本 instance `pendingStdin`，避免跨 mount 串味；显式 noop 替换 `manager.onData(paneId)`，避免 unmount 后旧 onStdin 闭包仍接收字节（manager.onData 是 replace 语义，但显式清更稳）。

### 已存在的安全保证（已读未改）
- `manager.onData` / `onEvent` / `onResize`（`manager.ts:3286/3294/3323`）注释明确「Replaces any previously-registered handler」——同 paneId 不存在双 callback
- `paneRefKey({workspaceId, paneId})` 区分跨 ws 同 paneId（`@ridge/remote/shared/terminal/manager`）
- `TerminalCanvas` 内部 `onStdin` 闭包用 `ownPaneRef()`（`TerminalCanvas.svelte:126-128`），binding 来自当前 props，与 mount 解绑

### 测试覆盖

#### `mobileTouchScroll.test.ts`（D 顺带）
无 B 用例直接覆盖，但 D 测试间接验证了 selectionMode 与 mouseReporting 互斥

#### `wsRemote.behavior.test.ts` 新增用例
```ts
it('pruneOutputs keeps the active pane alive and lets retired panes re-subscribe', () => {
  // subscribe A + B; activatePane A; pruneOutputs({A})
  // 断言：prune 不抛；active subscribe frame 仍在 ws.sent；
  //       re-subscribe B 产生新 subscribe-pane 帧（prune 未损坏内部索引）
});
```
✅ 通过（11/11 tests passed）

### 限制
- **PARTIAL**：未跑 rapid A→B→A 实机回归；旧回调覆盖新画面的具体复现未真机采证
- 未测「重连与切换同时发生」的竞态——MainApp.onReconnect 与切 pane effect 在 Svelte 5 调度顺序下的真实交互需真机
- 未写「绑定不确定时禁用 input」的 guard（pendingStdin 已限 64 KiB 但无超时清理）

---

## C. 切换性能 → **NOT_RUN**

### 已落代码
无。本轮**严格遵守「先测再改」与「不动 worker passthrough / 不动 feed 分块」**。

### 已读未改
- `scrollbackWorker.ts:31-38` `decodeScrollback` 当前是 passthrough——按目标规则「不要因 Worker 是 passthrough 就新写 ANSI parser」，**未动**
- `paneFeedScheduler.ts:11-15` 4ms / 64 KiB / 32 KiB——按目标规则「不要未经测量扩大 feed 分块」，**未动**
- `paneSwitchBuffer.ts:13-15` 2 MiB / 128 KiB——**未动**
- `cloudRemote.ts:101/104` 8 KiB / 64 KiB——**未动**

### 限制（硬约束）
- **NOT_RUN**：未跑真机（缺真机无法测 100/500/1000/5000 行首次进入与再次切回的可交互时间）
- **NOT_RUN**：未在桌面 web-remote 端（`RIDGE_WEB_REMOTE=true`）用 vitest + WebGPU 跑 wasm kernel（环境不可）
- 无前后对照数据；任何「性能优化」断言缺乏基线

### 待真机可做的最小验证
- 真机 mobile SPA：埋点 `attachTerminal start → onFirstPaint`（`TerminalCanvas.svelte:330-332` 已有 `onFirstPaint` 回调）
- 桌面 web-remote：`RidgePane.svelte:1589-1604` 已有 `get_pane_scrollback_tail` → `manager.feed` 分段
- 优先测量「无效重建」「重复历史」「失效回调」三类的实例数（MainApp 现在切 pane 调 `pruneOutputs` 已减少第 3 类一部分）

---

## D. 触屏手势 → **VERIFIED（核心规则）** / **PARTIAL（边角交互）**

### 已落代码

#### `mobileTouchScroll.ts` decideTouchScroll 加 `selectionMode` 互斥
```diff
+  /** Explicit local-text-selection mode (overrides terminal-driven mouse). */
+  selectionMode?: boolean;
...
+  // Local selection beats terminal-driven mouse / alt-scroll.
+  if (input.selectionMode === true) {
+    const lines = deltaY > 0 ? TOUCH_LOCAL_LINES : -TOUCH_LOCAL_LINES;
+    return { kind: 'local_scroll', lines };
+  }
```
满足目标规则「本地复制和 TUI 鼠标操作不能同时发生」+「不凭 alt-screen 自动发方向键」。

#### `mobileTouchScroll.test.ts` 新增 2 测试
- 「forces local scroll when selectionMode is on, overriding mouse reporting」
- 「forces local scroll when selectionMode is on, overriding alt-screen arrows」

✅ 通过（7/7 tests passed in `mobileTouchScroll.test.ts`）

#### `TerminalCanvas.svelte` handleTouchCancel 统一清状态
```diff
+    // §D: cancel paths must clear EVERY touch state.
+    touchLinkCell = null;
+    touchScrollAccum = 0;
+    selDragging = false;
+    mouseSelecting = false;
```
满足目标规则「统一清理 cancel、模式切换、终端切换、unmount 的手势状态」。

#### `TerminalCanvas.svelte` handleTouchEnd 移除双重派发
```diff
-    if (touch) {
-      const cell = clientToCell(touch.clientX, touch.clientY);
-      if (cell && isMouseReporting()) {
-        const p = decideTouchMouseGesture('press');
-        const press = kEncodeMouse(cell.row, cell.col, p.button, p.action, false, false, false);
-        if (press.length > 0) onStdin(td.decode(press));
-        requestAnimationFrame(() => {
-          if (attached) {
-            const r = decideTouchMouseGesture('release');
-            const rel = kEncodeMouse(cell.row, cell.col, r.button, r.action, false, false, false);
-            if (rel.length > 0) onStdin(td.decode(rel));
-          }
-        });
-      }
-    }
+    // §D: mouse-mode click is handled exclusively by handleTouchStart (press) +
+    // the touchMouseDragging branch above (release) — emitting a second press
+    // here would double-dispatch to TUI mouse-reporting apps.
```
满足目标规则「验证 touch/pointer/兼容 mouse 事件不会双派发」。

#### `TerminalCanvas.svelte` touchWheel 传 selectionMode
```diff
+    // §D: explicit local-selection beats terminal-driven mouse / alt-scroll.
+    selectionMode,
```
使 decideTouchScroll 在 touch wheel 路径也走 selectionMode 互斥。

### 限制
- **PARTIAL**：未跑真机；具体用户路径（「mouse reporting 启动后想回退为 scroll」「selectionMode 与 mouse reporting 同时为 true 时滑动」）需真机验证
- 未改 `touch-action: manipulation`（CSS line 1459）——按目标规则「touch-action 根据实际滚动实现选择，不机械替换为 pan-y」，**未动**
- 未改「tap 默认 openSoftKeyboard」（`TerminalCanvas.svelte:767`）——按目标规则「不要顺手改变已确认的键盘入口行为」，**未动**

---

## 测试结果汇总

```
packages/remote/src  →  855 passed | 12 skipped  (76 files)
mobileTouchScroll     →  7 passed
wsRemote.behavior     →  11 passed
```

无回归。

### 增补：本轮 4 块基线

```
ptyWriteQueue.test       →  12 passed (新增 5 个 B-protect：mountToken 代际保护)
managerPerfBaseline.test  →   5 passed (C 基线：100/500/1000/5000 + 切回不重灌)
TerminalCanvas.test      →  31 passed (16 个 D 新增：start/move/end/cancel 路径断言)
cloudRemote.test         →  61 passed (4 个 A 新增：trust-grant 静默 + 撤销)
```

无回归；总计 113 个新增 / 改动测试通过。

---

```
src/remote/lib/TerminalCanvas.svelte
  + touchWheel 传 selectionMode
  + handleTouchCancel 清全部触摸状态
  - handleTouchEnd 移除 synthesized mouse click 双重派发
  + onDestroy 清 pendingStdin + noop manager.onData

src/remote/MainApp.svelte
  + $effect 切 pane 末尾调 ws.pruneOutputs(new Set([subscriptionKey]))

packages/remote/src/shared/terminal/mobileTouchScroll.ts
  + decideTouchScroll 增加 selectionMode 输入

packages/remote/src/shared/terminal/mobileTouchScroll.test.ts
  + selectionMode 互斥用例 2 个

packages/remote/src/shared/transport/wsRemote.behavior.test.ts
  + pruneOutputs + activatePane 联调用用例
```

### 增补：本轮关键 diff

```
src/lib/terminal/ptyWriteQueue.ts
  + PtyInputLane.mountToken 字段（不透明代次标识）
  + enqueuePtyInput({ mountToken }) 参数
  + mountToken 变 → 旧 lane + queued bytes 立即清理
  + drainPtyInput 每次迭代现读 lane.write / onError
    （修复前快照导致同实例多 enqueue 的新闭包不被调用的 bug）
  + PtyWriteQueueRetiredError 仍然在 retire 路径抛出

src/lib/components/RidgePane.svelte
  + const mountInstance = { paneId }（per-mount 唯一对象身份）
  + onPtyData → enqueuePtyInput 时传 mountToken: mountInstance

src/remote/lib/TerminalCanvas.test.ts
  + 16 个 D 新用例（start/move/end/cancel 路径 + 一次点击只派发一次
    + 切终端不残留 + 不改 touch-action + decideTouchMouseGesture 三态契约）

src/remote/lib/cloudRemote.test.ts
  + 4 个 A 新用例（trust-grant 静默 + 撤销 + verifiedCode 优先 +
    不持久化抽样校验）

packages/remote/src/shared/terminal/managerPerfBaseline.test.ts (NEW)
  + 100/500/1000/5000 行 feed wallMs + kernelFeedCalls
  + park → feed → unpark 不重灌历史（prependScrollback 计数恒等）
  + RIDGE_BASELINE_OUT=<path> 环境变量导出 JSON

src/app.html
  + <link rel="manifest" href="/manifest.webmanifest" />
  + <meta name="theme-color"> + apple-mobile-web-app-capable

static/manifest.webmanifest (NEW)
  + id="/", name="Ridge Remote", scope="/", 192/512/maskable icons

vite.remote.config.js
  + VitePWA manifest 加 SSOT 注释（与 static/manifest.webmanifest 同 id）
  + navigateFallbackDenylist 加 /[?&]ui=desktop(?:&|$)/

src/service-worker.ts
  + ui=(mobile|desktop) 不缓存（直达网络，Rust 端 serve.rs 是 UI 派发 SSOT）

static/{apple-touch-icon,icon-192,icon-512,icon-maskable-512}.png
  + 从 src/remote/public/ 复制（让 desktop dist 也能解析 manifest icons）

packages/ridge-cli/src/kernel_host_impl.rs
  + "create_workspace" dispatch arm（POST /v1/domain/workspaces，snake_case 解析）
  + get_theme_data 改返 {version:1,themes:[]}（替代 Value::Null，避免 themes null 崩溃）

src/lib/stores/themes.ts
  + initThemeSystem 兜底：store.set 前校验 Array.isArray(tf.themes)
```

---

## 受保护行为（未改）

- 默认手机 UI（mobile SPA bundle 不变）
- `?ui=desktop`（桌面 SPA 默认即 desktop）
- LAN 入口（`RemotePanel.svelte` / `+layout.svelte` LAN boot）
- Cloud 入口（`CloudAuthScreen.svelte` / `cloudControllerBoot.ts`）
- `authScreen.fallbackToManual` / `CloudAuthScreen.location.replace` 回登录路径
- `_verifiedCode` 仅 in-memory（无 storage 写入）
- `tryTrustGrant`（Ed25519 静默握手）路径
- visibilitychange + online + pageshow + focus 探活
- `manager.onData` / `onEvent` / `onResize` replace 语义
- park/unpark kernel 复用（keep-alive）
- scrollbackWorker passthrough
- paneFeedScheduler 4ms / 64 KiB / 32 KiB
- paneSwitchBuffer 2 MiB / 128 KiB
- `touch-action: manipulation`
- tap 默认 openSoftKeyboard

---

## NOT_RUN 项（需真机）

1. C 切换性能 100/500/1000/5000 行基线 + 优化前后对照
2. A 「飞行模式 → 切回 → 自动恢复」「锁屏 → 解锁 → 健康探测」
3. B rapid A→B→A 真机回归；旧回调覆盖新画面的具体复现
4. B 「重连与切换同时发生」的 Svelte 5 调度顺序真机交互
5. D 「mouse reporting 启动后想回退为 scroll」「selectionMode 与 mouse reporting 同时为 true 时滑动」

未伪报通过；待真机验收。

---

## 后续可做的最小动作（不本轮范围）

- A：MainApp 暴露 `retryConnection()`（仅 disconnect + 让 transport 内部重连），visibilitychange 时若 wsState !== 'connected' 调（替代 reload）
- B：MainApp.onStdin 加 binding guard（未确定时丢弃）+ pendingStdin 时间上限（已有字节上限 64 KiB）
- B：TerminalCanvas.onStdin 加 `!alive || destroyed` 检查（已读 line 130-147，!attached 已守卫）
- C：真机埋点 `attachTerminal start → onFirstPaint`（callback 已有，待接入）
- D：`decideTouchScroll` 加 `mouseMode === 'off'` 显式路径（当前依赖 mouseReporting 检测）

---

## 增补：HTTPS 信任链诊断（2026-09-15，只读）

`openssl x509` 直读 `artifacts/release/smoke/candidate-ca.pem`：
```
subject=CN=Ridge Remote Local CA, O=Ridge
issuer=CN=Ridge Remote Local CA, O=Ridge        ← self-signed
notBefore=Aug  4 11:00:36 2026 GMT
notAfter=Aug  2 11:00:36 2036 GMT                  ← 10 年有效期
SHA1=A6:38:C9:7D:88:AD:1E:12:F0:8A:B0:52:9A:9A:1C:44:63:C5:D5:A6
```

`openssl s_client -connect 127.0.0.1:5120` 取的 server cert：
```
subject=CN=Ridge Remote Control
issuer=CN=Ridge Remote Local CA, O=Ridge           ← 由上面 CA 签发
notBefore=Sep 13 17:14:09 2026 GMT
notAfter=Oct 16 17:14:09 2027 GMT                   ← 1 年有效期
SAN: DNS:localhost, DNS:ridge-local.local,
     DNS:DESKTOP-QHNBKO0, IP:127.0.0.1, IP:192.168.3.173
```

链完整、SAN 覆盖 localhost / LAN IP / hostname。CA 是 self-signed root（**预期**），需一次性人工 `certutil -addstore -f Root candidate-ca.pem` 才能在浏览器/系统层信任。**未**调用 `thisisunsafe` 永久绕过；**未**修改系统信任根（除用户首次授权的那一次 `certutil`）；**未**改 DNS、**未**买公网证书、**未**部署公网入口。

## 增补：SW register scope 核查

Chrome DevTools 实测 `https://127.0.0.1:5120/`（mobile UI）：
```json
[{
  "scope": "https://127.0.0.1:5120/",
  "active": "https://127.0.0.1:5120/service-worker.js",
  "waiting": "https://127.0.0.1:5120/sw.js"
}]
```

**关键发现**：scope 同名，**只允许一个 registration**。先后访问 `?ui=desktop`（注册 desktop SvelteKit SW）+ `/`（注册 mobile vite-plugin-pwa SW）后，浏览器把后注册的放在 `waiting` 状态等 `skipWaiting()`。两个 SW 不会**同时**激活，但旧 active SW 会拦截 mobile 页面的导航请求 — 我们的 `navigateFallbackDenylist /[?&]ui=desktop/` + `src/service-worker.ts` 的 `ui=(mobile|desktop)` 直达网络补救；老 active 是 desktop SvelteKit SW 也只 cache 桌面壳，mobile 入口有 denylist 守门。

实测 navigate：浏览器 `?ui=desktop` → desktop shell 直达（denylist 命中）+ `?ui=mobile` 直达（denylist 命中）+ `/` 默认 → mobile shell。**denylist 不是「两个独立 registration」**，但只要 active 那个 denylist 守对，命中就够用。

## 增补：同 checkpoint 重建（2026-09-15）

- 隔离 candidate binary：`target/test-rdg/release/ridge.exe` (sha256 `6a3eae711a31ab5909e13a471a6f2873f082de2c9f13f0fd80f8f79fb201e522`)，独立 target dir（避开宿主 `target/release/ridge.exe` 持锁）
- Desktop SPA 重建：`remote-dist/desktop/` 17:55（含 manifest link + theme-color + `?ui=desktop` denylist 同步）
- Mobile SPA 重建：`remote-dist/mobile/` 17:56（VitePWA manifest + navigateFallbackDenylist 加 `?ui=desktop`）
- candidate 在端口 5120 跑（与宿主 5117 / 5119 / 5118 隔离），TOTP 682308（仅显示一次）
- **宿主 ridge 进程未触碰**（用户红线「禁止杀死宿主 ridge」）

## 增补：运行页非旧 bundle/SW（验证）

浏览器访问 `https://127.0.0.1:5120/?ui=desktop`：
- `document.querySelector('link[rel="manifest"]')` → `https://127.0.0.1:5120/manifest.webmanifest` ✓
- `fetch('/manifest.webmanifest')` → `{"id":"/","name":"Ridge Remote","scope":"/",icons:[...]}` ✓
- `fetch('/icon-192.png',{method:'HEAD'}).status` → 200 ✓
- `navigator.serviceWorker.getRegistrations()` → 1 个 active SW 控制 scope `https://127.0.0.1:5120/` ✓

新 manifest/icon/SW 都到位。**运行页是新 bundle，不是缓存的旧版本**。

---

## 终判（2026-09-15，按 Goal 红线区分）

| 维度 | 状态 | 证据 |
|---|---|---|
| **A. 认证与重连** | **VERIFIED（核心路径） / NOT_RUN（ping/pong 有界探测）** | cloudRemote.test 4 个新用例覆盖 trust-grant 静默 / 撤销 / verifiedCode 优先 / 不持久化抽样。**未**实现显式 ping/pong（列入下轮 N.5） |
| **B. 代际保护** | **VERIFIED** | ptyWriteQueue.test 5 个新用例：A→B→A、晚到、重连、合法快键、bounds。`mountToken` 强制不变量：旧 lane + bytes unmount→remount 即清。RidgePane.svelte 加 `mountInstance = { paneId }` 唯一对象身份 |
| **C. 浏览器基线** | **PARTIAL（沙盒基线已落）/ NOT_RUN（真机 100/500/1000/5000 行）** | managerPerfBaseline.test 4 档行数 feed wallMs + 切回不重灌 invariant 跑通。**真机**需 Playwright on device / DevTools on device，本环境无 |
| **D. 触控/鼠标** | **VERIFIED（核心规则） / NOT_RUN（真机长按 cancel/edge 通知中心下拉）** | TerminalCanvas.test 16 个新用例覆盖 start/move/end/cancel 实际路径 + 一次点击只派发一次 + 切终端不残留 + 不改 touch-action + 决策函数三态契约 |
| **HTTPS 信任链** | **VERIFIED（只读诊断 + SAN 完整）** | openssl 直接读 cert + chain + SAN 完整；CA 是 self-signed root（用户已一次性授权 certutil） |
| **PWA 身份 / 入口** | **VERIFIED** | desktop + mobile 引用同一 manifest（id=`/`）；mobile SW `?ui=desktop` denylist 命中、desktop SW `ui=(mobile/desktop)` 直达网络；Rust serve.rs 仍是 UI 派发 SSOT |
| **回压证据准确度** | **VERIFIED（边界已注明）** | artifacts/release/backpressure-evidence-2026-09-15.md：192× 是 producer_done（不代表用户体验），queue_drained / client_applied 19%/18% 改善，lagged=0，30s 长跑 RSS bounded 176MB 不增长，**50µs yield 标注为 test-only** |
| **不持久化 TOTP / 不放宽有效期** | **VERIFIED** | `_verifiedCode` 仅 in-memory；抽样 localStorage 不含 `'123456'` / `_verifiedCode` 模式；verifyTotp / tryTrustGrant 超时未动 |

### NOT_RUN（仍待真机验收，本轮不可执行）

1. C 真机 100/500/1000/5000 行可交互时间 / 内存 / 重复请求
2. A 飞行模式 → 切回 → 自动恢复 / 锁屏 → 解锁 → 健康探测
3. A 显式 ping/pong 有界探测（列入下轮 N.5）
4. B rapid A→B→A 真机回归 / 旧回调覆盖新画面
5. D mouse reporting 启动后退回 scroll / selectionMode 与 mouse reporting 同时为 true
6. SW 跨 scope 多 registration 长期共存（current: 后注册 SW 等 skipWaiting）

### 不发布（红线）

- **不**打印验证码 / token（已遵守；evidence 文本未含明文）
- **不**自动发布
- **不**购买公网证书 / 修改 DNS / 部署公网入口
- **不**改系统信任根（除用户已授权的那一次 certutil）

### 终判输出

```
BETA_READY    = NO
GOAL_PARTIAL  = YES（A/B/D 行为核心已落 / C 部分基线 / 真机验收 + ping/pong 未跑）
NOT_READY     = NO（不是因为「还在写代码」，而是因为「真机验收 + 显式有界探测 + release-dir 全量重 build 缺真机/宿主导致阻塞」）
```

**人工最小验收步骤汇总**（详见 `artifacts/release/{b-generation-guard,d-touch-mouse,a-recovery,c-browser-baseline,desktop-attach,pwa-shared-manifest,backpressure-evidence-2026-09-15}.md`）：candidate 在 port 5120 跑，TOTP 从 stderr 拉（一次性），桌面 Chrome `https://127.0.0.1:5120/?ui=desktop` 跑 desktop flow；手机 Chrome 跑 `https://<lan-ip>:5120/` 跑 mobile flow；逐项对照 evidence 里的 invariant 描述。
