// Isolated create-workspace test: does create-workspace orphan the PREVIOUS
// workspace at the SOURCE, or was the earlier prev-ws orphan the other session's
// concurrent desktop on the shared workspace? switch-workspace is per-connection,
// so a workspace this mobile client creates is NOT viewed by the other session.
import { chromium, devices } from '@playwright/test';
import http from 'node:http';
import path from 'path';
import { fileURLToPath } from 'url';
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const PROFILE_DIR = path.resolve(__dirname, '..', '.pw-remote-profile');
const URL = process.env.RIDGE_URL || 'https://127.0.0.1:9528';
const CODE = (process.env.RIDGE_CODE || '').replace(/\D/g, '').slice(0, 6);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[cws]', ...a);

const fj = (p) => new Promise((res, rej) => { const r = http.get({ host: '127.0.0.1', port: '9222', path: p, timeout: 3000 }, (x) => { let b = ''; x.on('data', (c) => (b += c)); x.on('end', () => { try { res(JSON.parse(b)); } catch (e) { rej(e); } }); }); r.on('timeout', () => r.destroy(new Error('t'))); r.on('error', rej); });
const targets = await fj('/json/list');
const ridge = targets.find((t) => t.type === 'page' && typeof t.url === 'string' && (t.url.includes('tauri.localhost') || t.url.startsWith('tauri://') || t.title === 'Ridge' || t.url.includes(':1420') || t.url.includes(':5173')));
const cdpWs = new WebSocket(ridge.webSocketDebuggerUrl);
let cid = 0; const cwant = new Map();
cdpWs.addEventListener('message', (e) => { const m = JSON.parse(e.data); if (m.id && cwant.has(m.id)) { cwant.get(m.id)(m); cwant.delete(m.id); } });
const cdpCall = (method, params) => new Promise((r) => { const i = ++cid; cwant.set(i, r); cdpWs.send(JSON.stringify({ id: i, method, params })); });
await new Promise((r) => cdpWs.addEventListener('open', r));
await cdpCall('Runtime.enable', {});
const dbg = async () => (await cdpCall('Runtime.evaluate', { expression: "window.__TAURI__.core.invoke('get_remote_info')", awaitPromise: true, returnByValue: true }))?.result?.result?.value?.paneDebug || [];
const wsOf = (pd, id) => pd.find((w) => w.ws === id);
const ids = (pd) => pd.map((w) => w.ws);

const { defaultBrowserType, ...iphone } = devices['iPhone 13'];
const ctx = await chromium.launchPersistentContext(PROFILE_DIR, { headless: true, ignoreHTTPSErrors: true, serviceWorkers: 'block', ...iphone });
const page = ctx.pages()[0] || (await ctx.newPage());
await page.goto(URL, { waitUntil: 'domcontentloaded' });
await sleep(1500);
if (await page.locator('input[inputmode="numeric"]').count()) { if (!CODE) { log('need RIDGE_CODE'); process.exit(3); } await page.locator('input[inputmode="numeric"]').fill(CODE); await page.locator('button').first().click(); }
await page.waitForSelector('.app-root', { timeout: 20000 });
await sleep(1200);
const openTree = async () => { if (!(await page.locator('.tree-popup').count())) { await page.locator('.tree-trigger').click(); await page.waitForSelector('.tree-popup'); await sleep(150); } };
const newWs = async () => { await openTree(); await page.locator('.tree-add').first().evaluate((el) => el.click()); await sleep(2500); };
const newPane = async () => { await openTree(); const n = await page.locator('.pane-row').count(); await page.locator('.pane-new').first().evaluate((el) => el.click()); for (let k = 0; k < 25 && (await page.locator('.pane-row').count()) <= n; k++) await sleep(200); await sleep(800); };

// 1) create WS_A (mine, isolated) — switches this client to it
const before = ids(await dbg());
await newWs();
const a = ids(await dbg()).find((x) => !before.includes(x));
log(`WS_A=${a?.slice(0, 4)} after create: ${JSON.stringify(wsOf(await dbg(), a))}`);
// 2) add a pane in WS_A
await newPane();
const aState1 = wsOf(await dbg(), a);
log(`WS_A after +pane: L${aState1.leaves} T${aState1.terminals} orphanT=${JSON.stringify(aState1.orphanTerminals)}`);
// 3) create WS_B → WS_A becomes the PREVIOUS workspace
const beforeB = ids(await dbg());
await newWs();
const b = ids(await dbg()).find((x) => !beforeB.includes(x));
await sleep(1500);
const aState2 = wsOf(await dbg(), a);
log(`WS_B=${b?.slice(0, 4)} created. WS_A (now prev): L${aState2.leaves} T${aState2.terminals} orphanT=${JSON.stringify(aState2.orphanTerminals)}`);
const orphaned = (aState2.orphanTerminals?.length || 0) + (aState2.orphanPending?.length || 0) > 0;
log(`RESULT: create-workspace ${orphaned ? 'DID ❌ orphan the previous (mine-only) workspace → real source bug' : 'did NOT ✅ orphan an isolated workspace → earlier orphan was the concurrent other session'}`);

// cleanup: close WS_A and WS_B (best effort)
try {
  await openTree();
  for (const w of [a, b]) {
    const nm = wsOf(await dbg(), w);
    if (!nm) continue;
  }
} catch {}
await cdpWs.close?.();
await ctx.close();
process.exit(orphaned ? 1 : 0);
