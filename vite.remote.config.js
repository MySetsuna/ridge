import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { VitePWA } from 'vite-plugin-pwa';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// §cloud-remote: the mobile app is ALWAYS browser-served (never native Tauri),
// so — exactly like the desktop web-remote build (vite.config.js) — every
// `@tauri-apps/api/*` import is redirected to the WS/WebRTC-backed shims in
// src/lib/transport/tauriShim. The LAN path imports no Tauri API, so these
// aliases are inert there; they only resolve once the cloud-controller path
// (cloudControllerBoot → TauriDataProvider/ControllerCloudProvider) is loaded.
/** @param {string} f */
const shim = (f) => path.resolve(__dirname, 'src/lib/transport/tauriShim', f);

export default defineConfig({
  root: path.resolve(__dirname, 'src/remote'),
  base: '/',
  // The mobile bundle is browser-served → RIDGE_WEB_REMOTE is true (drives the
  // browser-vs-native decisions in the shimmed `$lib` modules the cloud path
  // pulls in). RIDGE_CLOUD_BASE_DOMAIN empty → apiClient falls back to the
  // production base (9527127.xyz); the debug packager overrides it.
  define: {
    'import.meta.env.RIDGE_WEB_REMOTE': JSON.stringify(true),
    'import.meta.env.RIDGE_CLOUD_BASE_DOMAIN': JSON.stringify(process.env.RIDGE_CLOUD_BASE_DOMAIN || ''),
    'import.meta.env.RIDGE_CLOUD_DEV_PLAINTEXT': JSON.stringify(process.env.RIDGE_CLOUD_DEV_PLAINTEXT || ''),
  },
  // Isolate the dep-optimize cache from the MAIN dev server. Both Vite roots
  // resolve their default cacheDir to the project-root `node_modules/.vite`
  // (the nearest package.json), so when `set_remote_enabled` spawns this remote
  // dev server in debug mode it would re-optimize and invalidate the main
  // window's cached deps → `504 (Outdated Optimize Dep)` → SvelteKit 500. A
  // dedicated cacheDir keeps the two from clobbering each other.
  cacheDir: path.resolve(__dirname, 'node_modules/.vite-remote'),
  resolve: {
    alias: {
      '@ridge/term-wasm': path.resolve(__dirname, 'packages/ridge-term/pkg'),
      '@ridge/remote': path.resolve(__dirname, 'packages/remote/src'),
      '$lib': path.resolve(__dirname, 'src/lib'),
      // Tauri API → browser shims (cloud-controller path). Mirror of the
      // web-remote alias set in vite.config.js; keep the two in sync.
      '@tauri-apps/api/core': shim('core.ts'),
      '@tauri-apps/api/event': shim('event.ts'),
      '@tauri-apps/api/window': shim('window.ts'),
      '@tauri-apps/plugin-dialog': shim('dialog.ts'),
      '@tauri-apps/plugin-clipboard-manager': shim('clipboard.ts'),
      '@tauri-apps/plugin-opener': shim('opener.ts'),
    },
  },
  plugins: [
    svelte(),
    // PWA: offline-cache the static shell + assets, auto-update on new release.
    // The Rust remote server (src-tauri/src/remote/server.rs) serves the emitted
    // sw.js / manifest.webmanifest / icons via its SPA fallback with the right
    // cache headers (sw.js + manifest = no-cache so updates are detected).
    VitePWA({
      // 'prompt' (not 'autoUpdate'): the generated SW *waits* and fires
      // onNeedRefresh instead of reloading immediately. We drive the update
      // ourselves from main.ts — silently, but timed so it never interrupts an
      // active terminal session (reload happens when the tab is backgrounded).
      registerType: 'prompt',
      injectRegister: false, // registered manually in src/remote/main.ts
      // Icons / favicon (and any other static public asset) need precaching
      // too. Globs cover present + future drops into src/remote/public so a new
      // icon/media file is auto-included without editing this list. The flag
      // fonts are retained for non-terminal UI assets.
      includeAssets: [
        'favicon.png',
        'apple-touch-icon.png',
        'icon-192.png',
        'icon-512.png',
        'icon-maskable-512.png',
        '**/*.{png,jpg,jpeg,gif,svg,webp,ico}',
        '**/*.{woff2,woff,ttf}',
        '**/*.{mp3,mp4,wav,ogg,webm}',
      ],
      manifest: {
        // SSOT 共用 PWA 身份：本配置必须与 `static/manifest.webmanifest`（被
        // desktop SvelteKit 复制到 dist）保持同 id / icons，否则浏览器把 ?ui=desktop
        // 入口识别成第二个 PWA — 重复安装、错图标。修改两边任一处都请同步另一边。
        // 稳定 id 使浏览器把二次安装识别为同一 app（避免 start_url 漂移导致重装）。
        id: '/',
        name: 'Ridge Remote',
        short_name: 'Ridge',
        description: 'Ridge 远程终端控制台',
        lang: 'zh-CN',
        start_url: '/',
        scope: '/',
        display: 'standalone',
        orientation: 'any',
        background_color: '#0d1117',
        theme_color: '#0d1117',
        icons: [
          { src: '/icon-192.png', sizes: '192x192', type: 'image/png' },
          { src: '/icon-512.png', sizes: '512x512', type: 'image/png' },
          { src: '/icon-maskable-512.png', sizes: '512x512', type: 'image/png', purpose: 'maskable' },
        ],
      },
      workbox: {
        globPatterns: ['**/*'],
        // §perf: exclude wasm from precache (6 MB = 96% of install cost).
        // Runtime caching below makes it offline-capable after first use.
        globIgnores: ['**/*.wasm'],
        maximumFileSizeToCacheInBytes: 32 * 1024 * 1024,
        cleanupOutdatedCaches: true,
        inlineWorkboxRuntime: true,
        // Runtime cache wasm so first online use makes it offline-capable.
        runtimeCaching: [
          {
            urlPattern: /\.wasm$/,
            handler: 'CacheFirst',
            options: {
              cacheName: 'ridge-wasm',
              expiration: { maxEntries: 4, maxAgeSeconds: 30 * 24 * 60 * 60 },
            },
          },
        ],
        // Offline SPA navigations fall back to the cached shell, EXCEPT for the
        // API / WS / cert / download routes which must always hit the network.
        //   * `?ui=desktop` 永远走网络 → Rust 端 serve.rs 根据 ua.rs prefer_desktop_ui
        //     返回桌面 SPA shell。若 SW 拦截离线命中，固定返回 mobile 壳 → 「缓存
        //     固定返回错误 UI 壳」bug（Goal #2）。离线时 `?ui=desktop` 退化为「请连
        //     网后使用桌面版」错误页（由浏览器默认 + index.html 的 nopin 标头），
        //     不强塞 mobile 壳。
        navigateFallback: 'index.html',
        navigateFallbackDenylist: [
          /^\/ws/,
          /^\/info/,
          /^\/verify/,
          /^\/health/,
          /^\/status/,
          /^\/session/,
          /^\/workspace/,
          /^\/ridge-ca/,
          /^\/assets\//,
          // ?ui=desktop 必须直达网络，否则离线下 SW 命中 mobile 缓存 → 用户
          // 进桌面失败却看见 mobile 壳（看起来「能进」实际错壳，更难诊断）。
          /[?&]ui=desktop(?:&|$)/,
        ],
      },
      // No service worker during `pnpm dev:remote` — avoids stale-cache pain
      // while iterating; the SW only ships in the production build.
      devOptions: { enabled: false },
    }),
  ],
  build: {
    outDir: path.resolve(__dirname, 'remote-dist/mobile'),
    chunkSizeWarningLimit: 500,
    emptyOutDir: true,
    target: 'esnext',
    modulePreload: false,
    // §perf: 不用 manualChunks 强制成块。粗粒度规则会让 rollup 把 __vitePreload
    // helper co-locate 进大块，entry 静态 import helper 反把 terminal-canvas /
    // workspace-tree / icons / term-wasm / virtual-keyboard 全拖进 eager graph。
    // 同 vite.config.js:122-133 §perf 已修复的 bug，此处曾复现，现已删除。
    // 让 Rollup 自然检测 async chunk — MainApp 的 5 个 gated dynamic import 才真正生效。
    rollupOptions: {},
  },
  worker: {
    format: 'es',
  },
  optimizeDeps: {
    exclude: ['@ridge/term-wasm'],
  },
  server: {
    host: '0.0.0.0',
    port: 5174,
    strictPort: true,
    // Keep the browser's HMR client on the same fixed port as this server.
    // Without an explicit clientPort, a page that was first opened through a
    // desktop Vite instance can retain that instance's stale port (for
    // example 7734) and request old `/@fs/...` modules after Remote reloads.
    hmr: {
      protocol: 'ws',
      clientPort: 5174,
    },
    proxy: {
      '/ws': {
        target: 'ws://127.0.0.1:9527',
        ws: true,
      },
      '/info': { target: 'http://127.0.0.1:9527' },
      '/verify': { target: 'http://127.0.0.1:9527' },
      '/health': { target: 'http://127.0.0.1:9527' },
      '/status': { target: 'http://127.0.0.1:9527' },
      '/workspace': { target: 'http://127.0.0.1:9527' },
      '/ridge-ca.crt': { target: 'http://127.0.0.1:9527' },
      '/ridge-ca.pem': { target: 'http://127.0.0.1:9527' },
    },
  },
});
