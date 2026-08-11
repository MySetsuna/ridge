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
import { DEV_USER_DATA_DIR, readDevToolsActivePort, shouldAnnounceCdpPort } from './cdp-port.mjs';
import { applyKernelBreakawayPolicy, cloudBrowserNetworkArgs } from './tauri-dev-cdp-env.mjs';
import { cargoTool, systemTool } from './lib/toolPath.mjs';

const userDataDir = DEV_USER_DATA_DIR;
const root = path.resolve(import.meta.dirname, '..');
const devKernelDataDir = path.join(root, '.iteration', 'dev-kernel-isolated-cdp');
const configuredTargetDir = process.env.RIDGE_CDP_TARGET_DIR?.trim();
const cargoTargetDir = configuredTargetDir ? path.resolve(configuredTargetDir) : path.join(root, 'target');
const cargoEnv = {
  ...process.env,
  CARGO_TARGET_DIR: cargoTargetDir,
};
// Tauri's own `dev` child must use the same isolated target as the explicit
// prebuilds above; otherwise it reopens the shared `target/debug/ridge.exe`
// and collides with an existing desktop dev process on Windows.
process.env.CARGO_TARGET_DIR = cargoTargetDir;
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
  `--remote-debugging-port=0 --remote-debugging-address=127.0.0.1 --remote-allow-origins=*${forcedScaleArg}${cloudBrowserNetworkArgs(process.env.RIDGE_CLOUD_BASE_DOMAIN)}`;
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
// CREATE_BREAKAWAY_FROM_JOB. Keep the production fail-closed behavior by
// default; only an explicitly requested constrained-host run may opt into the
// bounded same-job fallback. Otherwise a force-killed CDP shell would also
// reap the kernel and produce a false lifecycle result.
applyKernelBreakawayPolicy(process.env);
fs.mkdirSync(userDataDir, { recursive: true });
fs.mkdirSync(devKernelDataDir, { recursive: true });
// The normal Tauri config rebuilds the release teammate shim on every dev
// launch. That is correct for a cold developer setup, but it serializes CDP
// smoke runs behind a second release Cargo build and can contend with the
// isolated debug target. CDP already supplies the debug sidecar explicitly;
// only refresh the frontend bundle and start Vite here.
fs.writeFileSync(configFile, JSON.stringify({
  build: {
    beforeDevCommand: 'node packages/ridge-term/build.mjs --dev && node scripts/start-vite-dev.mjs',
    devUrl: `http://127.0.0.1:${vitePort}`,
  },
}));

// The embedded Kernel must not keep Cargo's shared `target/debug/ridge.exe`
// open across a desktop-shell restart. Build and copy a per-run host binary;
// the Tauri shell may then be rebuilt while the previous Kernel remains alive.
const desktopBuild = spawnSync(cargoTool('cargo'), ['build', '-p', 'ridge'], {
  cwd: path.resolve(import.meta.dirname, '..'),
  env: cargoEnv,
  stdio: 'inherit',
  shell: process.platform === 'win32',
});
if (desktopBuild.status !== 0) {
  console.error(`[tauri-dev-cdp] ridge desktop build failed (exit ${desktopBuild.status ?? 'unknown'})`);
  process.exit(desktopBuild.status ?? 1);
}

// The LAN host is a detached `rdg` sidecar, not part of the Tauri crate. Build
// it last so a desktop build cannot leave the copied Remote binary behind the
// current checkout; otherwise Remote may run yesterday's CLI.
const cliBuild = spawnSync(cargoTool('cargo'), ['build', '-p', 'ridge-cli', '--bin', 'rdg'], {
  cwd: path.resolve(import.meta.dirname, '..'),
  env: cargoEnv,
  stdio: 'inherit',
  shell: process.platform === 'win32',
});
if (cliBuild.status !== 0) {
  console.error(`[tauri-dev-cdp] ridge-cli debug build failed (exit ${cliBuild.status ?? 'unknown'})`);
  process.exit(cliBuild.status ?? 1);
}

const ridgeSource = path.join(cargoTargetDir, 'debug', process.platform === 'win32' ? 'ridge.exe' : 'ridge');
const ridgeKernelCopy = path.join(
  userDataDir,
  `ridge-kernel-${process.pid}${process.platform === 'win32' ? '.exe' : ''}`,
);
fs.copyFileSync(ridgeSource, ridgeKernelCopy);
process.env.RIDGE_KERNEL_HOST_BINARY = ridgeKernelCopy;
console.log(`[tauri-dev-cdp] detached kernel binary: ${ridgeKernelCopy}`);

const rdgSource = path.join(cargoTargetDir, 'debug', process.platform === 'win32' ? 'rdg.exe' : 'rdg');
const rdgSourceStat = fs.statSync(rdgSource);
const rdgCodeStat = fs.statSync(path.join(root, 'packages', 'ridge-cli', 'src', 'tui', 'lan_host_impl.rs'));
if (rdgSourceStat.mtimeMs < rdgCodeStat.mtimeMs) {
  console.error('[tauri-dev-cdp] rdg sidecar is older than its LAN host source; refusing stale E2E binary');
  process.exit(1);
}
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

// Poll for the dynamic port and surface it whenever the webview registers a
// new CDP endpoint. Tauri/Vite rebuilds can restart WebView2 in-place, which
// replaces DevToolsActivePort while this launcher remains alive.
let announcedPort = null;
const poll = setInterval(() => {
  const port = readDevToolsActivePort();
  if (shouldAnnounceCdpPort(port, announcedPort)) {
    announcedPort = port;
    try { fs.writeFileSync(portFile, String(port)); } catch { /* ignore */ }
    console.log(`\n[tauri-dev-cdp] ✅ CDP ready on port ${port}  →  http://127.0.0.1:${port}/json/version`);
    console.log(`[tauri-dev-cdp]    attach: CDP_PORT=${port} pnpm cdp:smoke   (or just \`pnpm cdp:smoke\`)\n`);
  }
}, 1000);

const child = spawn(systemTool('pnpm'), [
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
