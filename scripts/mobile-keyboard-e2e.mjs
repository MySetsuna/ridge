#!/usr/bin/env node
/**
 * Deterministic mobile Remote keyboard probe.
 *
 * This is a browser-emulation probe, not a physical-device claim: Chromium's
 * mobile context resizes the visual viewport while the real LAN Remote host
 * and PTY/WebSocket path remain live. It verifies focus, bounded visual-only
 * translation, jitter convergence, keyboard-close recovery, and Console
 * cleanliness in a fresh context with extensions disabled.
 */

import { spawn } from 'node:child_process';
import { randomInt } from 'node:crypto';
import { createServer } from 'node:net';
import {
  existsSync,
  mkdirSync,
  readFileSync,
  unlinkSync,
  writeFileSync,
} from 'node:fs';
import { resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as sleep } from 'node:timers/promises';

import { chromium } from '@playwright/test';
import { systemTool } from './lib/toolPath.mjs';

const ROOT = resolve(fileURLToPath(new URL('..', import.meta.url)));
// Do not share the generic status file with another LAN probe.  A parallel
// host can otherwise replace the URL between readiness polling and navigation.
const STATUS_FILE = resolve(
  process.env.RIDGE_LAN_STATUS_FILE ||
    join(ROOT, '.ridge', `lan-host-status-mobile-e2e-${process.pid}.json`),
);
const EVIDENCE_FILE = resolve(
  process.env.RIDGE_KEYBOARD_EVIDENCE ||
    join(ROOT, '.iteration', 'artifacts', 'rdg-remote-e2e', 'mobile-keyboard.json'),
);
const RDG = process.env.RIDGE_RDG || join(ROOT, 'target', 'debug', 'rdg.exe');
const PORT_MIN = 28_000;
const PORT_MAX = 29_499;

function readStatus() {
  try {
    return JSON.parse(readFileSync(STATUS_FILE, 'utf8'));
  } catch {
    return null;
  }
}

async function findPort() {
  const width = PORT_MAX - PORT_MIN + 1;
  const start = randomInt(width);
  for (let i = 0; i < width; i += 1) {
    const port = PORT_MIN + ((start + i) % width);
    const available = await new Promise((resolveAvailable) => {
      const server = createServer();
      server.once('error', () => resolveAvailable(false));
      server.listen(port, '127.0.0.1', () => server.close(() => resolveAvailable(true)));
    });
    if (available) return port;
  }
  throw new Error('no browser-safe port available');
}

async function waitReady(timeoutMs = 45_000, expectedPort = 0, expectedPid = 0) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const status = readStatus();
    if (
      status?.ready &&
      status.port &&
      status.url_loopback &&
      status.totp &&
      (!expectedPort || Number(status.port) === expectedPort) &&
      (!expectedPid || Number(status.pid) === expectedPid)
    ) {
      return status;
    }
    await sleep(200);
  }
  throw new Error('timed out waiting for LAN host readiness');
}

function killTree(pid) {
  if (!pid) return Promise.resolve();
  return new Promise((resolveKill) => {
    const child = spawn(systemTool('taskkill'), ['/PID', String(pid), '/T', '/F'], {
      stdio: 'ignore',
      windowsHide: true,
    });
    const timer = setTimeout(() => resolveKill(), 5_000);
    child.once('exit', () => {
      clearTimeout(timer);
      resolveKill();
    });
    child.once('error', () => {
      clearTimeout(timer);
      resolveKill();
    });
  });
}

async function metric(page) {
  return page.evaluate(() => {
    const vv = window.visualViewport;
    const stage = document.querySelector('.term-stage');
    const input = document.querySelector('.hidden-input');
    const stageStyle = stage ? getComputedStyle(stage).transform : 'none';
    let shiftY = 0;
    try {
      shiftY = stageStyle && stageStyle !== 'none' ? new DOMMatrix(stageStyle).m42 : 0;
    } catch {
      shiftY = 0;
    }
    const inputRect = input?.getBoundingClientRect();
    return {
      innerHeight: window.innerHeight,
      visualHeight: vv?.height ?? null,
      visualOffsetTop: vv?.offsetTop ?? null,
      keyboardTop: vv ? vv.offsetTop + vv.height : null,
      shiftY,
      focused: document.activeElement === input,
      inputTop: inputRect?.top ?? null,
      inputBottom: inputRect?.bottom ?? null,
      geometry: window.__RIDGE_TERMINAL_GEOMETRY?.() ?? null,
    };
  });
}

async function authenticate(page, status) {
  const input = page.locator('input[inputmode="numeric"]').first();
  if (!(await input.isVisible().catch(() => false))) return;
  for (let attempt = 0; attempt < 3; attempt += 1) {
    const current = readStatus() || status;
    await input.fill(String(current.totp));
    const button = page
      .locator('button')
      .filter({ hasText: /Connect|连接|验证|Verify|继续/i })
      .first();
    if (await button.count()) await button.click();
    else await input.press('Enter');
    await page.waitForTimeout(1_000);
    if (!(await input.isVisible().catch(() => false))) return;
    if (attempt < 2) await sleep(1_500);
  }
  throw new Error('Remote remained on authentication gate');
}

async function main() {
  if (process.platform !== 'win32') throw new Error('probe currently requires Windows taskkill cleanup');
  if (!existsSync(RDG)) throw new Error(`rdg binary missing: ${RDG}`);
  mkdirSync(resolve(EVIDENCE_FILE, '..'), { recursive: true });
  try {
    unlinkSync(STATUS_FILE);
  } catch {
    /* stale status is harmless */
  }

  const port = await findPort();
  const env = {
    ...process.env,
    RIDGE_REMOTE_UI_ROOT: join(ROOT, 'remote-dist'),
    RIDGE_LAN_STATUS_FILE: STATUS_FILE,
    HTTP_PROXY: '',
    HTTPS_PROXY: '',
    ALL_PROXY: '',
    http_proxy: '',
    https_proxy: '',
    all_proxy: '',
    NO_PROXY: '*',
    no_proxy: '*',
    RUST_LOG: process.env.RUST_LOG || 'ridge_cli=info,ridge_remote=info',
  };
  const host = spawn(RDG, ['tui', '--lan-host', '--port', String(port)], {
    cwd: ROOT,
    env,
    stdio: 'ignore',
    detached: true,
    windowsHide: true,
  });
  host.unref();
  let browser;
  let context;
  const browserErrors = [];
  let result;
  try {
    const status = await waitReady(45_000, port, host.pid);
    const url = String(status.url_loopback).replace(/\/$/, '');
    browser = await chromium.launch({
      headless: true,
      args: ['--ignore-certificate-errors', '--no-proxy-server', '--disable-extensions'],
    });
    context = await browser.newContext({
      ignoreHTTPSErrors: true,
      userAgent:
        'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 '
        + '(KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1',
      viewport: { width: 390, height: 844 },
      isMobile: true,
      hasTouch: true,
    });
    await context.addInitScript(() => {
      localStorage.setItem('RIDGE_DIAG', '1');
    });
    const page = await context.newPage();
    page.on('pageerror', (error) => browserErrors.push(`pageerror:${error.message}`));
    page.on('console', (message) => {
      if (message.type() === 'error') browserErrors.push(`console:${message.text()}`);
    });

    const response = await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 30_000 });
    if ((response?.status() || 0) !== 200) throw new Error(`Remote HTTP ${response?.status()}`);
    await authenticate(page, status);
    await page.waitForSelector('.app-root', { timeout: 20_000 });
    await page.waitForSelector('canvas', { timeout: 20_000 });
    await page.locator('.hidden-input').waitFor({ state: 'attached', timeout: 5_000 });
    // Produce harmless shell output so the shared terminal manager has a
    // concrete cursor anchor near the lower viewport (an idle fresh PTY may
    // legitimately have none, which would correctly require no translation).
    await page.keyboard.type("1..40 | ForEach-Object { 'keyboard-probe' }");
    await page.keyboard.press('Enter');
    await page.waitForTimeout(900);
    // Re-anchor the IME sink after output has moved the cursor, matching the
    // user path (open the system keyboard only after the active prompt exists).
    await page.locator('[data-testid="system-ime-button"]').click();
    await page.waitForTimeout(500);
    await page.screenshot({
      path: resolve(EVIDENCE_FILE, '..', 'mobile-keyboard-before-resize.png'),
      fullPage: true,
    }).catch(() => {});

    const layoutHeight = await page.evaluate(() => window.innerHeight);
    const resizeViewport = async (height, settleMs = 220) => {
      // A real mobile IME normally keeps the layout viewport stable while the
      // VisualViewport shrinks. Override only those read-only measurements in
      // this test context; changing Playwright's viewport would shrink both
      // values and correctly look like an orientation/layout change instead.
      await page.evaluate(({ nextHeight, stableLayoutHeight }) => {
        const vv = window.visualViewport;
        Object.defineProperty(window, 'innerHeight', {
          configurable: true,
          value: stableLayoutHeight,
        });
        if (vv) {
          Object.defineProperty(vv, 'height', {
            configurable: true,
            value: nextHeight,
          });
          vv.dispatchEvent(new Event('resize'));
        }
      }, { nextHeight: height, stableLayoutHeight: layoutHeight });
      // The first event can race the terminal's cursor/IME-anchor repaint.
      // A second browser-equivalent notification makes the probe assert the
      // settled state, not a transient frame between PTY output and layout.
      await page.waitForTimeout(80);
      await page.evaluate(() => window.visualViewport?.dispatchEvent(new Event('resize')));
      await page.waitForTimeout(settleMs);
    };

    const baseline = await metric(page);
    // Use a deliberately small visual viewport so the synthetic IME top
    // intersects the current cursor cell; real phones reach the same path
    // when their keyboard consumes most of the viewport.
    await resizeViewport(400, 800);
    await page.waitForTimeout(800);
    const reduced = await metric(page);
    const jitter = [];
    for (const height of [392, 408, 396, 400]) {
      await resizeViewport(height, 180);
      jitter.push(await metric(page));
    }
    await resizeViewport(844, 900);
    await page.waitForTimeout(900);
    const restored = await metric(page);

    // Touch-selection smoke: exercise the same mobile touch handlers that
    // previously drifted from the finger after a visual-stage transform.
    // The geometry unit test proves the exact row/column math; this browser
    // assertion proves the real touch path reaches the copy affordance.
    const selectionTouch = await (async () => {
      const toggle = page.locator('.group-left .ctrl-btn').first();
      if (!(await toggle.count())) return { ok: false, reason: 'selection toggle not found' };
      await toggle.click();
      await page.keyboard.type("Write-Output SEL_A; Write-Output SEL_B; Write-Output SEL_C");
      await page.keyboard.press('Enter');
      await page.waitForTimeout(900);
      const sample = await metric(page);
      const pane = sample.geometry?.[0];
      if (!pane?.canvas || !pane.cell || !pane.kernel) {
        return { ok: false, reason: 'terminal geometry unavailable', sample };
      }
      const row = Math.max(0, pane.kernel.rows - 3);
      const endRow = Math.min(pane.kernel.rows - 1, row + 2);
      const start = {
        x: pane.canvas.x + pane.cell.width * 2.5,
        y: pane.canvas.y + pane.cell.height * (row + 0.5),
      };
      const end = {
        x: pane.canvas.x + pane.cell.width * 2.5,
        y: pane.canvas.y + pane.cell.height * (endRow + 0.5),
      };
      await page.evaluate(({ start: a, end: b }) => {
        const el = document.querySelector('.term-stage .container');
        if (!el) throw new Error('terminal container unavailable');
        const make = (p) => new Touch({
          identifier: 7,
          target: el,
          clientX: p.x,
          clientY: p.y,
          screenX: p.x,
          screenY: p.y,
          pageX: p.x,
          pageY: p.y,
        });
        const first = make(a);
        const last = make(b);
        el.dispatchEvent(new TouchEvent('touchstart', {
          bubbles: true,
          cancelable: true,
          touches: [first],
          targetTouches: [first],
          changedTouches: [first],
        }));
        el.dispatchEvent(new TouchEvent('touchmove', {
          bubbles: true,
          cancelable: true,
          touches: [last],
          targetTouches: [last],
          changedTouches: [last],
        }));
        el.dispatchEvent(new TouchEvent('touchend', {
          bubbles: true,
          cancelable: true,
          touches: [],
          targetTouches: [],
          changedTouches: [last],
        }));
      }, { start, end });
      await page.waitForTimeout(250);
      const copyVisible = await page.locator('.copy-pill').isVisible().catch(() => false);
      return { ok: copyVisible, row, endRow, start, end, copyVisible };
    })().catch((error) => ({
      ok: false,
      reason: error instanceof Error ? error.message : String(error),
    }));

    const visualReduced =
      (baseline.visualHeight != null && reduced.visualHeight != null
        && reduced.visualHeight < baseline.visualHeight - 20)
      || reduced.innerHeight < baseline.innerHeight - 20;
    const resizeSamples = [reduced, ...jitter];
    // A zero translation is correct when the focused input already remains
    // above the IME. Require motion only when the measured input actually
    // intersects the keyboard; this avoids encoding a needless jump as a
    // stability requirement.
    const shiftRequired = resizeSamples.some(
      (sample) => sample.keyboardTop != null
        && sample.inputBottom != null
        && sample.inputBottom > sample.keyboardTop + 1,
    );
    const shiftObserved = resizeSamples.some((sample) => sample.shiftY < -1);
    const shiftBounded = resizeSamples.every((sample) => Number.isFinite(sample.shiftY) && sample.shiftY <= 0);
    const inputSafe = resizeSamples
      .filter((sample) => sample.keyboardTop != null && sample.inputBottom != null)
      .every((sample) => sample.inputBottom <= sample.keyboardTop + 1);
    const recovered = Math.abs(restored.shiftY) <= 1 && restored.focused;
    result = {
      ok: visualReduced
        && (!shiftRequired || shiftObserved)
        && shiftBounded
        && inputSafe
        && recovered
        && selectionTouch.ok
        && browserErrors.length === 0,
      emulation: 'Chromium mobile context; not physical-device evidence',
      port,
      url,
      browser: 'Chromium Playwright --disable-extensions',
      browserErrors,
      baseline,
      reduced,
      jitter,
      restored,
      selectionTouch,
      assertions: {
        visualReduced,
        shiftRequired,
        shiftObserved,
        shiftBounded,
        inputSafe,
        recovered,
        selectionTouch: selectionTouch.ok,
      },
    };
  } catch (error) {
    result = {
      ok: false,
      emulation: 'Chromium mobile context; not physical-device evidence',
      port,
      browserErrors,
      error: error instanceof Error ? error.message : String(error),
    };
  } finally {
    await context?.close().catch(() => {});
    await browser?.close().catch(() => {});
    await killTree(host.pid);
    try {
      unlinkSync(STATUS_FILE);
    } catch {
      /* ok */
    }
  }
  writeFileSync(EVIDENCE_FILE, `${JSON.stringify(result, null, 2)}\n`);
  console.log(JSON.stringify({ evidence: EVIDENCE_FILE, ...result }));
  if (!result.ok) process.exitCode = 1;
}

await main();
