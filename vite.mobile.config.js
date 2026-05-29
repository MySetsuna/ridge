import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  root: path.resolve(__dirname, 'src/mobile'),
  base: '/',
  // Use a SEPARATE dep-optimization cache from the desktop SvelteKit dev server
  // (vite.config.js → default node_modules/.vite). Both run at once when remote
  // control is enabled; sharing one cache makes the mobile server re-optimize
  // and wipe the desktop's deps (e.g. `qrcode`), 404-ing the desktop's
  // already-loaded `qrcode.js?v=…` dynamic import.
  cacheDir: path.resolve(__dirname, 'node_modules/.vite-mobile'),
  resolve: {
    alias: {
      '@ridge/term-wasm': path.resolve(__dirname, 'packages/ridge-term/pkg'),
      // Shared, transport-agnostic UI (e.g. the sidebar components reused
      // between the desktop SvelteKit app and this plain-Svelte remote app).
      '@shared': path.resolve(__dirname, 'src/shared'),
    },
  },
  plugins: [
    svelte(),
  ],
  build: {
    outDir: path.resolve(__dirname, 'static/mobile'),
    emptyOutDir: true,
    target: 'esnext',
    modulePreload: false,
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
    },
  },
});
