// PTY-leak trace: drive the mobile remote (Playwright/9528) through each pane &
// workspace operation, reading per-workspace pane_tree leaves vs terminals vs
// pending_spawns (via get_remote_info over CDP/9222) after each step. The op
// after which terminals/pending > leaves for a workspace is the orphan source.
import { chromium, devices } from '@playwright/test';
import http from 'node:http';
import path from 'path';
import { fileURLToPath } from 'url';
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const PROFILE_DIR = path.resolve(__dirname, '..', '.pw-remote-profile');
const URL = process.env.RIDGE_URL || 'https://127.0.0.1:9528';
const CODE = (process.env.RIDGE_CODE || '').replace(/\D/g, '').slice(0, 6);
const CDP = process.env.CDP_PORT || '9222';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[leak]', ...a);

// ---- CDP client to read get_remote_info.paneDebug ----
const fetchJson = (p) => new Promise((res, rej) => { const r = http.get({ host: '127.0.0.1', port: CDP, path: p, timeout: 3000 }, (x) => { let b = ''; x.on('data', (c) => (b += c)); x.on('end', () => { try { res(JSON.parse(b)); } catch (e) { rej(e); } }); }); r.on('timeout', () => r.destroy(new Error('t'))); r.on('error', rej); });
const targets = await fetchJson('/json/list');
const ridge = targets.find((t) => t.type === 'page' && typeof t.url === 'string' && (t.url.includes('tauri.localhost') || t.url.startsWith('tauri://') || t.title === 'Ridge' || t.url.includes(':1420') || t.url.includes(':5173')));
const cdpWs = new WebSocket(ridge.webSocketDebuggerUrl);
let cid = 0; const cwant = new Map();
cdpWs.addEventListener('message', (ev) => { const m = JSON.parse(ev.data); if (m.id && cwant.has(m.id)) { cwant.get(m.id)(m); cwant.delete(m.id); } });
const cdpCall = (method, params) => new Promise((resolve) => { const id = ++cid; cwant.set(id, resolve); cdpWs.send(JSON.stringify({ id, method, params })); });
await new Promise((r) => cdpWs.addEventListener('open', r));
await cdpCall('Runtime.enable', {});
const getDebug = async () => { const r = await cdpCall('Runtime.evaluate', { expression: "window.__TAURI__.core.invoke('get_remote_info')", awaitPromise: true, returnByValue: true }); return r?.result?.result?.value?.paneDebug || []; };
const fmt = (d) => d.map((w) => `${w.ws.slice(0, 4)}{L${w.leaves} T${w.terminals} P${w.pending}${(w.terminals > w.leaves || w.pending > w.leaves) ? ' ⚠ORPHAN' : ''}}`).join(' ');
const step = async (label) => { await sleep(1500); log(`${label.padEnd(28)} → ${fmt(await getDebug())}`); };

// ---- Playwright mobile UI ----
const { defaultBrowserType, ...iphone } = devices['iPhone 13'];
const ctx = await chromium.launchPersistentContext(PROFILE_DIR, { headless: true, ignoreHTTPSErrors: true, serviceWorkers: 'block', ...iphone });
const page = ctx.pages()[0] || (await ctx.newPage());
await page.goto(URL, { waitUntil: 'domcontentloaded' });
await sleep(1500);
if (await page.locator('input[inputmode="numeric"]').count()) { if (!CODE) { log('need RIDGE_CODE'); process.exit(3); } await page.locator('input[inputmode="numeric"]').fill(CODE); await page.locator('button').first().click(); }
await page.waitForSelector('.app-root', { timeout: 20000 });
await sleep(1200);
const openTree = async () => { if (!(await page.locator('.tree-popup').count())) { await page.locator('.tree-trigger').click(); await page.waitForSelector('.tree-popup'); await sleep(150); } };
const paneCount = async () => { await openTree(); return page.locator('.pane-row').count(); };
const wsActive = async () => { await openTree(); return page.locator('.ws-row.active .ws-name').first().textContent(); };

await step('0 baseline');
// create 2 panes
for (let i = 0; i < 2; i++) { await openTree(); const n = await page.locator('.pane-row').count(); await page.locator('.pane-new').first().evaluate((el) => el.click()); for (let k = 0; k < 25 && (await page.locator('.pane-row').count()) <= n; k++) await sleep(200); await step(`1.${i + 1} create-pane`); }
// close active pane
await openTree(); await page.locator('.pane-row.active .row-close').first().evaluate((el) => el.click()).catch(() => {}); await step('2 close-pane');
// create workspace
await openTree(); const origWs = (await wsActive())?.trim(); await page.locator('.tree-add').first().evaluate((el) => el.click()); await sleep(1500); await step('3 create-workspace');
// switch back to original
await openTree(); await page.locator('.ws-row', { hasText: origWs }).first().evaluate((el) => el.click()); await sleep(1200); await step('4 switch-ws-back');
// close the created workspace
await openTree(); const cur = await page.evaluate(() => [...document.querySelectorAll('.ws-row .ws-name')].map((e) => e.textContent.trim())); const newWs = cur.find((n) => n !== origWs); if (newWs) await page.locator('.ws-row', { hasText: newWs }).locator('.row-close').first().evaluate((el) => el.click()); await sleep(1200); await step('5 close-created-ws');
// reconnect (reload page → mobile reconnects)
await page.reload({ waitUntil: 'domcontentloaded' }); await sleep(2000); await page.waitForSelector('.app-root', { timeout: 20000 }).catch(() => {}); await step('6 reconnect(reload)');
// create + close a pane after reconnect
await openTree(); { const n = await page.locator('.pane-row').count(); await page.locator('.pane-new').first().evaluate((el) => el.click()); for (let k = 0; k < 25 && (await page.locator('.pane-row').count()) <= n; k++) await sleep(200); } await step('7 create-pane-after-reconnect');

// final: deterministic reap via the manual command (independent of WS triggers),
// then read — proves whether reap_all can actually clear every orphan.
await sleep(1500);
const reaped = await cdpCall('Runtime.evaluate', { expression: "window.__TAURI__.core.invoke('remote_reap_orphans')", awaitPromise: true, returnByValue: true });
log(`manual reap_all count: ${reaped?.result?.result?.value}`);
await sleep(1000);
const finalDbg = await getDebug();
log(`FINAL (after manual reap)    → ${fmt(finalDbg)}`);
const stillOrphan = finalDbg.some((w) => w.terminals > w.leaves || w.pending > w.leaves);
log(`RESULT: ${stillOrphan ? 'FAIL ❌ orphan persists after reap (reap cannot clear it)' : 'PASS ✅ all orphans cleared (reap converges)'}`);
log('done'); await ctx.close(); cdpWs.close(); process.exit(stillOrphan ? 1 : 0);
