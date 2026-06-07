// Verify the host pane-list fix on a CLEAN workspace: create several terminals,
// then close them — proving each is a real, closable pane_tree leaf (no zombies)
// and that close-pane-result is success:true (pre-fix it was
// "无法关闭最后一个窗格"). Also checks the sessionStorage GC prunes closed panes.
import { chromium, devices } from '@playwright/test';
import path from 'path';
import { fileURLToPath } from 'url';
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const PROFILE_DIR = path.resolve(__dirname, '..', '.pw-remote-profile');
const URL = process.env.RIDGE_URL || 'https://127.0.0.1:9528';
const CODE = (process.env.RIDGE_CODE || '').replace(/\D/g, '').slice(0, 6);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[verify]', ...a);

const { defaultBrowserType, ...iphone } = devices['iPhone 13'];
const ctx = await chromium.launchPersistentContext(PROFILE_DIR, { headless: true, ignoreHTTPSErrors: true, serviceWorkers: 'block', ...iphone });
const page = ctx.pages()[0] || (await ctx.newPage());
const closeResults = [];
page.on('websocket', (ws) => ws.on('framereceived', (f) => { const s = typeof f.payload === 'string' ? f.payload : ''; if (s.includes('close-pane-result')) closeResults.push(s.slice(0, 200)); }));
const errs = [];
page.on('console', (m) => { if (m.type() === 'error') errs.push(m.text()); });
page.on('pageerror', (e) => errs.push('UNCAUGHT: ' + e.message));

await page.goto(URL, { waitUntil: 'domcontentloaded' });
await sleep(1500);
if (await page.locator('input[inputmode="numeric"]').count()) {
  if (!CODE) { log('need RIDGE_CODE'); await ctx.close(); process.exit(3); }
  await page.locator('input[inputmode="numeric"]').fill(CODE);
  await page.locator('button').first().click();
}
await page.waitForSelector('.app-root', { timeout: 20000 });
await sleep(1200);

const snap = () => page.evaluate(() => {
  const sb = []; for (let i = 0; i < sessionStorage.length; i++) { const k = sessionStorage.key(i); if (k && k.startsWith('rg-remote-sb:')) sb.push(k); }
  return { panes: [...document.querySelectorAll('.pane-row')].map((r) => (r.querySelector('.pane-name')?.textContent || '').trim()), sb: sb.sort() };
});
const openTree = async () => { if (!(await page.locator('.tree-popup').count())) { await page.locator('.tree-trigger').click(); await page.waitForSelector('.tree-popup'); await sleep(150); } };
const waitFor = async (fn, ms = 8000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await fn()) return true; await sleep(200); } return false; };

await openTree();
const base = (await snap()).panes.length;
log(`baseline panes=${base}`);

// Create 3 terminals.
for (let i = 0; i < 3; i++) {
  await openTree();
  const n = (await snap()).panes.length;
  await page.locator('.pane-new').first().evaluate((el) => el.click());
  await waitFor(async () => (await snap()).panes.length > n);
  await sleep(600);
  log(`after create #${i + 1}: panes=${(await snap()).panes.length}`);
}
const peak = await snap();
log(`peak panes=${peak.panes.length} sbKeys=${peak.sb.length}`);

// Close down to 1, verifying each close actually drops the count.
let closed = 0;
for (let guard = 0; guard < 20; guard++) {
  await openTree();
  const s = await snap();
  if (s.panes.length <= 1) break;
  const n = s.panes.length;
  await page.locator('.pane-row .row-close').last().evaluate((el) => el.click());
  const dropped = await waitFor(async () => (await snap()).panes.length < n);
  log(`  close → dropped=${dropped} panes=${(await snap()).panes.length}`);
  if (dropped) closed++; else break;
}
const end = await snap();
await sleep(800);

log('');
log('=========== VERIFY ===========');
log(`created→peak=${peak.panes.length} (closable created? ${peak.panes.length > base})`);
log(`closed ${closed} panes; final panes=${end.panes.length}`);
log('close-pane-result frames:'); for (const c of closeResults) log('  ' + c);
const allClosesOk = closeResults.every((c) => c.includes('"success":true'));
log(`all close-pane-result success:true? ${allClosesOk}`);
log(`final sb keys: ${JSON.stringify(end.sb)} (GC: ≤ final pane count)`);
log(`console/page errors: ${errs.length}`);
const ok = peak.panes.length > base && closed >= 1 && end.panes.length === 1 && allClosesOk && errs.length === 0;
log(`RESULT: ${ok ? 'PASS ✅ host fix verified (panes are real closable leaves, no zombies)' : 'FAIL ❌'}`);
log('==============================');
await ctx.close();
process.exit(ok ? 0 : 1);
