#!/usr/bin/env node
// Private native dev:cdp only. Inject read-only startup failures, never shell commands.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import { chromium, expect } from '@playwright/test';

const profile = path.resolve(process.argv[2] || '');
assert.ok(profile.startsWith(path.resolve('artifacts') + path.sep), 'Provide a private profile under artifacts/');
const config = JSON.parse(await fs.readFile(path.join(profile, 'tauri-dev-cdp.config.json'), 'utf8'));
const endpoint = JSON.parse(await fs.readFile(path.join(profile, 'dev-kernel/kernel.json'), 'utf8'));
async function processes() {
  const response = await fetch(`http://127.0.0.1:${endpoint.port}/v1/domain/ptys`, {
    headers: { 'x-ridge-kernel-token': endpoint.token }, signal: AbortSignal.timeout(5000),
  });
  assert.equal(response.status, 200);
  const result = await response.json();
  assert.equal(result.ok, true);
  return result.ptys.map(p => ({ id: p.id, pid: p.child_pid, start: p.child_start_instance })).sort((a, b) => a.id.localeCompare(b.id));
}
const [port, target] = (await fs.readFile(path.join(profile, 'EBWebView/DevToolsActivePort'), 'utf8')).trim().split(/\r?\n/);
assert.match(port, /^\d+$/);
assert.match(target, /^\/devtools\/browser\/[a-z0-9-]+$/);
const browser = await chromium.connectOverCDP(`ws://127.0.0.1:${port}${target}`, { timeout: 5000 });
let page;
try {
  page = browser.contexts().flatMap(c => c.pages()).find(p => p.url().startsWith(config.build.devUrl));
  assert.ok(page, 'Private native app must be running');
  page.setDefaultTimeout(10000);
  const panes = page.locator('[data-rg-pane-id]:visible');
  await expect.poll(() => panes.count()).toBeGreaterThan(0);
  await expect(page.locator('.rg-webgpu-error')).toHaveCount(0);
  const before = await processes();
  assert.ok(before.length > 0, 'Prepare live private PTYs');
  await page.evaluate(() => sessionStorage.setItem('ridge.e2e.rendererFault', '1'));
  await page.addInitScript(() => {
    if (sessionStorage.getItem('ridge.e2e.rendererFault') !== '1') return;
    const original = WebSocket.prototype.send;
    const fault = { failures: 0, realRequests: 0, restore: () => { WebSocket.prototype.send = original; } };
    window.__ridgeRendererFault = fault;
    WebSocket.prototype.send = function (data) {
      let frame;
      if (typeof data === 'string') { try { frame = JSON.parse(data); } catch {} }
      if (this.url.includes('/v1/local/ws') && (frame?.method ?? frame?.cmd) === 'load_terminal_font_faces') {
        if (fault.failures < 2) { fault.failures++; throw Error('Injected read-only font startup timeout'); }
        fault.realRequests++;
      }
      return original.call(this, data);
    };
  });
  const pageErrors = [];
  page.on('pageerror', error => pageErrors.push(String(error)));
  await page.reload({ waitUntil: 'domcontentloaded' });
  const retries = page.getByRole('button', { name: /重试终端渲染|Retry terminal rendering/ });
  await expect.poll(() => retries.count(), { timeout: 30000 }).toBeGreaterThan(0);
  assert.equal(await page.evaluate(() => window.__ridgeRendererFault.failures), 2);
  assert.deepEqual(await processes(), before, 'Renderer failure must preserve live PTYs');
  for (const retry of await retries.elementHandles()) {
    await retry.evaluate(button => { button.click(); button.click(); });
  }
  await expect(page.locator('.rg-webgpu-error')).toHaveCount(0, { timeout: 45000 });
  await expect.poll(() => page.evaluate(() => {
    const probe = window.__windE2E;
    return !!probe && [...document.querySelectorAll('[data-rg-pane-id]')].every(p =>
      probe.paneKeys().some(key => key === p.dataset.rgPaneId || key.endsWith(`:${p.dataset.rgPaneId}`)));
  }), { timeout: 15000 }).toBe(true);
  await panes.first().click();
  const marker = 'R17';
  try {
    await page.keyboard.type(marker);
    await expect.poll(() => page.evaluate(text => {
      const id = document.activeElement?.closest('[data-rg-pane-id]')?.getAttribute('data-rg-pane-id');
      const probe = window.__windE2E;
      const key = probe?.paneKeys().find(key => key === id || key.endsWith(`:${id}`));
      return !!key && probe.visibleText(key).join('').includes(text);
    }, marker)).toBe(true);
  } finally {
    for (const character of marker) await page.keyboard.press('Backspace');
  }
  assert.deepEqual(await processes(), before, 'Repeated recovery must not replace or duplicate PTYs');
  assert.deepEqual(pageErrors, []);
  const counts = await page.evaluate(() => ({ failures: window.__ridgeRendererFault.failures, realRequests: window.__ridgeRendererFault.realRequests }));
  const evidence = { source: 'private native dev:cdp', injectedReadOnlyFailures: counts.failures,
    realFontRequests: counts.realRequests, samePtys: before, repeatedClickRecovery: true,
    literalEcho: true, shellCommandsSent: 0, pageErrors };
  await fs.writeFile(path.join(profile, 'renderer-retry-evidence.json'), JSON.stringify(evidence, null, 2));
  console.log(JSON.stringify(evidence));
} finally {
  await page?.evaluate(() => { sessionStorage.removeItem('ridge.e2e.rendererFault'); window.__ridgeRendererFault?.restore(); }).catch(() => {});
  await browser.close();
}
