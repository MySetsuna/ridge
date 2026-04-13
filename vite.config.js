// vite.config.js
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  plugins: [
    sveltekit(),
    tailwindcss(),           // 如果你使用了 Tailwind
  ],

  // 路径别名在 svelte.config.js 的 kit.alias 中配置（与 SvelteKit / TS 一致）

  // Tauri：使用 1420，避免与常见 Vite 默认端口 5173 冲突导致 beforeDevCommand 失败
  // Windows 上仅监听 ::1 时，WebView 通过 127.0.0.1 访问会连不上，需显式绑定 IPv4
  server: {
    host: '127.0.0.1',
    port: 1420,
    strictPort: true,
    hmr: {
      protocol: 'ws',
      host: '127.0.0.1',
      port: 1420,
    },
    // 允许 Tauri 的 WebView 访问
    fs: {
      allow: ['..'],   // 允许访问 src-tauri 等上级目录
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