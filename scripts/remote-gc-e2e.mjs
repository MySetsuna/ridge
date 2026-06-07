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
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const PROFILE_DIR = path.resolve(__dirname, '..', '.pw-remote-profile');

// The live host serves HTTPS and UA-forks: a mobile UA gets the mobile PWA
// (static/remote, where the fix lives). 9528 serves the freshly-built bundle.
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
      if (k && k.startsWith(p)) out.push(k);
    }
    return out.sort();
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
    const results = [];
    const record = (name, ok, detail) => {
      results.push({ name, ok, detail });
      log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? ' — ' + detail : ''}`);
    };
    const waitFor = async (fn, timeout = 9000, interval = 250) => {
      const t0 = Date.now();
      while (Date.now() - t0 < timeout) { try { if (await fn()) return true; } catch { /* retry */ } await sleep(interval); }
      return false;
    };
    // Structural snapshot. ws/pane rows only exist while the tree popup is open.
    const snap = () => page.evaluate(() => {
      const txt = (el, sel) => { const x = el.querySelector(sel); return x ? (x.textContent || '').trim() : ''; };
      const wsRows = [...document.querySelectorAll('.ws-row')].map((r) => ({
        name: txt(r, '.ws-name'), active: r.classList.contains('active'),
        canClose: !!r.querySelector('.row-close'), disabled: r.disabled === true,
      }));
      const paneRows = [...document.querySelectorAll('.pane-row')].map((r) => ({
        name: txt(r, '.pane-name'), active: r.classList.contains('active'), canClose: !!r.querySelector('.row-close'),
      }));
      const sb = [];
      for (let i = 0; i < sessionStorage.length; i++) { const k = sessionStorage.key(i); if (k && k.startsWith('rg-remote-sb:')) sb.push(k); }
      const errEl = document.querySelector('.tree-err');
      return {
        treeOpen: !!document.querySelector('.tree-popup'),
        wsRows, paneRows, sb: sb.sort(),
        err: errEl ? (errEl.textContent || '').trim() : '',
        hasCanvas: !!document.querySelector('canvas'),
      };
    });
    const openTree = async () => {
      if (await page.locator('.tree-popup').count()) return;
      await page.locator('.tree-trigger').click();
      await page.waitForSelector('.tree-popup', { timeout: 5000 });
      await sleep(150);
    };
    // Click via DOM (el.click()) to bypass Playwright viewport actionability —
    // and MEASURE the element's on-screen box first so a genuinely off-screen
    // control (a real mobile UX bug) is surfaced instead of silently worked around.
    const domClick = async (loc, desc) => {
      if (!(await loc.count())) { log(`  click ${desc}: NOT FOUND`); return false; }
      const box = await loc.first().evaluate((el) => {
        const r = el.getBoundingClientRect();
        return { top: Math.round(r.top), bottom: Math.round(r.bottom), left: Math.round(r.left), right: Math.round(r.right), vw: innerWidth, vh: innerHeight };
      });
      if (box.bottom < 0 || box.top > box.vh || box.right < 0 || box.left > box.vw) {
        log(`  ⚠ ${desc}: OFF-SCREEN box=${JSON.stringify(box)}`);
      }
      await loc.first().evaluate((el) => el.click());
      return true;
    };

    await openTree();
    const s0 = await snap();
    const origWsNames = s0.wsRows.map((w) => w.name);
    const origActiveWs = s0.wsRows.find((w) => w.active)?.name || '';
    log('T0 initial: ws=' + JSON.stringify(origWsNames) + ' activeWs=' + origActiveWs +
        ' panes=' + s0.paneRows.length + ' sb=' + JSON.stringify(s0.sb));

    // ── T1: create terminal → its sessionStorage scrollback key must appear ──
    const sbBefore = s0.sb;
    const paneCount0 = s0.paneRows.length;
    await openTree();
    await domClick(page.locator('.pane-new'), 'pane-new (create terminal)');
    const grew = await waitFor(async () => (await snap()).paneRows.length > paneCount0);
    const t1 = await snap();
    record('T1 create-terminal: pane count +1', grew, `before=${paneCount0} after=${t1.paneRows.length}`);
    const keyAppeared = await waitFor(async () => (await snap()).sb.some((k) => !sbBefore.includes(k)));
    const t1b = await snap();
    const newKey = t1b.sb.find((k) => !sbBefore.includes(k)) || '';
    record('T1 create-terminal: new sb cache key written', keyAppeared, `newKey=${newKey || '(none)'}`);

    // ── T2: close that terminal → its sb key must be PRUNED (THE FIX) ──
    await openTree();
    const closedClicked = await domClick(page.locator('.pane-row.active .row-close'), 'close active terminal');
    if (!closedClicked) {
      await domClick(page.locator('.pane-row .row-close').last(), 'close last terminal (fallback)');
    }
    const pruned = await waitFor(async () => !(await snap()).sb.includes(newKey));
    const shrank = await waitFor(async () => (await snap()).paneRows.length <= paneCount0);
    const t2 = await snap();
    record('T2 close-terminal: pane count back', shrank, `now=${t2.paneRows.length}`);
    record('T2 close-terminal: dead pane sb key PRUNED (GC fix)', pruned && !!newKey,
      `newKey=${newKey || '(none)'} stillPresent=${t2.sb.includes(newKey)} sbNow=${JSON.stringify(t2.sb)}`);
    if (t2.err) record('T2 close-terminal: no UI error', false, 'tree-err=' + t2.err);

    // ── T3: create workspace (auto-switch + auto-create one pane) ──
    await openTree();
    const wsCount0 = (await snap()).wsRows.length;
    const addBox = await page.locator('.tree-add').first().evaluate((el) => {
      const r = el.getBoundingClientRect();
      return { left: Math.round(r.left), right: Math.round(r.right), top: Math.round(r.top), bottom: Math.round(r.bottom), vw: innerWidth, vh: innerHeight };
    });
    const addOnScreen = addBox.left >= 0 && addBox.right <= addBox.vw && addBox.top >= 0 && addBox.bottom <= addBox.vh;
    record('T3 新建工作区(+) button fully on-screen (finger-reachable)', addOnScreen, JSON.stringify(addBox));
    await domClick(page.locator('.tree-add'), 'tree-add (new workspace)');
    const wsGrew = await waitFor(async () => (await snap()).wsRows.length > wsCount0);
    await waitFor(async () => (await snap()).paneRows.length >= 1);
    const t3 = await snap();
    const newWsName = t3.wsRows.map((w) => w.name).find((n) => !origWsNames.includes(n)) || '';
    const newWsActive = t3.wsRows.find((w) => w.active)?.name === newWsName && !!newWsName;
    record('T3 create-workspace: ws count +1', wsGrew, `before=${wsCount0} after=${t3.wsRows.length} new=${newWsName}`);
    record('T3 create-workspace: new ws is active', newWsActive, `activeWs=${t3.wsRows.find((w) => w.active)?.name}`);
    record('T3 create-workspace: new ws has a terminal', t3.paneRows.length >= 1, `panes=${t3.paneRows.length}`);
    record('T3 create-workspace: canvas renders', t3.hasCanvas, '');
    if (t3.err) record('T3 create-workspace: no UI error', false, 'tree-err=' + t3.err);

    // ── T4: switch back to the original workspace ──
    await openTree();
    const preT4 = await snap();
    log('T4 pre-click ws rows: ' + JSON.stringify(preT4.wsRows));
    // BUG CHARACTERIZATION: if the rows are `disabled` (busy from T3's create
    // chain), a tap is silently lost. Distinguish "transient busy + lost tap"
    // (UX bug) from "switch is broken" (logic bug): wait until enabled, click,
    // and also report whether a naive immediate tap would have been swallowed.
    const wasBusyAtReady = preT4.wsRows.some((w) => w.disabled);
    const enabled = await waitFor(async () => !(await snap()).wsRows.some((w) => w.disabled), 8000);
    log(`T4 rows-disabled-at-open=${wasBusyAtReady} became-enabled=${enabled}`);
    await openTree();
    if (!(await domClick(page.locator('.ws-row', { hasText: origActiveWs }), `switch to ws "${origActiveWs}"`))) {
      await domClick(page.locator('.ws-row').first(), 'switch to first ws (fallback)');
    }
    for (let i = 0; i < 12; i++) {
      const s = await snap();
      log(`  T4 +${(i * 0.4).toFixed(1)}s active=${s.wsRows.find((w) => w.active)?.name} panes=${s.paneRows.length} disabled=${s.wsRows.some((w) => w.disabled)} err="${s.err}"`);
      if (s.wsRows.find((w) => w.active)?.name === origActiveWs) break;
      await sleep(400);
    }
    const switched = await waitFor(async () => (await snap()).wsRows.find((w) => w.active)?.name === origActiveWs, 2000);
    await waitFor(async () => (await snap()).hasCanvas);
    const t4 = await snap();
    record('T4 switch-workspace: busy swallows taps (no feedback)', !wasBusyAtReady,
      `rows were disabled immediately after create-workspace = ${wasBusyAtReady}`);
    record('T4 switch-workspace: active ws is original', switched, `activeWs=${t4.wsRows.find((w) => w.active)?.name}`);
    record('T4 switch-workspace: panes present + canvas', t4.paneRows.length >= 1 && t4.hasCanvas,
      `panes=${t4.paneRows.length} canvas=${t4.hasCanvas}`);
    if (t4.err) record('T4 switch-workspace: no UI error', false, 'tree-err=' + t4.err);

    // ── T5: close the workspace we created ──
    await openTree();
    let closedWs = false;
    if (newWsName) {
      closedWs = await domClick(page.locator('.ws-row', { hasText: newWsName }).locator('.row-close'), `close ws "${newWsName}"`);
    }
    const wsBack = await waitFor(async () => !(await snap()).wsRows.map((w) => w.name).includes(newWsName));
    const t5 = await snap();
    record('T5 close-workspace: created ws removed', closedWs && wsBack,
      `clicked=${closedWs} wsNow=${JSON.stringify(t5.wsRows.map((w) => w.name))}`);
    record('T5 close-workspace: active ws still valid + canvas', !!t5.wsRows.find((w) => w.active) && t5.hasCanvas,
      `activeWs=${t5.wsRows.find((w) => w.active)?.name} canvas=${t5.hasCanvas}`);
    if (t5.err) record('T5 close-workspace: no UI error', false, 'tree-err=' + t5.err);

    await page.screenshot({ path: path.resolve(__dirname, '..', 'gc-e2e-final.png') });

    // ── summary ──
    const failed = results.filter((r) => !r.ok);
    log('');
    log('================ SUMMARY ================');
    log(`steps: ${results.length}  passed: ${results.length - failed.length}  failed: ${failed.length}`);
    if (failed.length) { log('FAILURES:'); for (const f of failed) log('  ✗ ' + f.name + (f.detail ? ' — ' + f.detail : '')); }
    if (consoleErrors.length) {
      log(`console/page errors during run: ${consoleErrors.length}`);
      for (const e of [...new Set(consoleErrors)].slice(0, 20)) log('  ! ' + e);
    } else { log('no console/page errors'); }
    log('=========================================');

    await ctx.close();
    process.exit(failed.length ? 1 : 0);
  }
}

main().catch((e) => {
  console.error('[gc-e2e] FATAL', e);
  process.exit(1);
});
