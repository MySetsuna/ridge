# 共用 PWA 入口 + 启动 UI 路由（#24）

**Date**: 2026-09-15
**Goal**: 两种 Web UI 引用同一 manifest + 应用身份；正常 PWA 启动默认手机 UI；仅显式 `?ui=desktop` 进入桌面版；SW 不会固定返回错误 UI 壳；不缓存认证响应；不离线补发终端输入；不用强制更新中断操作。

## 改动

### 1. 共用 manifest SSOT：`static/manifest.webmanifest`

新文件，作为 desktop + mobile 两份构建的 PWA 身份 SSOT：

```json
{
  "id": "/",
  "name": "Ridge Remote",
  "short_name": "Ridge",
  "start_url": "/",
  "scope": "/",
  "display": "standalone",
  "theme_color": "#0d1117",
  "background_color": "#0d1117",
  "icons": [
    { "src": "/icon-192.png", "sizes": "192x192", "type": "image/png" },
    { "src": "/icon-512.png", "sizes": "512x512", "type": "image/png" },
    { "src": "/icon-maskable-512.png", "sizes": "512x512", "type": "image/png", "purpose": "maskable" }
  ]
}
```

同 id/name/scope/icons → 浏览器把 desktop 与 mobile 入口识别为同一 app，重装不产生重复。

### 2. desktop SvelteKit 入口加 manifest link：`src/app.html`

```html
<link rel="manifest" href="/manifest.webmanifest" />
<meta name="theme-color" content="#0d1117" />
<meta name="apple-mobile-web-app-capable" content="yes" />
<meta name="apple-mobile-web-app-title" content="Ridge" />
<link rel="apple-touch-icon" href="%sveltekit.assets%/apple-touch-icon.png" />
```

> 同步复制 `static/` 下缺失的 PWA 图标（apple-touch-icon / icon-192/512/maskable-512）以保证 desktop 也能解析 manifest 中所有 icon 路径。

### 3. mobile VitePWA 配置加 SSOT 注释：`vite.remote.config.js`

```js
// SSOT 共用 PWA 身份：本配置必须与 `static/manifest.webmanifest`（被
// desktop SvelteKit 复制到 dist）保持同 id / icons，否则浏览器把 ?ui=desktop
// 入口识别成第二个 PWA — 重复安装、错图标。修改两边任一处都请同步另一边。
```

### 4. mobile SW 排除 `?ui=desktop` 命中：`vite.remote.config.js`

```js
navigateFallbackDenylist: [
  /^\/ws/, /^\/info/, /^\/verify/, /^\/health/, /^\/status/,
  /^\/session/, /^\/workspace/, /^\/ridge-ca/, /^\/assets\//,
  // ?ui=desktop 必须直达网络，否则离线下 SW 命中 mobile 缓存 → 用户
  // 进桌面失败却看见 mobile 壳（看起来「能进」实际错壳，更难诊断）。
  /[?&]ui=desktop(?:&|$)/,
],
```

> 旧设计：mobile SW 把所有 navigation fallback 到 `index.html`。如果用户安装 mobile PWA，离线访问 `?ui=desktop` 会被 mobile SW 拦截 → 返回 mobile `index.html` 壳（user 看着「能进」实际是 mobile UI，且 TOTP 鉴权逻辑根本不会运行 → "缓存固定返回错误 UI 壳" bug，Goal #2）。

### 5. desktop SW 排除对侧 UI 入口命中：`src/service-worker.ts`

```js
if (/(?:^|[?&])ui=(?:mobile|desktop)(?:&|$)/.test(url.search)) return;
```

> 桌面 SW 同样不能把 `?ui=mobile` 当成普通 `?` 变体回吐 desktop 缓存。Rust `serve.rs` 是 UI 派发 SSOT，desktop SW 必须放行显式 UI 切换的导航请求到网络。

## 验证

### dist 内容（重建后）

```
remote-dist/desktop/manifest.webmanifest       (550 B, 2026-09-15 17:55, 来自 static/ SSOT)
remote-dist/desktop/icon-192.png               (3315 B)
remote-dist/desktop/icon-512.png               (13601 B)
remote-dist/desktop/icon-maskable-512.png      (12596 B)
remote-dist/desktop/apple-touch-icon.png       (3068 B)
remote-dist/desktop/index.html                 含 <link rel="manifest" href="/manifest.webmanifest" />
                                              + <meta name="theme-color" content="#0d1117" />
                                              + <meta name="apple-mobile-web-app-capable" content="yes" />

remote-dist/mobile/sw.js                       (17397 B, 2026-09-15 17:56, 含 /[?&]ui=desktop(?:&|$)/ 在 navigateFallbackDenylist)
remote-dist/mobile/manifest.webmanifest        (1 行 minified，与 desktop 字段同 id/name/icons)
```

### 浏览器端实测（candidate `https://127.0.0.1:5120/?ui=desktop`）

```js
document.querySelector('link[rel="manifest"]').href
  → "https://127.0.0.1:5120/manifest.webmanifest"  ✓

document.querySelector('meta[name="theme-color"]').content
  → "#0d1117"  ✓

fetch('/manifest.webmanifest').then(r => r.json())
  → { id: "/", name: "Ridge Remote", scope: "/", icons: [...], ... }  ✓

fetch('/icon-192.png', {method: 'HEAD'}).status
  → 200  ✓

navigator.serviceWorker.getRegistrations()
  → 1 个 registration, scope: "https://127.0.0.1:5120/", script: "/service-worker.js"  ✓
```

### 不同入口的 UI 派发（Rust serve.rs SSOT）

| 入口 | 返回 SPA | 备注 |
|---|---|---|
| `https://host/` (任意 UA) | mobile (`<html lang="zh-CN">` + mobile `<title>`) | 默认手机 UI |
| `https://host/?ui=desktop` | desktop (`<html lang="en" data-rg-theme="dark">` + desktop CSP) | 显式桌面 |
| `https://host/?ui=mobile` | mobile | 显式手机 |

### SW 派发（兜底）

| 请求 | mobile SW | desktop SW | 行为 |
|---|---|---|---|
| `GET /` | 命中 precache → mobile shell | 命中 HTML cache → desktop shell | 默认各自 UI |
| `GET /?ui=desktop` | 命中 denylist → 直达网络 → Rust 返 desktop | 命中 denylist → 直达网络 → Rust 返 desktop | 始终桌面 |
| `GET /?ui=mobile` | 命中 precache → mobile shell | 命中 denylist → 直达网络 → Rust 返 mobile | 始终手机 |
| `POST /verify` | 命中 denylist → 直达网络 | 命中 denylist → 直达网络 | 不缓存认证响应 ✓ |
| `GET /workspace/list` | 命中 denylist → 直达网络 | 命中 denylist → 直达网络 | 不缓存工作区数据 ✓ |
| `WS /ws` | 命中 denylist → 直达网络 | 命中 denylist → 直达网络 | 不缓存 PTY 字节 ✓ |

### 强制更新策略

- `vite-plugin-pwa` `registerType: 'prompt'` + `injectRegister: false`：mobile SW 不自动 reload
- `src/remote/main.ts` 手动驱动更新：`onNeedRefresh` 在 tab 切到后台时调用 `skipWaiting()` → 不会打断前台操作
- `src/service-worker.ts` (desktop) `version-gate`：`install`/`activate` 检查 `ridge-version-${version}` marker，不匹配则 `caches.delete` 旧缓存

## 已知遗留

- mobile + desktop 的 SW 都注册在 scope `/`，二者**只能存活一个**（后注册的覆盖前者）。
  - 用户流程：装 PWA → 走 mobile PWA → 桌面 SW 未注册 → mobile SW serve `?ui=desktop` 时被 denylist 命中 → Rust 返 desktop shell → 桌面 SPA 启动后调用 `navigator.serviceWorker.register('/service-worker.js')` → **替换** mobile SW（scope 相同）
  - 这是设计选择：「PWA 启动默认 mobile UI」优先；`?ui=desktop` 入口切换后，desktop SW 接管 cache
  - 若需「mobile PWA 永远存活」: 应在 desktop SPA 启动时检查 `?ui=desktop` 是否带 `__pwa_only=1`，否则不注册 desktop SW — 与当前「共享身份」目标正交，列入下轮

## 产物

- `static/manifest.webmanifest` 新增
- `static/{apple-touch-icon,icon-192,icon-512,icon-maskable-512}.png` 从 `src/remote/public/` 复制
- `src/app.html` 加 manifest link + theme-color + apple-capable
- `vite.remote.config.js` VitePWA 配置加 SSOT 注释 + `?ui=desktop` denylist
- `src/service-worker.ts` 加对侧 UI 入口 denylist
- `remote-dist/desktop/` + `remote-dist/mobile/` 已重建（17:55 / 17:56）
- 当前 candidate（port 5120，test-rdg 二进制 6a3eae71）已加载新 dist
