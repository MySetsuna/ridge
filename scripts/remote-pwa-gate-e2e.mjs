import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs/promises';
import { createServer } from 'node:http';
import path from 'node:path';
import { chromium } from '@playwright/test';
import { pnpmInvocation } from './lib/toolPath.mjs';

const repo = path.resolve(import.meta.dirname, '..');
const dist = path.join(repo, 'remote-dist', 'mobile');
const artifacts = path.join(repo, 'artifacts', 'remote-pwa-gate-e2e');
await fs.mkdir(artifacts, { recursive: true });
const runDir = await fs.mkdtemp(path.join(artifacts, 'run-'));
const evidence = { scope: 'Production mobile App and generated Workbox SW in real Chromium. Isolated HTTP fixture, read-only Cloud 401 fixture, passive worker version message probe.', checks: [] };
let browser, server, revision = 1;
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json',
  '.webmanifest': 'application/manifest+json', '.wasm': 'application/wasm', '.png': 'image/png', '.svg': 'image/svg+xml', '.woff2': 'font/woff2' };

async function workerVersion(page) {
  return page.evaluate(() => new Promise((resolve, reject) => {
    const channel = new MessageChannel();
    const timeout = setTimeout(() => reject(Error('active worker probe timeout')), 5000);
    channel.port1.onmessage = event => { clearTimeout(timeout); channel.port1.close(); resolve(event.data); };
    navigator.serviceWorker.controller.postMessage({ type: 'RIDGE_ACCEPTANCE_VERSION' }, [channel.port2]);
  }));
}

try {
  const pnpm = pnpmInvocation();
  const build = spawnSync(pnpm.command, [...pnpm.args, 'build:remote:mobile'], { cwd: repo, windowsHide: true, stdio: 'inherit' });
  assert.equal(build.status, 0, 'mobile production build failed');
  const worker = await fs.readFile(path.join(dist, 'sw.js'), 'utf8');
  server = createServer(async (request, response) => {
    const url = new URL(request.url, 'http://127.0.0.1');
    if (url.pathname === '/sw.js') {
      response.writeHead(200, { 'content-type': 'text/javascript', 'cache-control': 'no-store', 'service-worker-allowed': '/' });
      response.end(worker + `\nself.addEventListener('message', event => { if (event.data?.type === 'RIDGE_ACCEPTANCE_VERSION') event.ports[0]?.postMessage(${revision}); });\n`);
      return;
    }
    if (url.pathname.startsWith('/api/')) {
      response.writeHead(401, { 'content-type': 'application/json' });
      response.end(JSON.stringify({ error: { code: 'UNAUTHORIZED', message: 'owned signed-out fixture' } }));
      return;
    }
    const file = path.resolve(dist, '.' + (url.pathname === '/' ? '/index.html' : decodeURIComponent(url.pathname)));
    if (!file.startsWith(dist + path.sep)) { response.writeHead(403); response.end(); return; }
    try {
      const body = await fs.readFile(file);
      response.writeHead(200, { 'content-type': mime[path.extname(file)] || 'application/octet-stream', 'cache-control': 'no-store' });
      response.end(body);
    } catch { response.writeHead(404); response.end('not found'); }
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  browser = await chromium.launch({ channel: 'chrome', headless: true });
  for (const mode of ['lan', 'cloud']) {
    revision = 1;
    const context = await browser.newContext({ viewport: { width: 390, height: 480 },
      userAgent: 'Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 Chrome/130 Mobile Safari/537.36' });
    try {
      const page = await context.newPage();
      const check = { mode, stage: 'opening', consoleErrors: [] };
      evidence.checks.push(check);
      page.on('console', message => { if (message.type() === 'error') check.consoleErrors.push(message.text()); });
      page.on('pageerror', error => check.consoleErrors.push(String(error)));
      await page.addInitScript(() => sessionStorage.setItem('ridge_tenant_login_bounce', '1'));
      await page.route('https://9527127.xyz/**', route => route.fulfill({ status: 401,
        headers: { 'content-type': 'application/json', 'access-control-allow-origin': origin, 'access-control-allow-credentials': 'true' },
        body: JSON.stringify({ error: { code: 'UNAUTHORIZED', message: 'owned signed-out fixture' } }) }));
      let loads = 0;
      page.on('load', () => { loads++; });
      const query = mode === 'cloud' ? '?ui=mobile&cloudHost=authprobe&cloudUser=jack2' : '?ui=mobile';
      await page.goto(origin + '/' + query, { waitUntil: 'domcontentloaded' });
      await page.locator('.screen').waitFor();
      if (mode === 'cloud') await page.locator('.screen .error-msg').waitFor();
      check.stage = 'initial-worker-control';
      await page.evaluate(async () => { await navigator.serviceWorker.ready; });
      // The generated prompt worker does not claim already-open documents.
      // Reload through its real scope before requesting a second revision.
      await page.reload({ waitUntil: 'domcontentloaded' });
      await page.waitForFunction(() => Boolean(navigator.serviceWorker.controller), undefined, { timeout: 30000 });
      assert.equal(await workerVersion(page), 1);
      assert.equal(await page.evaluate(() => document.visibilityState), 'visible');
      revision = 2;
      check.stage = 'waiting-worker';
      await page.evaluate(async () => { await (await navigator.serviceWorker.getRegistration()).update(); });
      await page.waitForFunction(async () => (await navigator.serviceWorker.getRegistration())?.waiting?.state === 'installed', undefined, { timeout: 30000 });
      assert.equal(await workerVersion(page), 1, 'waiting update was applied without the foreground action');
      check.foregroundWaitingVersion = 1;
      check.stage = 'update-entry';
      const apply = page.getByRole('button', { name: /立即更新|Update now/i });
      await apply.waitFor({ state: 'visible', timeout: 3000 });
      const banner = page.locator('.pwa-update-banner');
      assert.equal(await banner.count(), 1);
      const bannerRect = await banner.boundingBox();
      const authRect = await page.locator('.screen').boundingBox();
      assert.ok(bannerRect && authRect && authRect.y >= bannerRect.y + bannerRect.height - 1, 'auth page overlaps update controls');
      for (const rect of await banner.locator('button').evaluateAll(buttons => buttons.map(button => button.getBoundingClientRect().height))) {
        assert.ok(rect >= 44, 'update touch target is too short');
      }
      await page.screenshot({ path: path.join(runDir, `${mode}-waiting.png`), scale: 'device' });
      const lazy = await page.evaluate(() => performance.getEntriesByType('resource').map(entry => entry.name)
        .filter(name => /\/(MainApp-|manager-)|\.wasm(?:$|\?)/.test(name)));
      assert.deepEqual(lazy, [], 'connection gate imported terminal resources');
      await page.getByRole('button', { name: /稍后|Later/i }).click();
      assert.equal(await banner.count(), 0);
      assert.equal(await workerVersion(page), 1);
      assert.equal(await page.evaluate(async () => Boolean((await navigator.serviceWorker.getRegistration())?.waiting)), true);
      await page.reload({ waitUntil: 'domcontentloaded' });
      await apply.waitFor({ state: 'visible', timeout: 10000 });
      const before = loads;
      const reloaded = page.waitForEvent('load');
      await apply.click();
      await reloaded;
      await page.waitForFunction(async () => {
        const registration = await navigator.serviceWorker.getRegistration();
        return registration?.active?.state === 'activated' && !registration.waiting;
      }, undefined, { timeout: 20000 });
      await page.waitForLoadState('load');
      assert.ok(loads > before, 'app did not reload after applying the waiting worker');
      assert.equal(await workerVersion(page), 2, 'the active controller is still the previous worker');
      Object.assign(check, { stage: 'passed', appliedControllerVersion: 2, reloadObserved: true,
        bannerCount: 1, laterKeptWaiting: true, shortViewportAuthTop: authRect.y, bannerHeight: bannerRect.height, pageTerminalImports: lazy });
    } finally { await context.close(); }
  }
  evidence.result = 'PASS';
} catch (error) {
  evidence.result = 'FAIL';
  evidence.error = String(error);
  process.exitCode = 1;
} finally {
  await browser?.close();
  if (server) await new Promise(resolve => server.close(resolve));
  evidence.artifactDirectory = runDir;
  await fs.writeFile(path.join(runDir, 'evidence.json'), JSON.stringify(evidence, null, 2));
  console.log(JSON.stringify(evidence, null, 2));
}
