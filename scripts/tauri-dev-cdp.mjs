#!/usr/bin/env node
// Launch `tauri dev` with WebView2 CDP remote debugging enabled.
//
// Lets chrome-devtools-mcp (or any DevTools client) attach to the Tauri
// webview at http://127.0.0.1:9222 — useful for driving the live dev
// session from an external automation, capturing console/network/perf
// traces while iterating on UI / WebGPU rendering bugs.
//
// Why an isolated user-data-dir:
//   The dev and the installed Ridge share the same Tauri bundleIdentifier,
//   which means they default to the SAME WebView2 user-data-dir (under
//   `%LOCALAPPDATA%\com.<bundleId>\EBWebView`). When the installed Ridge
//   is already running (e.g. hosting the user's Claude Code session) and
//   has that dir open, launching dev with a different
//   AdditionalBrowserArguments value fails with HRESULT 0x8007139F
//   (ERROR_INVALID_STATE) — WebView2 refuses two instances with
//   conflicting args on the same data dir. Pointing dev at its own
//   project-local dir sidesteps the conflict entirely AND keeps dev
//   storage out of the installed app's profile.
//
// Usage:
//   pnpm tauri:dev:cdp                  # default port 9222
//   CDP_PORT=9333 pnpm tauri:dev:cdp    # override port
//
// The env vars only live for this child process; no shell side effects.
import { spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const port = process.env.CDP_PORT ?? '9222';
const userDataDir = path.resolve(__dirname, '..', '.webview2-dev-cdp');

// `--remote-allow-origins=*` is REQUIRED for chrome-devtools-mcp (and any
// CDP client that sends an Origin header) to attach on Chromium 111+ — without
// it the DevTools websocket handshake is rejected with HTTP 403. Harmless on
// older runtimes.
//
// NOTE (2026-06-18, WebView2 148.x): on this runtime the DevTools server was
// observed to not initialise the remote-debugging port at all — no listener
// on the port and no `DevToolsActivePort` file is written under the
// user-data-dir despite these flags being applied to the WebView2 process.
// If `pnpm cdp:smoke` can't reach the port, the WebView2 runtime version is
// the likely cause; CDP-based forensics are unavailable until it's resolved.
process.env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS =
  `--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1 --remote-allow-origins=*`;
process.env.WEBVIEW2_USER_DATA_FOLDER = userDataDir;
// Let this debug instance coexist with an already-running installed Ridge:
// the installed app holds the single-instance lock, so without this the dev
// instance would be focused-and-exited on launch. Gated entirely in lib.rs by
// this env var; the installed/release app never sets it. (See docs/CDP_TESTING.md.)
process.env.RIDGE_DISABLE_SINGLE_INSTANCE = '1';

console.log(`[tauri-dev-cdp] WebView2 CDP   : http://127.0.0.1:${port}`);
console.log(`[tauri-dev-cdp] user-data-dir : ${userDataDir}`);

const child = spawn('pnpm', ['tauri', 'dev'], {
  stdio: 'inherit',
  shell: true,
  env: process.env,
});
child.on('exit', (code) => process.exit(code ?? 0));
