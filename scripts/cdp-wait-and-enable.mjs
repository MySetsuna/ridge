#!/usr/bin/env node
// Wait until the dev host has rebuilt to a binary that includes get_remote_info's
// `paneDebug` field (proves the new code is live), surviving CDP up/down churn,
// then enable remote and print {port, code}. dev-only.
import http from 'node:http';
const CDP = process.env.CDP_PORT || '9222';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const fetchJson = (p) => new Promise((res, rej) => { const r = http.get({ host: '127.0.0.1', port: CDP, path: p, timeout: 2500 }, (x) => { let b = ''; x.on('data', (c) => (b += c)); x.on('end', () => { try { res(JSON.parse(b)); } catch (e) { rej(e); } }); }); r.on('timeout', () => r.destroy(new Error('t'))); r.on('error', rej); });

async function rpc(expr) {
  const targets = await fetchJson('/json/list');
  const ridge = targets.find((t) => t.type === 'page' && typeof t.url === 'string' && (t.url.includes('tauri.localhost') || t.url.startsWith('tauri://') || t.title === 'Ridge' || t.url.includes(':1420') || t.url.includes(':5173')));
  if (!ridge) throw new Error('no ridge target');
  const ws = new WebSocket(ridge.webSocketDebuggerUrl);
  let id = 0; const want = new Map();
  ws.addEventListener('message', (ev) => { const m = JSON.parse(ev.data); if (m.id && want.has(m.id)) { want.get(m.id)(m); want.delete(m.id); } });
  const call = (method, params) => new Promise((resolve) => { const i = ++id; want.set(i, resolve); ws.send(JSON.stringify({ id: i, method, params })); });
  await new Promise((r, j) => { ws.addEventListener('open', r); ws.addEventListener('error', j); });
  await call('Runtime.enable', {});
  const r = await call('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true });
  ws.close();
  return r?.result?.result?.value;
}

// 1) wait for new binary (paneDebug present)
let ready = false;
for (let i = 0; i < 120; i++) {
  try { const info = await rpc("window.__TAURI__.core.invoke('get_remote_info')"); if (info && Array.isArray(info.paneDebug)) { console.error(`[wait] new binary live after ~${i * 3}s (paneDebug present)`); ready = true; break; } else { console.error(`[wait] ${i * 3}s: old binary (no paneDebug)`); } }
  catch { /* CDP down mid-rebuild */ }
  await sleep(3000);
}
if (!ready) { console.error('[wait] timed out waiting for new binary'); process.exit(1); }

// 2) enable remote + wait ready
await rpc("window.__TAURI__.core.invoke('set_remote_enabled', { enabled: true }).catch(()=>{})");
for (let i = 0; i < 30; i++) {
  try { const info = await rpc("window.__TAURI__.core.invoke('get_remote_info')"); if (info && info.ready && info.port > 0) { console.error(`[wait] remote ready port=${info.port}`); console.log(JSON.stringify({ port: info.port, code: info.totpCode })); process.exit(0); } } catch {}
  await sleep(1000);
}
console.error('[wait] remote did not become ready'); process.exit(2);
