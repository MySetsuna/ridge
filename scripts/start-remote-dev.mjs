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

export function parseArgs(args = []) { return { build: args.includes('--build') || args.includes('-b') }; }

export async function main({ args = process.argv.slice(2), execSyncImpl = execSync, spawnImpl = spawn, io = console } = {}) {
if (parseArgs(args).build) {
  // Build mode: build remote app then start standalone binary
  console.log('[ridge-remote] Building remote app...');
  try {
    execSyncImpl('pnpm build:remote', { cwd: root, stdio: 'inherit' });
  } catch {
    io.error('[ridge-remote] Remote build failed');
    return 1;
  }

  console.log('[ridge-remote] Building standalone server binary...');
  try {
    execSyncImpl('cargo build --bin remote-server --manifest-path src-tauri/Cargo.toml', {
      cwd: root,
      stdio: 'inherit',
    });
  } catch {
    io.error('[ridge-remote] Standalone server build failed');
    return 1;
  }

  const binaryPath = path.resolve(root, 'src-tauri', 'target', 'debug', 'remote-server.exe');
  console.log(`[ridge-remote] Starting standalone server: ${binaryPath}`);
  const child = spawnImpl(binaryPath, [], {
    cwd: root,
    stdio: 'inherit',
    env: { ...process.env },
  });
  return new Promise((resolveExit) => child.on('exit', (code) => resolveExit(code ?? 0)));
} else {
  // Dev mode: start Vite dev server for the remote app
  io.log('[ridge-remote] Starting remote Vite dev server on port 5174...');
  io.log('[ridge-remote] Make sure `pnpm tauri dev` is running in another terminal for the backend.');
  io.log();
  const child = spawnImpl('pnpm', ['exec', 'vite', 'dev', '--config', 'vite.remote.config.js'], {
    cwd: root,
    stdio: 'inherit',
    shell: true,
    env: { ...process.env },
  });
  return new Promise((resolveExit) => child.on('exit', (code) => resolveExit(code ?? 0)));
}
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url))) {
  process.exit(await main());
}
