#!/usr/bin/env node
// CDP smoke probe — verifies the tauri:dev:cdp WebView2 is reachable at
// http://127.0.0.1:9222 (override with CDP_PORT) and lists every debuggable
// target so we know chrome-devtools-mcp will be able to attach.
//
// Workflow:
//   Terminal 1: pnpm tauri:dev:cdp   (wait for the Ridge window to appear)
//   Terminal 2: pnpm cdp:smoke       (this script)
//
// Exits 0 with the target list when reachable and at least one page target
// exists. Exits 1 with a diagnostic message otherwise.
import http from 'node:http';
import { resolveCdpPort } from './cdp-port.mjs';

// Chromium 136+ uses a DYNAMIC debug port; discover it from DevToolsActivePort
// (or CDP_PORT override). See scripts/cdp-port.mjs + tauri-dev-cdp.mjs.
const port = resolveCdpPort();
const host = '127.0.0.1';
const timeoutMs = Number(process.env.CDP_SMOKE_TIMEOUT_MS ?? 3000);

function fetchJson(path) {
  return new Promise((resolve, reject) => {
    const req = http.get({ host, port, path, timeout: timeoutMs }, (res) => {
      if (res.statusCode !== 200) {
        res.resume();
        reject(new Error(`HTTP ${res.statusCode} for ${path}`));
        return;
      }
      let body = '';
      res.setEncoding('utf8');
      res.on('data', (c) => (body += c));
      res.on('end', () => {
        try {
          resolve(JSON.parse(body));
        } catch (e) {
          reject(new Error(`invalid JSON from ${path}: ${e.message}`));
        }
      });
    });
    req.on('timeout', () => {
      req.destroy(new Error(`timeout after ${timeoutMs}ms hitting ${path}`));
    });
    req.on('error', reject);
  });
}

try {
  const version = await fetchJson('/json/version');
  console.log(`[cdp-smoke] connected to ${host}:${port}`);
  console.log(`[cdp-smoke] browser       : ${version.Browser ?? 'unknown'}`);
  console.log(`[cdp-smoke] protocol      : ${version['Protocol-Version'] ?? 'unknown'}`);
  console.log(`[cdp-smoke] ws endpoint   : ${version.webSocketDebuggerUrl ?? '(none)'}`);

  const targets = await fetchJson('/json/list');
  if (!Array.isArray(targets) || targets.length === 0) {
    console.error('[cdp-smoke] FAIL: /json/list returned no targets');
    process.exit(1);
  }
  console.log(`[cdp-smoke] targets       : ${targets.length}`);
  for (const t of targets) {
    const url = t.url ?? '(no url)';
    const type = t.type ?? '?';
    const title = t.title ? ` — ${t.title}` : '';
    console.log(`  - [${type}] ${url}${title}`);
  }

  const ridge = targets.find(
    (t) =>
      t.type === 'page' &&
      typeof t.url === 'string' &&
      (t.url.startsWith('tauri://') ||
        t.url.startsWith('http://tauri.localhost') ||
        t.url.startsWith('https://tauri.localhost') ||
        t.url.startsWith('http://localhost:1420') ||
        t.url.startsWith('http://127.0.0.1:1420') ||
        t.url.startsWith('http://localhost:5173') ||
        t.url.startsWith('http://127.0.0.1:5173') ||
        (t.title === 'Ridge' && t.url.startsWith('http'))),
  );
  if (!ridge) {
    console.warn('[cdp-smoke] WARN: no obvious Ridge target found (tauri://, tauri.localhost, or :1420)');
    console.warn('[cdp-smoke]       chrome-devtools-tauri can still attach, but verify the page target manually.');
  } else {
    console.log(`[cdp-smoke] ridge target : ${ridge.url}`);
  }
  process.exit(0);
} catch (err) {
  console.error(`[cdp-smoke] FAIL: ${err.message}`);
  console.error('[cdp-smoke] is `pnpm tauri:dev:cdp` running and has the Ridge window opened?');
  process.exit(1);
}
