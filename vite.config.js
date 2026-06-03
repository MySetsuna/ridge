// vite.config.js
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import path from 'path';
import { fileURLToPath } from 'url';
// @ts-ignore — @tailwindcss/vite v4 ships ESM-only with package `exports`
// that tsconfig `moduleResolution: "Node"` cannot resolve; resolved fine at
// runtime by vite's bundler-style resolver.
import tailwindcss from '@tailwindcss/vite';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// §web-remote: `RIDGE_WEB_REMOTE=1 vite build` produces a static SPA of the FULL
// desktop UI for serving to plain browsers by the LAN remote server. Every
// `@tauri-apps/api/*` import is redirected to the WS-backed shims in
// src/lib/transport/tauriShim so the desktop code runs untouched outside Tauri.
// In the normal Tauri build the flag is unset and none of this applies.
const WEB_REMOTE = !!process.env.RIDGE_WEB_REMOTE;
/** @param {string} f */
const shim = (f) => path.resolve(__dirname, 'src/lib/transport/tauriShim', f);
/** @type {Record<string, string>} */
const webRemoteAliases = {};
if (WEB_REMOTE) {
  webRemoteAliases['@tauri-apps/api/core'] = shim('core.ts');
  webRemoteAliases['@tauri-apps/api/event'] = shim('event.ts');
  webRemoteAliases['@tauri-apps/api/window'] = shim('window.ts');
  webRemoteAliases['@tauri-apps/plugin-dialog'] = shim('dialog.ts');
  webRemoteAliases['@tauri-apps/plugin-clipboard-manager'] = shim('clipboard.ts');
}

export default defineConfig({
  plugins: [
    sveltekit(),
    tailwindcss(), // 如果你使用了 Tailwind
  ],

  define: {
    // Build-time flag read by +layout.svelte and the shims. `false` in the
    // Tauri build lets the whole web-remote branch tree-shake away.
    'import.meta.env.RIDGE_WEB_REMOTE': JSON.stringify(WEB_REMOTE),
  },

  resolve: {
    alias: webRemoteAliases,
  },

  // 路径别名在 svelte.config.js 的 kit.alias 中配置（与 SvelteKit / TS 一致）

  // Tauri dev 端口配置
  server: {
    host: '0.0.0.0',
    port: 5173,
    strictPort: false,
    hmr: {
      // 浏览器侧 WebSocket 连接目标必须是可达地址。`0.0.0.0` 只能用于
      // 服务端 bind（监听全部接口），把它透传给 client 会被浏览器拒为
      // ERR_ADDRESS_INVALID，HMR 死循环重连。Tauri WebView 与本机浏览
      // 器都通过 localhost 访问 dev server，写死 localhost 即可。
      protocol: 'ws',
      host: 'localhost',
      port: 5173,
    },
    // 允许 Tauri 的 WebView 访问
    fs: {
      allow: ['..'], // 允许访问 src-tauri 等上级目录
    },
  },

  // 构建配置
  build: {
    target: 'esnext',
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes('node_modules/monaco-editor')) {
            return 'monaco-editor';
          }
          if (id.includes('node_modules/mermaid')) {
            return 'mermaid';
          }
        }
      }
    }
  },

  // 优化依赖预构建
  optimizeDeps: {
    include: [
      'monaco-editor',
      'svelte-splitpanes',
      '@tauri-apps/api',
      'qrcode',
    ],
    exclude: ['@ridge/term-wasm'],
  },
  ssr: {
    noExternal: ['qrcode'],
  },
  assetsInclude: ['**/*.wasm'],

  // (2026-05-22) — render worker 用 `new Worker(url, { type: 'module' })`
  // 创建（src/lib/terminal/workerRendererSingleton.ts），且 worker 本身
  // import 其它模块（renderWorker.ts → handleRequest deps）。Vite 默认
  // `worker.format: 'iife'` 与 code-splitting 不兼容，production build
  // 报错 `Invalid value "iife" for option "worker.format"`。改成 'es' 让
  // worker chunk 走 ESM 输出，与上面的 `type: 'module'` 一致；现代
  // WebView2 (Chromium 148) 完全支持 module workers。
  worker: {
    format: 'es',
  },
});