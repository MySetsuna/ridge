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
  },

  // 优化依赖预构建
  optimizeDeps: {
    include: [
      'monaco-editor',
      'svelte-splitpanes',
      '@tauri-apps/api'
    ],
    exclude: ['@ridge/term-wasm'],
  },
  assetsInclude: ['**/*.wasm'],
});