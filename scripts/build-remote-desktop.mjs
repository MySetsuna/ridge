// Build the complete browser Remote SPA into the shared `remote-dist` root.

import { spawn } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const viteCli = resolve(root, 'node_modules', 'vite', 'bin', 'vite.js');

const child = spawn(process.execPath, [viteCli, 'build'], {
  cwd: root,
  stdio: 'inherit',
  env: { ...process.env, RIDGE_WEB_REMOTE: '1' },
});

child.on('exit', async (code) => {
  if (code === 0) {
    try {
      await import('./prune-stale-fonts.mjs');
    } catch (e) {
      console.warn('[build-remote-desktop] prune-stale-fonts failed:', e?.message ?? e);
    }
  }
  process.exit(code ?? 1);
});
