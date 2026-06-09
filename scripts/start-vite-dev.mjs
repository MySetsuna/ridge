// Start Vite dev server for Tauri
// Respects TAURI_SKIP_VITE_DEV environment variable

import { spawn } from 'node:child_process';

if (process.env.TAURI_SKIP_VITE_DEV) {
  console.log('[start-vite-dev] TAURI_SKIP_VITE_DEV set, skipping Vite dev server');
  process.exit(0);
}

console.log('[start-vite-dev] Starting Vite dev server...');

const child = spawn('pnpm', ['run', 'dev'], {
  stdio: 'inherit',
  shell: true,
  env: {
    ...process.env,
    RIDGE_CLOUD_BASE_DOMAIN: process.env.RIDGE_CLOUD_BASE_DOMAIN || 'localhost:5001',
  },
});

child.on('exit', (code) => {
  process.exit(code ?? 0);
});

child.on('error', (err) => {
  console.error('[start-vite-dev] Failed to start:', err);
  process.exit(1);
});

// Handle parent termination
process.on('SIGINT', () => child.kill('SIGINT'));
process.on('SIGTERM', () => child.kill('SIGTERM'));