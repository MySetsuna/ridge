// Deterministic reap test over CDP: read paneDebug (orphan ids), invoke
// remote_reap_orphans, read again. Isolates "does reap_all actually remove
// orphans" from WS triggers / Playwright / the other session. dev-only.
import http from 'node:http';
const CDP = process.env.CDP_PORT || '9222';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const fj = (p) => new Promise((res, rej) => { const r = http.get({ host: '127.0.0.1', port: CDP, path: p, timeout: 3000 }, (x) => { let b = ''; x.on('data', (c) => (b += c)); x.on('end', () => { try { res(JSON.parse(b)); } catch (e) { rej(e); } }); }); r.on('timeout', () => r.destroy(new Error('t'))); r.on('error', rej); });

const targets = await fj('/json/list');
const ridge = targets.find((t) => t.type === 'page' && typeof t.url === 'string' && (t.url.includes('tauri.localhost') || t.url.startsWith('tauri://') || t.title === 'Ridge' || t.url.includes(':1420') || t.url.includes(':5173')));
if (!ridge) { console.log('no ridge target'); process.exit(1); }
const ws = new WebSocket(ridge.webSocketDebuggerUrl);
let id = 0; const want = new Map();
ws.addEventListener('message', (e) => { const m = JSON.parse(e.data); if (m.id && want.has(m.id)) { want.get(m.id)(m); want.delete(m.id); } });
const call = (method, params) => new Promise((r) => { const i = ++id; want.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
await new Promise((r) => ws.addEventListener('open', r));
await call('Runtime.enable', {});
const evalp = async (expr) => { const r = await call('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true }); if (r?.result?.exceptionDetails) throw new Error(JSON.stringify(r.result.exceptionDetails)); return r?.result?.result?.value; };

const fmt = (pd) => (pd || []).map((w) => `${w.ws.slice(0, 4)}{L${w.leaves} T${w.terminals} P${w.pending}${(w.orphanTerminals?.length || w.orphanPending?.length) ? ' orphanT=' + JSON.stringify((w.orphanTerminals || []).map((x) => x.slice(0, 4))) + ' orphanP=' + JSON.stringify((w.orphanPending || []).map((x) => x.slice(0, 4))) : ''}}`).join('  ');

const before = await evalp("window.__TAURI__.core.invoke('get_remote_info')");
console.log('BEFORE:', fmt(before.paneDebug));
let reaped;
try { reaped = await evalp("window.__TAURI__.core.invoke('remote_reap_orphans')"); }
catch (e) { console.log('remote_reap_orphans NOT FOUND (old binary?):', e.message.slice(0, 120)); ws.close(); process.exit(2); }
console.log('reaped count:', reaped);
await sleep(800);
const after = await evalp("window.__TAURI__.core.invoke('get_remote_info')");
console.log('AFTER :', fmt(after.paneDebug));
const orphans = (after.paneDebug || []).some((w) => (w.orphanTerminals?.length || 0) + (w.orphanPending?.length || 0) > 0);
console.log(`RESULT: ${orphans ? 'FAIL ❌ orphans remain after reap' : 'PASS ✅ reap cleared all orphans'}`);
ws.close();
process.exit(orphans ? 1 : 0);
