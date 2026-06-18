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
      // subset stays excluded (see globIgnores below) to keep it on-demand.
      includeAssets: [
        'favicon.png',
        'apple-touch-icon.png',
        'icon-192.png',
        'icon-512.png',
        'icon-maskable-512.png',
        '**/*.{png,jpg,jpeg,gif,svg,webp,ico}',
        '**/*.{woff2,woff,ttf}',
        '**/*.{mp3,mp4,wav,ogg,webm}',
        // Negation: keep the flag subset out of precache (mirrors workbox
        // globIgnores) so it stays on-demand — includeAssets is NOT filtered by
        // globIgnores, so without this it would be force-precached.
        '!**/fonts/flags.woff2',
      ],
      manifest: {
        // Stable `id` so the browser treats reinstalls as the same app (and
        // doesn't create a duplicate install). Without it some browsers key the
        // app by start_url, which is fragile.
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
        // Precache the built shell + EVERY static asset type the remote can
        // emit so the SW truly takes over offline: app shell (js/css/html),
        // terminal wasm, fonts (woff2/woff/ttf), all image formats
        // (png/jpg/jpeg/gif/svg/webp/ico), data (json/txt/webmanifest) and
        // media (mp3/mp4/wav/ogg/webm). Missing a type here = that file silently
        // bypasses the cache and breaks offline, which is the bug being fixed.
        globPatterns: [
          '**/*.{js,css,html,wasm}',
          '**/*.{woff2,woff,ttf}',
          '**/*.{png,jpg,jpeg,gif,svg,webp,ico}',
          '**/*.{mp3,mp4,wav,ogg,webm}',
          '**/*.{json,txt,webmanifest}',
        ],
        // Keep the flag-only emoji subset OUT of the precache so it stays truly
        // on-demand: the unicode-range @font-face (injected only on flag-less
        // OSes — see flagEmojiSupport.ts) makes the browser fetch flags.woff2
        // exactly once, when a flag codepoint first appears. mac/iOS render
        // flags natively and never download it; first paint stays font-request
        // free (design §8).
        globIgnores: ['**/fonts/flags.woff2'],
        // The term-wasm bundle is large and bundled media (mp4/webm/ogg) can be
        // larger still; raise the precache size ceiling so big assets are not
        // silently skipped (the default 2 MiB would drop wasm + any video).
        maximumFileSizeToCacheInBytes: 32 * 1024 * 1024,
        cleanupOutdatedCaches: true,
        // Inline the Workbox runtime into sw.js so there is no extra hashed
        // workbox-*.js root file for the server to special-case.
        inlineWorkboxRuntime: true,
        // Offline SPA navigations fall back to the cached shell, EXCEPT for the
        // API / WS / cert / download routes which must always hit the network.
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
        ],
      },
      // No service worker during `pnpm dev:remote` — avoids stale-cache pain
      // while iterating; the SW only ships in the production build.
      devOptions: { enabled: false },
    }),
  ],
  build: {
    outDir: path.resolve(__dirname, 'static/remote'),
    emptyOutDir: true,
    target: 'esnext',
    modulePreload: false,
    // Better code splitting: split by feature/vendor
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes('ridge-term')) return 'term-wasm';
          if (id.includes('node_modules/lucide-svelte')) return 'icons';
          // Split heavy editor/terminal components
          if (id.includes('monaco-editor')) return 'monaco-editor';
          if (id.includes('mermaid')) return 'mermaid';
          // Split virtual keyboard and touch-specific code
          if (id.includes('/remote/lib/VirtualKeyboard') || id.includes('/remote/lib/modState')) return 'virtual-keyboard';
          // Split terminal canvas (heavy WASM-dependent)
          if (id.includes('/remote/lib/TerminalCanvas') || id.includes('/remote/lib/terminalController')) return 'terminal-canvas';
          // Split workspace tree
          if (id.includes('/remote/lib/WorkspaceTree')) return 'workspace-tree';
        },
        // Smaller chunk size for better caching
        chunkSizeWarningLimit: 500,
      },
    },
  },
  optimizeDeps: {
    exclude: ['@ridge/term-wasm'],
  },
  server: {
    host: '0.0.0.0',
    port: 5174,
    strictPort: true,
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
