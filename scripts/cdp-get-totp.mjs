#!/usr/bin/env node
// Read the live remote pairing code from the running ridge WebView via CDP.
// READ-ONLY: it only invokes the `get_remote_info` Tauri command and prints
// totpCode/lanIp/port — no DOM/navigation changes, safe alongside another CDP
// client. The TOTP secret is in-memory only (auth.rs generate_secret), so this
// is the sole way to obtain the current code programmatically.
import http from 'node:http';

const port = process.env.CDP_PORT ?? '9222';
const host = '127.0.0.1';

function fetchJson(path) {
  return new Promise((resolve, reject) => {
    const req = http.get({ host, port, path, timeout: 3000 }, (res) => {
      let body = '';
      res.setEncoding('utf8');
      res.on('data', (c) => (body += c));
      res.on('end', () => {
        try { resolve(JSON.parse(body)); } catch (e) { reject(e); }
      });
    });
    req.on('timeout', () => req.destroy(new Error('timeout')));
    req.on('error', reject);
  });
}

const targets = await fetchJson('/json/list');
const ridge = targets.find(
  (t) => t.type === 'page' &&
    typeof t.url === 'string' &&
    (t.url.includes('tauri.localhost') || t.url.startsWith('tauri://') ||
     t.title === 'Ridge' || t.url.includes(':1420') || t.url.includes(':5173')),
);
if (!ridge) {
  console.error('[totp] no ridge page target found. Targets:');
  for (const t of targets) console.error(`  [${t.type}] ${t.url} — ${t.title}`);
  process.exit(1);
}

const ws = new WebSocket(ridge.webSocketDebuggerUrl);
const send = (() => { let id = 0; return (method, params) => {
  const mid = ++id;
  ws.send(JSON.stringify({ id: mid, method, params }));
  return mid;
}; })();

const want = new Map();
ws.addEventListener('message', (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.id && want.has(msg.id)) { want.get(msg.id)(msg); want.delete(msg.id); }
});
const call = (method, params) => new Promise((resolve) => { const mid = send(method, params); want.set(mid, resolve); });

ws.addEventListener('open', async () => {
  await call('Runtime.enable', {});
  const r = await call('Runtime.evaluate', {
    expression: "window.__TAURI__.core.invoke('get_remote_info')",
    awaitPromise: true,
    returnByValue: true,
  });
  const val = r?.result?.result?.value;
  if (r?.result?.exceptionDetails || !val) {
    console.error('[totp] evaluate failed:', JSON.stringify(r?.result?.exceptionDetails || r));
    process.exit(2);
  }
  // Machine-readable last line for piping; human lines above.
  console.error(`[totp] lanIp=${val.lanIp} port=${val.port} ready=${val.ready} remoteEnabled=${val.remoteEnabled}`);
  console.log(val.totpCode);
  ws.close();
  process.exit(0);
});
ws.addEventListener('error', (e) => { console.error('[totp] ws error', e.message || e); process.exit(3); });
