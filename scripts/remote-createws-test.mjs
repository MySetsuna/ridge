// Isolated create-workspace test: does create-workspace orphan the PREVIOUS
// workspace at the SOURCE, or was the earlier prev-ws orphan the other session's
// concurrent desktop on the shared workspace? switch-workspace is per-connection,
// so a workspace this mobile client creates is NOT viewed by the other session.
import { chromium, devices } from '@playwright/test';
import path from 'path';
import { fileURLToPath } from 'url';
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const PROFILE_DIR = process.env.RIDGE_PROFILE_DIR || path.resolve(__dirname, '..', '.pw-remote-profile');
const URL = process.env.RIDGE_URL || 'https://127.0.0.1:9527';
const CODE = (process.env.RIDGE_CODE || '').replace(/\D/g, '').slice(0, 6);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[cws]', ...a);

const { defaultBrowserType, ...iphone } = devices['iPhone 13'];
const ctx = await chromium.launchPersistentContext(PROFILE_DIR, { headless: true, ignoreHTTPSErrors: true, serviceWorkers: 'block', ...iphone });
const page = ctx.pages()[0] || (await ctx.newPage());
await page.goto(URL, { waitUntil: 'domcontentloaded' });
await sleep(1500);
if (await page.locator('input[inputmode="numeric"]').count()) { if (!CODE) { log('need RIDGE_CODE'); process.exit(3); } await page.locator('input[inputmode="numeric"]').fill(CODE); await page.locator('button').first().click(); }
await page.waitForSelector('.app-root', { timeout: 20000 });
await page.waitForSelector('.tree-trigger', { state: 'visible', timeout: 20000 });
await sleep(2500);
const openTree = async () => {
  const popup = page.locator('.tree-popup');
  if (!(await popup.isVisible().catch(() => false))) {
    await page.locator('.tree-trigger').click();
    await popup.waitFor({ state: 'visible' });
    await sleep(150);
  }
};
const uiSnapshot = async () => {
  await openTree();
  return {
    workspaceCount: await page.locator('.ws-row').count(),
    activeIndex: await page.locator('.ws-row').evaluateAll((rows) => rows.findIndex((row) => row.classList.contains('active'))),
    paneCount: await page.locator('.pane-row').count(),
  };
};
const newWs = async () => {
  await openTree();
  const before = await uiSnapshot();
  // The first action opens saved workspaces; the second creates a workspace.
  const add = page.locator('.tree-add:not([data-testid="tree-open-saved"])');
  await add.waitFor({ state: 'visible' });
  await add.click();
  for (let k = 0; k < 40 && (await page.locator('.ws-row').count()) <= before.workspaceCount; k++) await sleep(200);
  const after = await uiSnapshot();
  if (after.workspaceCount <= before.workspaceCount) throw new Error('create-workspace did not add a tree row');
  if (after.activeIndex < 0) throw new Error('create-workspace did not expose an active workspace row');
  await sleep(800);
  return { index: after.activeIndex, before, after };
};
const newPane = async () => {
  const before = await uiSnapshot();
  if (!(await page.locator('.pane-new').count())) {
    await page.locator('.ws-row.active .ws-chev').first().click();
    await page.locator('.pane-new').first().waitFor({ state: 'visible' });
  }
  await page.locator('.pane-new').first().click();
  for (let k = 0; k < 25 && (await page.locator('.pane-row').count()) <= before.paneCount; k++) await sleep(200);
  const after = await uiSnapshot();
  if (after.paneCount <= before.paneCount) throw new Error('create-pane did not add a pane row');
  await sleep(800);
  return { before, after };
};

// 1) create WS_A (isolated browser session) — switches this client to it.
const a = await newWs();
log(`WS_A row=${a.index} after create: ${JSON.stringify(a.after)}`);
// 2) add a pane in WS_A.
const aPane = await newPane();
log(`WS_A after +pane: ${JSON.stringify(aPane.after)}`);
// 3) create WS_B, then switch back to WS_A and assert its pane remains.
const b = await newWs();
log(`WS_B row=${b.index} after create: ${JSON.stringify(b.after)}`);
await page.locator('.ws-row').nth(a.index).click();
await sleep(1200);
const aReturned = await uiSnapshot();
const orphaned = aReturned.paneCount < aPane.after.paneCount;
log(`WS_A after switch-back: ${JSON.stringify(aReturned)}`);
log(`RESULT: create-workspace ${orphaned ? 'DID ❌ lose the previous workspace pane' : 'did NOT ✅ lose the previous workspace pane'}`);

// cleanup: close WS_A and WS_B (best effort)
try {
  await openTree();
} catch {}
await ctx.close();
process.exit(orphaned ? 1 : 0);
