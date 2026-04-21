// vite.config.js
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
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
      protocol: 'ws',
      host: '0.0.0.0',
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
      'xterm',
      'xterm-addon-fit',
      'monaco-editor',
      'svelte-splitpanes',
      '@tauri-apps/api'
    ],
  },
});