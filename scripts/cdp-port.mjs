// Resolve the actual CDP port of the `tauri:dev:cdp` WebView2 instance.
//
// WHY THIS EXISTS (2026-06-19): Chromium 136+ (WebView2 149 here) rejects a
// FIXED `--remote-debugging-port` (e.g. 9222) as a security hardening — the
// DevTools endpoint silently never opens, no `DevToolsActivePort` file is
// written. The fix (Microsoft's own WebView2 recipe) is to launch with
// `--remote-debugging-port=0` (dynamic); Chromium then picks a free port and
// writes it to `<userDataDir>\EBWebView\DevToolsActivePort` (line 1). This
// helper discovers that real port so the cdp-*.mjs probes and any CDP client
// can attach.
//
// Verified in isolation: Edge/WebView2 149 + `--remote-debugging-port=9223`
// → no port opens; `--remote-debugging-port=0` → DevToolsActivePort written,
// CDP reachable.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

/** The dev:cdp WebView2 user-data dir (matches tauri-dev-cdp.mjs). */
export const DEV_USER_DATA_DIR = path.resolve(__dirname, '..', '.webview2-dev-cdp');
const DEVTOOLS_ACTIVE_PORT = path.join(DEV_USER_DATA_DIR, 'EBWebView', 'DevToolsActivePort');

/** Read the live CDP port from DevToolsActivePort, or null if absent/invalid. */
export function readDevToolsActivePort() {
  try {
    const txt = fs.readFileSync(DEVTOOLS_ACTIVE_PORT, 'utf8');
    const port = parseInt(txt.split('\n')[0].trim(), 10);
    return Number.isFinite(port) && port > 0 ? port : null;
  } catch {
    return null;
  }
}

/**
 * Resolve the CDP port to use:
 *   1. explicit `CDP_PORT` env override, else
 *   2. the live `DevToolsActivePort`, else
 *   3. 9222 legacy fallback (will fail on Chromium 136+; surfaces a clear error).
 */
export function resolveCdpPort() {
  if (process.env.CDP_PORT) return Number(process.env.CDP_PORT);
  const live = readDevToolsActivePort();
  return live ?? 9222;
}
