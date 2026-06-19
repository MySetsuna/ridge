# Ridge — 下次发布待办（next release backlog）

## 0.0.8（计划）

### [ ] mobile-app 前台 SW 更新提示（foreground update prompt）
**背景**：当前 SW 自动更新策略——
- desktop-app（SvelteKit SW）：`skipWaiting`+`clientsClaim`+版本门控，检测到新版自动 `location.reload()` 切换（较激进，OK）。
- mobile-app（vite-plugin-pwa，`registerType:'prompt'`）：检测到新版后**仅在标签切后台时**静默 skipWaiting+reload（避免打字中途刷新），见 `src/remote/main.ts` 的 `flushUpdateWhenHidden`。

**问题**：mobile 端若用户一直前台开着、不切后台，就拿不到新版（需手动刷新/清缓存）。
**要做**：在 mobile-app 前台加一个轻量「发现新版 → 点此刷新」提示条：`onNeedRefresh` 时若 `visibilityState==='visible'`，显示可点击的 toast/banner；用户点击即 `applyUpdate(true)`（skip-waiting+reload）。既不中途打断输入、也不必清缓存。
**改动点**：`src/remote/main.ts`（onNeedRefresh 分支）+ 一个轻量 UI 组件/banner（复用 MainApp 顶部条样式）。仅 mobile（vite.remote.config）路径;desktop SW 已自动 reload 无需改。
**验收**：mobile 前台开着时发版 → 出现「新版可用」提示 → 点击后 reload 到新版、登录态经 cookie 保持。
