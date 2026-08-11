// Build the complete browser Remote SPA into the shared `remote-dist` root.

import { spawn } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { pruneOutputs } from './prune-stale-fonts.mjs';
import { syncGeneratedCspFile } from './sync-generated-csp.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const viteCli = resolve(root, 'node_modules', 'vite', 'bin', 'vite.js');

export function main({ spawnImpl = spawn, prune = pruneOutputs, syncCsp = syncGeneratedCspFile, io = console } = {}) {
  return new Promise((resolveExit) => {
    const child = spawnImpl(process.execPath, [viteCli, 'build'], {
      cwd: root, stdio: 'inherit', env: { ...process.env, RIDGE_WEB_REMOTE: '1' },
    });
    child.on('exit', (code) => {
      if (code === 0) {
        syncCsp(resolve(root, 'remote-dist', 'desktop', 'index.html'));
        try { prune(); }
        catch (e) { io.warn('[build-remote-desktop] prune-stale-fonts failed:', e?.message ?? e); }
      }
      resolveExit(code ?? 1);
    });
  });
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  process.exit(await main());
}
