import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { VitePWA } from 'vite-plugin-pwa';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  root: path.resolve(__dirname, 'src/remote'),
  base: '/',
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
      // Icons / favicon live in src/remote/public and need precaching too.
      includeAssets: ['favicon.png', 'apple-touch-icon.png', 'icon-192.png', 'icon-512.png', 'icon-maskable-512.png'],
      manifest: {
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
        // Precache the built shell + assets, including the terminal wasm.
        globPatterns: ['**/*.{js,css,html,wasm,svg,png,ico,webp,woff,woff2,webmanifest}'],
        // Keep the flag-only emoji subset OUT of the precache so it stays truly
        // on-demand: the unicode-range @font-face (injected only on flag-less
        // OSes — see flagEmojiSupport.ts) makes the browser fetch flags.woff2
        // exactly once, when a flag codepoint first appears. mac/iOS render
        // flags natively and never download it; first paint stays font-request
        // free (design §8).
        globIgnores: ['**/fonts/flags.woff2'],
        // The term-wasm bundle is large; raise the precache size ceiling.
        maximumFileSizeToCacheInBytes: 12 * 1024 * 1024,
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
