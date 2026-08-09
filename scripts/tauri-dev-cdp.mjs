#!/usr/bin/env node
// Launch `tauri dev` with WebView2 CDP remote debugging enabled.
//
// Lets chrome-devtools-mcp (or any DevTools client) attach to the Tauri
// webview — useful for driving the live dev session from external automation,
// capturing console/network/perf traces while iterating on UI / WebGPU bugs.
//
// Why an isolated user-data-dir:
//   The dev and the installed Ridge share the same Tauri bundleIdentifier,
//   which means they default to the SAME WebView2 user-data-dir. When the
//   installed Ridge is already running and has that dir open, launching dev
//   with a different AdditionalBrowserArguments value fails with HRESULT
//   0x8007139F (ERROR_INVALID_STATE). Pointing dev at its own project-local
//   dir sidesteps the conflict AND keeps dev storage out of the installed
//   app's profile.
//
// CDP PORT — DYNAMIC, NOT FIXED (root-caused 2026-06-19):
//   Chromium 136+ (WebView2 149 here) REJECTS a fixed `--remote-debugging-port`
//   (e.g. 9222) as a security hardening — the DevTools endpoint silently never
//   opens and no `DevToolsActivePort` file is written. (Verified in isolation:
//   Edge 149 + `--remote-debugging-port=9223` → dead; `=0` → works.) So we
//   launch with `--remote-debugging-port=0`; Chromium picks a free port and
//   writes it to `<userDataDir>\EBWebView\DevToolsActivePort`. This script then
//   surfaces that real port. CDP clients discover it via `scripts/cdp-port.mjs`
//   (`resolveCdpPort()`), or read `<userDataDir>\cdp-port.txt`, or set CDP_PORT.
//
// Usage:
//   pnpm tauri:dev:cdp                  # then watch for "[tauri-dev-cdp] CDP ready on port N"
//   CDP_PORT=<N> pnpm cdp:smoke         # or just `pnpm cdp:smoke` (auto-discovers)
//
// The env vars only live for this child process; no shell side effects.
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { DEV_USER_DATA_DIR, readDevToolsActivePort } from './cdp-port.mjs';

const userDataDir = DEV_USER_DATA_DIR;
const root = path.resolve(import.meta.dirname, '..');
const devKernelDataDir = path.join(root, '.iteration', 'dev-kernel-isolated-cdp');
const portFile = path.join(userDataDir, 'cdp-port.txt');
const activePortFile = path.join(userDataDir, 'EBWebView', 'DevToolsActivePort');
const configFile = path.join(userDataDir, 'tauri-dev-cdp.config.json');
const vitePort = await new Promise((resolve, reject) => {
  const server = net.createServer();
  server.once('error', reject);
  server.listen(0, '127.0.0.1', () => {
    const address = server.address();
    server.close(() => resolve(address.port));
  });
});

// Start from a clean slate so the DevToolsActivePort we later read is THIS run's.
try { fs.rmSync(activePortFile, { force: true }); } catch { /* ignore */ }
try { fs.rmSync(portFile, { force: true }); } catch { /* ignore */ }

// `--remote-debugging-port=0` (dynamic) is REQUIRED on Chromium 136+ — a fixed
// port is silently ignored. `--remote-allow-origins=*` is REQUIRED for
// chrome-devtools-mcp (and any CDP client sending an Origin header) to attach on
// Chromium 111+; without it the DevTools websocket handshake is rejected (403).
const scaleFactor = process.env.RIDGE_CDP_DEVICE_SCALE_FACTOR?.trim();
const forcedScaleArg = scaleFactor && /^\d+(?:\.\d+)?$/.test(scaleFactor)
  ? ` --force-device-scale-factor=${scaleFactor}`
  : '';
process.env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS =
  `--remote-debugging-port=0 --remote-debugging-address=127.0.0.1 --remote-allow-origins=*${forcedScaleArg}`;
process.env.WEBVIEW2_USER_DATA_FOLDER = userDataDir;
// Keep loopback IPC/Remote traffic off any workspace-wide proxy (NLM uses the
// same proxy for Google traffic, but the local WSS host must never tunnel).
const appendLoopbackNoProxy = (value) => [...new Set(
  `${value ?? ''},localhost,127.0.0.1`.split(',').map((item) => item.trim()).filter(Boolean),
)].join(',');
process.env.NO_PROXY = appendLoopbackNoProxy(process.env.NO_PROXY);
process.env.no_proxy = appendLoopbackNoProxy(process.env.no_proxy);
// Let this debug instance coexist with an already-running installed Ridge:
// the installed app holds the single-instance lock, so without this the dev
// instance would be focused-and-exited on launch. Gated entirely in lib.rs by
// this env var; the installed/release app never sets it. (See docs/CDP_TESTING.md.)
process.env.RIDGE_DISABLE_SINGLE_INSTANCE = '1';
process.env.RIDGE_DEV_SERVER_PORT = String(vitePort);
// Do not attach dev:cdp to the installed Ridge kernel. The desktop and its
// `rdg host` sidecar must share this project-local registry/data graph.
process.env.RIDGE_KERNEL_DATA_DIR = devKernelDataDir;
// Some desktop test hosts run inside a Windows Job that rejects
// CREATE_BREAKAWAY_FROM_JOB. The kernel launcher keeps production fail-closed;
// this explicit dev harness opt-in permits the bounded same-job fallback so
// CDP can still exercise the desktop in that constrained host.
process.env.RIDGE_TEST_ALLOW_NON_BREAKAWAY = '1';
fs.mkdirSync(userDataDir, { recursive: true });
fs.mkdirSync(devKernelDataDir, { recursive: true });
fs.writeFileSync(configFile, JSON.stringify({ build: { devUrl: `http://127.0.0.1:${vitePort}` } }));

// The LAN host is a detached `rdg` sidecar, not part of the Tauri crate. Keep
// the debug sidecar in lockstep with this checkout before launching WebView2;
// otherwise desktop code can be new while Remote still runs yesterday's CLI.
const cliBuild = spawnSync('cargo', ['build', '-p', 'ridge-cli'], {
  cwd: path.resolve(import.meta.dirname, '..'),
  env: process.env,
  stdio: 'inherit',
  shell: process.platform === 'win32',
});
if (cliBuild.status !== 0) {
  console.error(`[tauri-dev-cdp] ridge-cli debug build failed (exit ${cliBuild.status ?? 'unknown'})`);
  process.exit(cliBuild.status ?? 1);
}
const rdgSource = path.join(root, 'target', 'debug', process.platform === 'win32' ? 'rdg.exe' : 'rdg');
const rdgCopy = path.join(
  userDataDir,
  `rdg-dev-${process.pid}${process.platform === 'win32' ? '.exe' : ''}`,
);
fs.copyFileSync(rdgSource, rdgCopy);
process.env.RIDGE_RDG_BINARY = rdgCopy;
console.log(`[tauri-dev-cdp] debug rdg sidecar: ${rdgCopy}`);

console.log(`[tauri-dev-cdp] WebView2 CDP   : dynamic port (Chromium 136+ blocks fixed ports)`);
console.log(`[tauri-dev-cdp] user-data-dir : ${userDataDir}`);
console.log(`[tauri-dev-cdp] kernel-data-dir: ${devKernelDataDir}`);
console.log(`[tauri-dev-cdp] Ridge Vite URL : http://127.0.0.1:${vitePort}`);
console.log(`[tauri-dev-cdp] waiting for DevToolsActivePort after the Ridge window opens…`);

// Poll for the dynamic port and surface it once the webview registers CDP.
let announced = false;
const poll = setInterval(() => {
  const port = readDevToolsActivePort();
  if (port && !announced) {
    announced = true;
    try { fs.writeFileSync(portFile, String(port)); } catch { /* ignore */ }
    console.log(`\n[tauri-dev-cdp] ✅ CDP ready on port ${port}  →  http://127.0.0.1:${port}/json/version`);
    console.log(`[tauri-dev-cdp]    attach: CDP_PORT=${port} pnpm cdp:smoke   (or just \`pnpm cdp:smoke\`)\n`);
  }
}, 1000);

const child = spawn('pnpm', [
  'tauri',
  'dev',
  '--config',
  configFile,
], {
  stdio: 'inherit',
  shell: true,
  env: process.env,
});
child.on('exit', (code) => {
  clearInterval(poll);
  process.exit(code ?? 0);
});
