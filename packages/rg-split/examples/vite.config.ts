import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  plugins: [sveltekit()],
  resolve: {
    alias: {
      'rg-split': path.resolve(__dirname, '../src/lib/index.ts')
    }
  }
});
