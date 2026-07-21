# 桌面 web-remote：service worker 提前注册（与移动端对齐）

- 日期：2026-06-25
- 范围：仅 `src/routes/+layout.svelte`（SvelteKit 桌面 / web-remote app；**非** `src/remote`）

## 背景

桌面 web-remote 的 service worker 是 `src/service-worker.ts` → 经 SvelteKit 产出
`/service-worker.js`。它负责：

- 预缓存 `_app/immutable` 静态壳（含 Monaco）；
- version-gate：版本变了即 nuke 旧缓存 = 用户要的「发布后自更新」；
- BYPASS 列表排除控制面路径（`/ws /verify /session /workspace /info /health /status /file`）。

由于 `svelte.config.js` 设了 `kit.serviceWorker.register = false`（Tauri 不要 SW），
注册改由 `+layout.svelte` 手动调 `registerServiceWorker()`（约 52 行，调
`navigator.serviceWorker.register('/service-worker.js')`）完成。

## 问题

改动前，`registerServiceWorker()` 只在鉴权 / 接线成功（`ready = true`）之后的 4 处被调用：

- 缓存码 TOTP 验证通过；
- 信任授权（trust grant）通过；
- LAN-WS `finish()`；
- 手输 TOTP 提交通过。

后果：未连上 / host 离线 / 卡登录时 `ready` 永不为 true → SW 永不注册 → 静态壳不被缓存、
也拿不到 version-gate 自更新。移动端 `main.ts` 是 `immediate: true` 一进页面即注册，行为不一致。

## 改动

1. **提前注册（与鉴权解耦）**：在 `onMount` 的 WEB_REMOTE 路径起点
   （`startCloudControllerBootMode()` 之前）调用一次 `registerServiceWorker()`。
   这样静态壳无论是否连上 host 都能被缓存并参与 version-gate 自更新，与移动端一致。

2. **只在 web-remote 路径注册**：`!WEB_REMOTE`（真实 Tauri）分支在 `onMount` 内早 return，
   提前注册放在其后的 WEB_REMOTE 路径内，确保 Tauri runtime 永不安装 SW。

3. **删除 4 处冗余 post-auth 调用**：因为注册幂等（提前注册已覆盖所有路径），这 4 处
   `ready = true; registerServiceWorker();` 中的注册调用已无意义。**删去注册调用、保留
   `ready = true`**，不改变各分支其它行为。

### 对 4 处冗余调用的取舍

选择**删除**而非保留。理由：

- 注册本身幂等，提前注册后这 4 处不再产生任何额外效果，纯属噪音；
- 删后代码更清晰——「SW 注册」只在一个固定时机发生，读代码时不会误以为它与鉴权成功耦合；
- 每处删的只是 `registerServiceWorker()` 一行，`ready = true` 及周边逻辑原样保留，
  各分支可观察行为零变化，diff 最小且无害。

保守的「保留」做法也无害（幂等），但会留下误导性的耦合假象，故不采用。

## 测试取舍

`onMount` 的注册时机发生在 Svelte 组件生命周期内，难以在不引入脆弱 DOM/生命周期 mock
的情况下做有价值的单测；本次仅「挪动 + 删除一个幂等调用」，不值得硬造脆测。改动正确性
由 `svelte-check` 类型检查 + 既有行为不变性（4 处仅删注册、保留 `ready`）保证。

## 不在范围

未改动 `src/service-worker.ts`、`svelte.config.js` 及任何其它文件。
