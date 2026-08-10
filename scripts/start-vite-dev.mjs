// Start Vite dev server for Tauri
// Respects TAURI_SKIP_VITE_DEV environment variable

import { spawn } from 'node:child_process';

export function viteArgs(env = process.env) {
  const args = ['run', 'dev'];
  if (env.RIDGE_DEV_SERVER_PORT) args.push('--host', '127.0.0.1', '--port', env.RIDGE_DEV_SERVER_PORT, '--strictPort');
  return args;
}

export function main({ env = process.env, spawnImpl = spawn, io = console, onSignal = process.on } = {}) {
  if (env.TAURI_SKIP_VITE_DEV) { io.log('[start-vite-dev] TAURI_SKIP_VITE_DEV set, skipping Vite dev server'); return Promise.resolve(0); }
  io.log('[start-vite-dev] Starting Vite dev server...');
  const child = spawnImpl('pnpm', viteArgs(env), {
    stdio: 'inherit', shell: true,
    env: { ...env, RIDGE_CLOUD_BASE_DOMAIN: env.RIDGE_CLOUD_BASE_DOMAIN || 'localhost:5001' },
  });
  onSignal('SIGINT', () => child.kill('SIGINT'));
  onSignal('SIGTERM', () => child.kill('SIGTERM'));
  return new Promise((resolveExit) => {
    child.on('exit', (code) => resolveExit(code ?? 0));
    child.on('error', (err) => { io.error('[start-vite-dev] Failed to start:', err); resolveExit(1); });
  });
}

if (process.argv[1] && process.argv[1].endsWith('start-vite-dev.mjs')) main().then((code) => process.exit(code));
