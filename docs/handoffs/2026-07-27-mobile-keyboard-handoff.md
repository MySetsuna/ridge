# 换机交接：手机端软键盘遮挡/错位（0.1.5 已发，但**未修好**）

用户实测：公网 Remote 在手机上唤起软键盘后，终端可见区仍不对。0.1.5 的改法**没有解决**，
换机继续。本文只写「已知事实 + 落点 + 怎么复现」，不预设结论。

## 一、现状：代码/产物都在哪

- 分支 `main`，全部已推。相关提交（`git log --oneline v0.1.4..HEAD`）：
  - `15902b9 fix(remote): 手机唤起键盘后终端可见区错位/被压到底部——改为按视觉视口收高` ← **本问题的改动**
  - 其余为多 tab 卡死、agent 面板、remote 资产/并发 invoke、手机端切换终端类型。
- **桌面安装包**：GitHub Release `v0.1.5`，三平台资产齐（Windows setup/msi/rdg、
  Linux deb/AppImage/rdg、macOS dmg×2/app.tar.gz×2/rdg）。
- **公网手机 SPA**：走**另一条**发布线 `publish-remote.yml`（手动触发，token 在仓库 secret）。
  已发布并激活：`{"version":"0.1.5+g79688f2","activatedAt":"2026-07-27T01:32:57Z"}`。
  也就是说，公网手机上拿到的确实是含该改动的包 —— 问题不是「没发上去」。
- **局域网手机 SPA** 内嵌在桌面端/rdg 里，要验局域网必须装 0.1.5 或本地 `pnpm build:remote`。

PWA 是 `registerType: 'prompt'`（`vite.remote.config.js`），新版**只在页面切到后台时**
静默换入（`src/remote/main.ts` 的 `flushUpdateWhenHidden`）。验证前务必把手机上的
Remote 切后台再回来，或彻底关掉重开，否则量的是旧包。

## 二、改了什么（`15902b9`）

落点两个文件：

- `src/remote/lib/keyboardOffset.ts`
  —— 旧导出 `keyboardShiftPx`（算 translateY 位移量）**已删**，改为
  `terminalViewportHeightPx({layoutHeightPx, visualHeightPx, visualOffsetTopPx,
  containerTopPx, chromeBelowPx, minHeightPx})`：返回键盘弹出时终端宿主该有的高度，
  键盘收起返回 `null`。纯几何、无 DOM，单测在 `keyboardOffset.test.ts`（7 条，绿）。
- `src/remote/lib/TerminalCanvas.svelte`
  —— `keyboardOffset`（transform）整条路径删掉，改为内联
  `style="height:{keyboardHeight}px;flex:0 0 auto"`；`.container` 的过渡由
  `transform .2s` 改 `height .18s`；`measureGapBelowCanvas()` 现在同时测
  `containerTopWhenIdle` 与 `gapBelowCanvas`（**只在键盘收起时测**）。

改动的理由（当时的判断，**未被真机证实**）：手机尤其 iOS 弹键盘只缩**视觉**视口
（`visualViewport.height`），布局视口 `innerHeight` 不变，所以 `flex:1` 的容器下半截
天然被键盘压住；旧实现只把光标那一行顶上去，其余内容仍在键盘后面。

## 三、为什么没被验出来

**这条改动从未在真机软键盘上跑过。** 已有的 e2e
（`scripts/cdp-remote-mobile-agents.mjs`）跑在 WebView2 里，**没有软键盘**，
`visualViewport` 不会因键盘而变；它验的是花名册渲染、逐成员发消息、切换终端类型，
碰不到这段代码。所以「单测绿 + 云端已激活」并不构成这个问题已修的证据。

下一台机器的第一件事应当是**先建立能观测该现象的手段**，再改代码。

## 四、怎么复现 / 观测

1. 手机（真机）打开公网 Remote，切后台再回来确保吃到新包。
2. 需要看现场数值时，用同一手机的浏览器远程调试（Android Chrome：`chrome://inspect`；
   iOS Safari：Mac 上「开发」菜单）。挂上后在控制台读：

   ```js
   const vv = window.visualViewport;
   const c = document.querySelector('.container');
   ({
     innerHeight: window.innerHeight,
     vvHeight: vv.height, vvOffsetTop: vv.offsetTop,
     containerRect: c.getBoundingClientRect().toJSON(),
     containerStyleHeight: c.style.height,
   })
   ```

   键盘收起、弹出各取一次。判据：弹出后 `containerRect.bottom` 应 `<= vv.offsetTop + vv.height`
   （容器整体在可见区内），且 `containerRect.top` 不应被推到负值。

3. 请用户明确现象二选一 —— **两者修法不同，别猜**：
   - 「内容被压扁 / 变形」→ 高度变了但画布没跟着重排（看 `manager.viewportChanged` 是否
     真的触发了 refit，以及 WebGPU host 模式下 scissor 是否按新 bbox 重算）。
   - 「底部被挡住 / 看不见输入行」→ 高度没收够，查 `chromeBelowPx` / `containerTopWhenIdle`
     是不是在键盘已经弹起时被测的（`measureGapBelowCanvas` 只在 `keyboardHeight === null`
     时才测；若组件在键盘已弹起时挂载，这两个常量就是错的）。

## 五、已知可疑点（未验证，供起步）

- `measureGapBelowCanvas()` 的守卫是 `keyboardHeight !== null` 就跳过。若用户在键盘弹起
  状态下切换 pane / 组件重挂，`containerTopWhenIdle` 与 `gapBelowCanvas` 会带着错值进公式。
- iOS 上聚焦输入框会把整页**上推**（`visualViewport.offsetTop > 0`）。公式已按
  `offsetTop + height` 算可见区底边，但 `containerTopPx` 用的是**键盘收起时**测的值；
  页面被上推后容器真实 top 变了，两者不同步。
- `.container` 有 `.18s` 高度过渡，`manager.viewportChanged` 在 0ms 与 240ms 各调一次；
  若真机键盘动画更慢，中间态可能留下错的网格尺寸。
- 手机端顶栏/底栏（`BottomTabBar`）本身在键盘弹起时是否也改变高度 —— 若改变，
  `chromeBelowPx` 当常量的前提就不成立。

## 六、换机后立刻能跑的东西

```bash
pnpm install
pnpm build:remote                 # 手机 SPA → static/remote
npx vitest run src/remote/lib/keyboardOffset.test.ts
pnpm tauri:dev:cdp                # 起带 CDP 的桌面端（端口见 DevToolsActivePort）
CDP_PORT=<port> node scripts/cdp-remote-mobile-agents.mjs   # 手机端全链路门禁（不含键盘）
```

真机验完、确认修好之后，**两条线都要发**：

- `git tag -a vX.Y.Z && git push origin vX.Y.Z` → 等 `release.yml` 五个 job 全绿、
  资产齐了再 `gh release edit --draft=false`（CI 默认出 draft）。
- `gh workflow run publish-remote.yml` → 公网手机 SPA 才会更新（**这条最容易漏**）。
