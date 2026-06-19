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
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { DEV_USER_DATA_DIR, readDevToolsActivePort } from './cdp-port.mjs';

const userDataDir = DEV_USER_DATA_DIR;
const portFile = path.join(userDataDir, 'cdp-port.txt');
const activePortFile = path.join(userDataDir, 'EBWebView', 'DevToolsActivePort');

// Start from a clean slate so the DevToolsActivePort we later read is THIS run's.
try { fs.rmSync(activePortFile, { force: true }); } catch { /* ignore */ }
try { fs.rmSync(portFile, { force: true }); } catch { /* ignore */ }

// `--remote-debugging-port=0` (dynamic) is REQUIRED on Chromium 136+ — a fixed
// port is silently ignored. `--remote-allow-origins=*` is REQUIRED for
// chrome-devtools-mcp (and any CDP client sending an Origin header) to attach on
// Chromium 111+; without it the DevTools websocket handshake is rejected (403).
process.env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS =
  `--remote-debugging-port=0 --remote-debugging-address=127.0.0.1 --remote-allow-origins=*`;
process.env.WEBVIEW2_USER_DATA_FOLDER = userDataDir;
// Let this debug instance coexist with an already-running installed Ridge:
// the installed app holds the single-instance lock, so without this the dev
// instance would be focused-and-exited on launch. Gated entirely in lib.rs by
// this env var; the installed/release app never sets it. (See docs/CDP_TESTING.md.)
process.env.RIDGE_DISABLE_SINGLE_INSTANCE = '1';

console.log(`[tauri-dev-cdp] WebView2 CDP   : dynamic port (Chromium 136+ blocks fixed ports)`);
console.log(`[tauri-dev-cdp] user-data-dir : ${userDataDir}`);
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

const child = spawn('pnpm', ['tauri', 'dev'], {
  stdio: 'inherit',
  shell: true,
  env: process.env,
});
child.on('exit', (code) => {
  clearInterval(poll);
  process.exit(code ?? 0);
});
