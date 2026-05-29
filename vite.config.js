// vite.config.js
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
// @ts-ignore — @tailwindcss/vite v4 ships ESM-only with package `exports`
// that tsconfig `moduleResolution: "Node"` cannot resolve; resolved fine at
// runtime by vite's bundler-style resolver.
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  plugins: [
    sveltekit(),
    tailwindcss(), // 如果你使用了 Tailwind
  ],

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