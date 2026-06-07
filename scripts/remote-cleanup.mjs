// Close test terminals left in the active workspace by remote-gc-e2e.mjs,
// reducing it back toward its pre-test 1-terminal baseline. Conservative: only
// touches the ACTIVE workspace's panes (can't close the last one), and only
// closes extra WORKSPACES whose name matches the test pattern. Reuses the saved
// token in .pw-remote-profile (no pairing code needed).
import { chromium, devices } from '@playwright/test';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const PROFILE_DIR = path.resolve(__dirname, '..', '.pw-remote-profile');
const URL = process.env.RIDGE_URL || 'https://127.0.0.1:9528';
const KEEP_WS = process.env.RIDGE_KEEP_WS || '工作区 1'; // workspace to keep
const log = (...a) => console.log('[cleanup]', ...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const { defaultBrowserType, ...iphone } = devices['iPhone 13'];
const ctx = await chromium.launchPersistentContext(PROFILE_DIR, {
  headless: true, ignoreHTTPSErrors: true, serviceWorkers: 'block', ...iphone,
});
const page = ctx.pages()[0] || (await ctx.newPage());
await page.goto(URL, { waitUntil: 'domcontentloaded' });
await sleep(1500);
if (await page.locator('input[inputmode="numeric"]').count()) {
  log('saved token expired — re-run remote-gc-e2e with RIDGE_CODE first'); await ctx.close(); process.exit(3);
}
await page.waitForSelector('.app-root', { timeout: 20000 });
await sleep(1200);

const snap = () => page.evaluate(() => {
  const txt = (el, sel) => { const x = el.querySelector(sel); return x ? (x.textContent || '').trim() : ''; };
  return {
    open: !!document.querySelector('.tree-popup'),
    ws: [...document.querySelectorAll('.ws-row')].map((r) => ({ name: txt(r, '.ws-name'), active: r.classList.contains('active'), disabled: r.disabled === true, canClose: !!r.querySelector('.row-close') })),
    panes: [...document.querySelectorAll('.pane-row')].map((r) => ({ name: txt(r, '.pane-name'), active: r.classList.contains('active') })),
  };
});
const openTree = async () => { if (!(await page.locator('.tree-popup').count())) { await page.locator('.tree-trigger').click(); await page.waitForSelector('.tree-popup', { timeout: 5000 }); await sleep(150); } };
const domClick = async (loc) => { if (!(await loc.count())) return false; await loc.first().evaluate((el) => el.click()); return true; };

await openTree();
const before = await snap();
log('before: ws=' + JSON.stringify(before.ws.map((w) => w.name)) + ' activePanes=' + before.panes.length);

// 1) close test-leftover panes ("terminal"/"pending...") in the active workspace,
// keeping real ones. Detect which are genuinely unclosable (host bug: panes in
// the list that aren't pane_tree leaves — e.g. stuck pending_spawns — can't be
// closed) and skip them by index so we don't spin.
const KEEP = ['管理员', 'RIDGE_E2E_TITLE']; // substrings of real terminals to preserve
const unclosable = new Set();
let guard = 0;
while (guard++ < 60) {
  await openTree();
  const rows = await page.locator('.pane-row').evaluateAll((els) =>
    els.map((el) => ({ name: (el.querySelector('.pane-name')?.textContent || '').trim(), hasClose: !!el.querySelector('.row-close') })));
  const s = await snap();
  if (s.ws.some((w) => w.disabled)) { await sleep(300); continue; }
  // candidate = a closable, non-kept, not-known-unclosable row
  const idx = rows.findIndex((r, i) => r.hasClose && !KEEP.some((k) => r.name.includes(k)) && !unclosable.has(`${r.name}#${i}`));
  if (idx < 0) { log(`iter ${guard}: no more closable candidates (panes=${rows.length})`); break; }
  const name = rows[idx].name;
  const before = rows.length;
  await page.locator('.pane-row').nth(idx).locator('.row-close').first().evaluate((el) => el.click());
  let dropped = false;
  for (let i = 0; i < 18; i++) { await sleep(200); if ((await page.locator('.pane-row').count()) < before) { dropped = true; break; } }
  log(`close "${name}" (idx ${idx}) dropped=${dropped}`);
  if (!dropped) unclosable.add(`${name}#${idx}`); // host refused → skip next time
}
log('unclosable (host bug — not pane_tree leaves): ' + JSON.stringify([...unclosable]));
const mid = await snap();
log('active workspace panes now: ' + mid.panes.length);

// 2) close extra workspaces (keep KEEP_WS); only if more than one exists
guard = 0;
while (guard++ < 20) {
  await openTree();
  const s = await snap();
  const extra = s.ws.find((w) => w.name !== KEEP_WS && w.canClose);
  if (!extra || s.ws.length <= 1) break;
  if (s.ws.some((w) => w.disabled)) { await sleep(300); continue; }
  log('closing extra workspace: ' + extra.name);
  await domClick(page.locator('.ws-row', { hasText: extra.name }).locator('.row-close'));
  for (let i = 0; i < 20; i++) { await sleep(200); if (!(await snap()).ws.map((w) => w.name).includes(extra.name)) break; }
}

await openTree();
const after = await snap();
log('after: ws=' + JSON.stringify(after.ws.map((w) => w.name)) + ' activePanes=' + after.panes.length);
await ctx.close();
