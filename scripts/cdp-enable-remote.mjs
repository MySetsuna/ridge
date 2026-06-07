#!/usr/bin/env node
// Enable the remote server on the running ridge WebView via CDP, then print the
// resolved port + current pairing code. Used to bring the dev:cdp host's remote
// up after a restart so the mobile-remote e2e can connect. dev-only.
import http from 'node:http';
const port = process.env.CDP_PORT ?? '9222';
const host = '127.0.0.1';
const fetchJson = (p) => new Promise((res, rej) => {
  const r = http.get({ host, port, path: p, timeout: 3000 }, (x) => { let b = ''; x.on('data', (c) => (b += c)); x.on('end', () => { try { res(JSON.parse(b)); } catch (e) { rej(e); } }); });
  r.on('timeout', () => r.destroy(new Error('timeout'))); r.on('error', rej);
});
const targets = await fetchJson('/json/list');
const ridge = targets.find((t) => t.type === 'page' && typeof t.url === 'string' && (t.url.includes('tauri.localhost') || t.url.startsWith('tauri://') || t.title === 'Ridge' || t.url.includes(':1420') || t.url.includes(':5173')));
if (!ridge) { console.error('no ridge target'); process.exit(1); }
const ws = new WebSocket(ridge.webSocketDebuggerUrl);
let id = 0; const want = new Map();
const call = (method, params) => new Promise((resolve) => { const mid = ++id; want.set(mid, resolve); ws.send(JSON.stringify({ id: mid, method, params })); });
ws.addEventListener('message', (ev) => { const m = JSON.parse(ev.data); if (m.id && want.has(m.id)) { want.get(m.id)(m); want.delete(m.id); } });
const evalExpr = (expr) => call('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true });
ws.addEventListener('open', async () => {
  await call('Runtime.enable', {});
  const en = await evalExpr("window.__TAURI__.core.invoke('set_remote_enabled', { enabled: true }).then(()=>'ok').catch(e=>'ERR:'+e)");
  console.error('[enable] set_remote_enabled ->', en?.result?.result?.value);
  // poll get_remote_info until ready (the server bind is async)
  for (let i = 0; i < 30; i++) {
    const r = await evalExpr("window.__TAURI__.core.invoke('get_remote_info')");
    const v = r?.result?.result?.value;
    if (v && v.ready && v.port > 0) { console.error(`[enable] ready port=${v.port} lanIp=${v.lanIp}`); console.log(JSON.stringify({ port: v.port, code: v.totpCode })); ws.close(); process.exit(0); }
    await new Promise((s) => setTimeout(s, 1000));
  }
  console.error('[enable] timed out waiting for ready'); ws.close(); process.exit(2);
});
ws.addEventListener('error', (e) => { console.error('[enable] ws error', e.message || e); process.exit(3); });
