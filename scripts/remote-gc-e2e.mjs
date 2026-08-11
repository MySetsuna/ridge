// §remote GC e2e — drive the mobile remote PWA (served with the fix on :5174,
// proxying WS to the live host on :9527) in an ISOLATED Playwright Chromium with
// mobile emulation. Never attaches to the other session's CDP (:9222).
//
// Proves the pane-cache GC: a closed pane's `rg-remote-sb:<id>` sessionStorage
// key must be pruned when the host re-broadcasts the `panes` list — instead of
// leaking forever (the root cause of "long-run → page won't open → clear data").
//
// Usage:
//   RIDGE_CODE=123456 RIDGE_PHASE=probe node scripts/remote-gc-e2e.mjs
//   RIDGE_PHASE=gc                       node scripts/remote-gc-e2e.mjs   (reuses saved token)
//
// A persistent profile under .pw-remote-profile keeps the session token, so only
// the FIRST run needs a pairing code (TOTP, ~60s validity).

import { chromium, devices } from '@playwright/test';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const PROFILE_DIR = path.resolve(__dirname, '..', '.pw-remote-profile');

// The live host serves HTTPS and UA-forks: a mobile UA gets the mobile PWA
// (remote-dist/mobile, where the fix lives). 9528 serves the freshly-built bundle.
const URL = process.env.RIDGE_URL || 'https://127.0.0.1:9528';
const CODE = (process.env.RIDGE_CODE || '').replace(/\D/g, '').slice(0, 6);
const PHASE = process.env.RIDGE_PHASE || 'probe';
const SB = 'rg-remote-sb:';

const log = (...a) => console.log('[gc-e2e]', ...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const sbKeys = (page) =>
  page.evaluate((p) => {
    const out = [];
    for (let i = 0; i < sessionStorage.length; i++) {
      const k = sessionStorage.key(i);
      if (k?.startsWith(p)) out.push(k);
    }
    return out.sort((a, b) => a.localeCompare(b));
  }, SB);

// Dump every clickable control's text/title/aria/class so we can find the
// create/close affordances without guessing.
const dumpControls = (page) =>
  page.evaluate(() => {
    const sel = 'button, [role="button"], [role="tab"]';
    return Array.from(document.querySelectorAll(sel)).map((el) => ({
      tag: el.tagName.toLowerCase(),
      text: (el.textContent || '').trim().slice(0, 30),
      title: el.getAttribute('title') || '',
      aria: el.getAttribute('aria-label') || '',
      cls: (el.getAttribute('class') || '').slice(0, 60),
    }));
  });

function createGcHarness(page, record) {
  const waitFor = async (fn, timeout = 9000, interval = 250) => {
    const started = Date.now();
    while (Date.now() - started < timeout) {
      try { if (await fn()) return true; } catch { /* retry */ }
      await sleep(interval);
    }
    return false;
  };
  const snap = () => page.evaluate(() => {
    const text = (el, selector) => {
      const child = el.querySelector(selector);
      return child ? (child.textContent || '').trim() : '';
    };
    const wsRows = [...document.querySelectorAll('.ws-row')].map((row) => ({
      name: text(row, '.ws-name'), active: row.classList.contains('active'),
      canClose: !!row.querySelector('.row-close'), disabled: row.disabled === true,
    }));
    const paneRows = [...document.querySelectorAll('.pane-row')].map((row) => ({
      name: text(row, '.pane-name'), active: row.classList.contains('active'), canClose: !!row.querySelector('.row-close'),
    }));
    const sb = [];
    for (let i = 0; i < sessionStorage.length; i += 1) {
      const key = sessionStorage.key(i);
      if (key?.startsWith('rg-remote-sb:')) sb.push(key);
    }
    const error = document.querySelector('.tree-err');
    return {
      treeOpen: !!document.querySelector('.tree-popup'), wsRows, paneRows,
      sb: [...sb].sort((a, b) => a.localeCompare(b)), err: error ? (error.textContent || '').trim() : '',
      hasCanvas: !!document.querySelector('canvas'),
    };
  });
  const openTree = async () => {
    if (await page.locator('.tree-popup').count()) return;
    await page.locator('.tree-trigger').click();
    await page.waitForSelector('.tree-popup', { timeout: 5000 });
    await sleep(150);
  };
  const domClick = async (locator, description) => {
    if (!(await locator.count())) { log(`  click ${description}: NOT FOUND`); return false; }
    const box = await locator.first().evaluate((el) => {
      const rect = el.getBoundingClientRect();
      return { top: Math.round(rect.top), bottom: Math.round(rect.bottom), left: Math.round(rect.left), right: Math.round(rect.right), vw: innerWidth, vh: innerHeight };
    });
    if (box.bottom < 0 || box.top > box.vh || box.right < 0 || box.left > box.vw) log(`  ⚠ ${description}: OFF-SCREEN box=${JSON.stringify(box)}`);
    await locator.first().evaluate((element) => element.click());
    return true;
  };
  return { page, waitFor, snap, openTree, domClick, record };
}

async function runPaneGc(harness, initial) {
  const { page, snap, openTree, domClick, waitFor, record } = harness;
  const beforeKeys = initial.sb;
  const paneCount = initial.paneRows.length;
  await openTree();
  await domClick(page.locator('.pane-new'), 'pane-new (create terminal)');
  const grew = await waitFor(async () => (await snap()).paneRows.length > paneCount);
  const created = await snap();
  record('T1 create-terminal: pane count +1', grew, `before=${paneCount} after=${created.paneRows.length}`);
  const keyAppeared = await waitFor(async () => (await snap()).sb.some((key) => !beforeKeys.includes(key)));
  const withKey = await snap();
  const newKey = withKey.sb.find((key) => !beforeKeys.includes(key)) || '';
  record('T1 create-terminal: new sb cache key written', keyAppeared, `newKey=${newKey || '(none)'}`);
  await openTree();
  await domClick(page.locator('.pane-row.active .row-close'), 'close active terminal')
    || await domClick(page.locator('.pane-row .row-close').last(), 'close last terminal (fallback)');
  const pruned = await waitFor(async () => !(await snap()).sb.includes(newKey));
  const shrank = await waitFor(async () => (await snap()).paneRows.length <= paneCount);
  const closedState = await snap();
  record('T2 close-terminal: pane count back', shrank, `now=${closedState.paneRows.length}`);
  record('T2 close-terminal: dead pane sb key PRUNED (GC fix)', pruned && !!newKey,
    `newKey=${newKey || '(none)'} stillPresent=${closedState.sb.includes(newKey)} sbNow=${JSON.stringify(closedState.sb)}`);
  if (closedState.err) record('T2 close-terminal: no UI error', false, `tree-err=${closedState.err}`);
}

async function runWorkspaceGc(harness, initial) {
  const { page, snap, openTree, domClick, waitFor, record } = harness;
  const originalNames = new Set(initial.wsRows.map((workspace) => workspace.name));
  const originalActive = initial.wsRows.find((workspace) => workspace.active)?.name || '';
  await openTree();
  const wsCount = (await snap()).wsRows.length;
  const addBox = await page.locator('.tree-add').first().evaluate((element) => {
    const rect = element.getBoundingClientRect();
    return { left: Math.round(rect.left), right: Math.round(rect.right), top: Math.round(rect.top), bottom: Math.round(rect.bottom), vw: innerWidth, vh: innerHeight };
  });
  const addOnScreen = addBox.left >= 0 && addBox.right <= addBox.vw && addBox.top >= 0 && addBox.bottom <= addBox.vh;
  record('T3 新建工作区(+) button fully on-screen (finger-reachable)', addOnScreen, JSON.stringify(addBox));
  await domClick(page.locator('.tree-add'), 'tree-add (new workspace)');
  const wsGrew = await waitFor(async () => (await snap()).wsRows.length > wsCount);
  await waitFor(async () => (await snap()).paneRows.length >= 1);
  const created = await snap();
  const newName = created.wsRows.map((workspace) => workspace.name).find((name) => !originalNames.has(name)) || '';
  record('T3 create-workspace: ws count +1', wsGrew, `before=${wsCount} after=${created.wsRows.length} new=${newName}`);
  record('T3 create-workspace: new ws is active', created.wsRows.find((workspace) => workspace.active)?.name === newName && !!newName, `activeWs=${created.wsRows.find((workspace) => workspace.active)?.name}`);
  record('T3 create-workspace: new ws has a terminal', created.paneRows.length >= 1, `panes=${created.paneRows.length}`);
  record('T3 create-workspace: canvas renders', created.hasCanvas, '');
  if (created.err) record('T3 create-workspace: no UI error', false, `tree-err=${created.err}`);
  await openTree();
  const beforeSwitch = await snap();
  const wasBusy = beforeSwitch.wsRows.some((workspace) => workspace.disabled);
  const enabled = await waitFor(async () => !(await snap()).wsRows.some((workspace) => workspace.disabled), 8000);
  log(`T4 rows-disabled-at-open=${wasBusy} became-enabled=${enabled}`);
  await openTree();
  await domClick(page.locator('.ws-row', { hasText: originalActive }), `switch to ws "${originalActive}"`)
    || await domClick(page.locator('.ws-row').first(), 'switch to first ws (fallback)');
  const switched = await waitFor(async () => (await snap()).wsRows.find((workspace) => workspace.active)?.name === originalActive, 7000);
  await waitFor(async () => (await snap()).hasCanvas);
  const switchedState = await snap();
  record('T4 switch-workspace: busy swallows taps (no feedback)', !wasBusy, `rows disabled immediately after create-workspace = ${wasBusy}`);
  record('T4 switch-workspace: active ws is original', switched, `activeWs=${switchedState.wsRows.find((workspace) => workspace.active)?.name}`);
  record('T4 switch-workspace: panes present + canvas', switchedState.paneRows.length >= 1 && switchedState.hasCanvas, `panes=${switchedState.paneRows.length} canvas=${switchedState.hasCanvas}`);
  if (switchedState.err) record('T4 switch-workspace: no UI error', false, `tree-err=${switchedState.err}`);
  await openTree();
  const closed = newName
    ? await domClick(page.locator('.ws-row', { hasText: newName }).locator('.row-close'), `close ws "${newName}"`)
    : false;
  const removed = await waitFor(async () => !(await snap()).wsRows.map((workspace) => workspace.name).includes(newName));
  const closedState = await snap();
  record('T5 close-workspace: created ws removed', closed && removed, `clicked=${closed} wsNow=${JSON.stringify(closedState.wsRows.map((workspace) => workspace.name))}`);
  record('T5 close-workspace: active ws still valid + canvas', closedState.wsRows.some((workspace) => workspace.active) && closedState.hasCanvas, `activeWs=${closedState.wsRows.find((workspace) => workspace.active)?.name} canvas=${closedState.hasCanvas}`);
  if (closedState.err) record('T5 close-workspace: no UI error', false, `tree-err=${closedState.err}`);
}

async function runGcPhase(page, ctx, consoleErrors) {
  const results = [];
  const record = (name, ok, detail) => {
    results.push({ name, ok, detail });
    const suffix = detail ? ` - ${detail}` : '';
    log(`${ok ? 'PASS' : 'FAIL'}  ${name}${suffix}`);
  };
  const harness = createGcHarness(page, record);
  await harness.openTree();
  const initial = await harness.snap();
  log(`T0 initial: ws=${JSON.stringify(initial.wsRows.map((workspace) => workspace.name))} activeWs=${initial.wsRows.find((workspace) => workspace.active)?.name || ''} panes=${initial.paneRows.length} sb=${JSON.stringify(initial.sb)}`);
  await page.screenshot({ path: path.resolve(__dirname, '..', 'gc-e2e-connected.png') });
  await runPaneGc(harness, initial);
  await runWorkspaceGc(harness, initial);
  await page.screenshot({ path: path.resolve(__dirname, '..', 'gc-e2e-final.png') });
  const failed = results.filter((result) => !result.ok);
  log('');
  log('================ SUMMARY ================');
  log(`steps: ${results.length}  passed: ${results.length - failed.length}  failed: ${failed.length}`);
  if (failed.length) for (const failure of failed) {
    const suffix = failure.detail ? ` - ${failure.detail}` : '';
    log(`  X ${failure.name}${suffix}`);
  }
  if (consoleErrors.length) {
    log(`console/page errors during run: ${consoleErrors.length}`);
    for (const error of [...new Set(consoleErrors)].slice(0, 20)) log(`  ! ${error}`);
  } else log('no console/page errors');
  log('=========================================');
  await ctx.close();
  process.exit(failed.length ? 1 : 0);
}

async function main() {
  const { defaultBrowserType, ...iphone } = devices['iPhone 13'];
  log('launching isolated Chromium (mobile), profile:', PROFILE_DIR);
  const ctx = await chromium.launchPersistentContext(PROFILE_DIR, {
    headless: true,
    ignoreHTTPSErrors: true, // host uses a self-signed LAN cert
    serviceWorkers: 'block', // deterministic iteration; GC path is SW-independent
    ...iphone,
  });
  const page = ctx.pages()[0] || (await ctx.newPage());
  const consoleErrors = [];
  page.on('console', (m) => {
    if (m.type() === 'error') { consoleErrors.push(m.text()); log('  page-error:', m.text()); }
  });
  page.on('pageerror', (e) => { consoleErrors.push('UNCAUGHT: ' + e.message); log('  pageerror:', e.message); });

  log('navigating', URL);
  await page.goto(URL, { waitUntil: 'domcontentloaded' });

  // Auth: the app auto-reconnects with a saved token onMount. If none, the code
  // input (showManual) appears — fill it.
  await sleep(1500);
  const needCode = (await page.locator('input[inputmode="numeric"]').count()) > 0;
  if (needCode) {
    if (!CODE) {
      log('NO saved token and no RIDGE_CODE provided — set RIDGE_CODE=<6 digits>.');
      await page.screenshot({ path: path.resolve(__dirname, '..', 'gc-e2e-auth.png') });
      await ctx.close();
      process.exit(3);
    }
    log('entering pairing code');
    await page.locator('input[inputmode="numeric"]').fill(CODE);
    await page.locator('button').first().click();
  } else {
    log('reusing saved session token (no code needed)');
  }

  // MainApp root is <div class="app-root">.
  await page.waitForSelector('.app-root', { timeout: 20000 });
  log('connected — MainApp loaded');
  await sleep(1200);

  const initial = await sbKeys(page);
  log('initial sessionStorage sb keys:', JSON.stringify(initial));
  await page.screenshot({ path: path.resolve(__dirname, '..', 'gc-e2e-connected.png') });

  if (PHASE === 'probe') {
    const controls = await dumpControls(page);
    log('controls on screen:');
    for (const c of controls) console.log('   ', JSON.stringify(c));
    log('probe done — screenshots: gc-e2e-connected.png');
    await ctx.close();
    return;
  }

  if (PHASE === 'gc') {
    await runGcPhase(page, ctx, consoleErrors);
  }
}

try {
  await main();
} catch (e) {
  console.error('[gc-e2e] FATAL', e);
  process.exit(1);
}
