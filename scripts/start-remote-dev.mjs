#!/usr/bin/env node
// Start the remote dev server.
//
// In dev mode, this runs the Vite dev server for the remote app (src/remote/)
// on port 5174 with HMR support.
//
// The remote app connects to the Ridge Tauri app's remote WebSocket server.
// Run `pnpm tauri dev` in another terminal for the full backend.
//
// Usage:
//   pnpm dev:remote          # start remote Vite dev server
//   pnpm dev:remote --build  # build remote app + start standalone binary

import { spawn, execSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, '..');

const args = process.argv.slice(2);

if (args.includes('--build') || args.includes('-b')) {
  // Build mode: build remote app then start standalone binary
  console.log('[ridge-remote] Building remote app...');
  try {
    execSync('pnpm build:remote', { cwd: root, stdio: 'inherit' });
  } catch {
    console.error('[ridge-remote] Remote build failed');
    process.exit(1);
  }

  console.log('[ridge-remote] Building standalone server binary...');
  try {
    execSync('cargo build --bin remote-server --manifest-path src-tauri/Cargo.toml', {
      cwd: root,
      stdio: 'inherit',
    });
  } catch {
    console.error('[ridge-remote] Standalone server build failed');
    process.exit(1);
  }

  const binaryPath = path.resolve(root, 'src-tauri', 'target', 'debug', 'remote-server.exe');
  console.log(`[ridge-remote] Starting standalone server: ${binaryPath}`);
  const child = spawn(binaryPath, [], {
    cwd: root,
    stdio: 'inherit',
    env: { ...process.env },
  });
  child.on('exit', (code) => process.exit(code ?? 0));
} else {
  // Dev mode: start Vite dev server for the remote app
  console.log('[ridge-remote] Starting remote Vite dev server on port 5174...');
  console.log('[ridge-remote] Make sure `pnpm tauri dev` is running in another terminal for the backend.');
  console.log();
  const child = spawn('pnpm', ['exec', 'vite', 'dev', '--config', 'vite.remote.config.js'], {
    cwd: root,
    stdio: 'inherit',
    shell: true,
    env: { ...process.env },
  });
  child.on('exit', (code) => process.exit(code ?? 0));
}