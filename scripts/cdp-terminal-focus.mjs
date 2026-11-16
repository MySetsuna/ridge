#!/usr/bin/env node
// Explicit private dev:cdp profile only. No shell commands or model prompts.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import { chromium, expect } from '@playwright/test';

const profile = path.resolve(process.argv[2] || '');
const artifacts = path.resolve('artifacts') + path.sep;
assert.ok(profile.startsWith(artifacts), 'Provide a private dev:cdp profile under artifacts/');
const config = JSON.parse(await fs.readFile(path.join(profile, 'tauri-dev-cdp.config.json'), 'utf8'));
const [port, endpoint] = (await fs.readFile(path.join(profile, 'EBWebView', 'DevToolsActivePort'), 'utf8')).trim().split(/\r?\n/);
assert.match(port, /^\d+$/);
assert.match(endpoint, /^\/devtools\/browser\/[a-z0-9-]+$/);
const browser = await chromium.connectOverCDP(`ws://127.0.0.1:${port}${endpoint}`, { timeout: 5000 });
const results = [];
try {
  const page = browser.contexts().flatMap(context => context.pages()).find(page => page.url().startsWith(config.build.devUrl));
  assert.ok(page, 'Private native app page must be running');
  page.setDefaultTimeout(5000);
  const pageErrors = [];
  page.on('pageerror', error => pageErrors.push(String(error)));
  const tabs = page.locator('[data-ws-tab-id]');
  const tabIds = await tabs.evaluateAll(elements => elements.map(element => element.dataset.wsTabId));
  assert.ok(tabIds.length >= 2, 'Prepare two private test workspaces');
  const [primaryWorkspace, secondaryWorkspace] = tabIds.slice(-2);
  const panes = page.locator('[data-rg-pane-id]:visible');
  let expectedWorkspace;
  const focusedPane = () => page.evaluate(expected => {
    const pane = document.querySelector('[data-rg-pane-active="true"]');
    const target = document.activeElement;
    return !!pane && pane.getBoundingClientRect().width > 0 && pane.contains(target)
      && pane.closest('[data-rg-ws-pane-host]')?.getAttribute('data-rg-ws-pane-host') === expected
      && (target.matches('textarea.rg-ime-helper') || target === pane);
  }, expectedWorkspace);
  async function checkLiteralInput() {
    // Existing split layouts may expose only a few columns/rows.
    const marker = `Z${Math.random().toString(36).slice(2, 4)}`;
    try {
      // No Enter: verify real shell echo, then clear the unexecuted input line.
      await page.keyboard.type(marker);
      await expect.poll(() => page.evaluate(marker => {
        const id = document.activeElement?.closest('[data-rg-pane-id]')?.getAttribute('data-rg-pane-id');
        const probe = window.__windE2E;
        const key = probe?.paneKeys().find(key => key === id || key.endsWith(`:${id}`));
        return key ? probe.visibleText(key).join('').includes(marker) : false;
      }, marker), { timeout: 5000 }).toBe(true);
    } finally {
      for (const character of marker) await page.keyboard.press('Backspace');
    }
  }
  for (const id of [secondaryWorkspace, primaryWorkspace]) {
    expectedWorkspace = id;
    await page.locator(`[data-ws-tab-id="${id}"]`).click();
    await expect.poll(focusedPane, { timeout: 5000 }).toBe(true);
    results.push({ action: 'switch', workspaceId: id, focused: true });
  }
  for (const id of [secondaryWorkspace, primaryWorkspace, secondaryWorkspace, primaryWorkspace]) {
    expectedWorkspace = id;
    await page.locator(`[data-ws-tab-id="${id}"]`).click();
  }
  await expect.poll(focusedPane, { timeout: 5000 }).toBe(true);
  results.push({ action: 'rapid workspace switches', focused: true });
  for (const title of ['左右分屏（当前选中窗格）', '上下分屏（当前选中窗格）']) {
    const before = await panes.evaluateAll(elements => elements.map(element => element.dataset.rgPaneId));
    await page.getByRole('button', { name: title, exact: true }).click();
    await expect(panes).toHaveCount(before.length + 1);
    await expect.poll(focusedPane, { timeout: 5000 }).toBe(true);
    await checkLiteralInput();
    const added = await panes.evaluateAll((elements, previous) => elements.map(element => element.dataset.rgPaneId).filter(id => !previous.includes(id)), before);
    assert.equal(added.length, 1);
    results.push({ action: title, addedPane: added[0], focused: true, literalEcho: true });
  }
  await page.getByRole('button', { name: '+', exact: true }).click();
  await expect(tabs).toHaveCount(tabIds.length + 1);
  expectedWorkspace = (await tabs.evaluateAll(elements => elements.map(element => element.dataset.wsTabId))).find(id => !tabIds.includes(id));
  assert.ok(expectedWorkspace, 'New workspace has its own identity');
  await expect.poll(focusedPane, { timeout: 5000 }).toBe(true);
  await checkLiteralInput();
  results.push({ action: 'create workspace', focused: true, literalEcho: true });
  await expect(page.getByRole('dialog', { name: /^Unhandled|^Terminal navigation failed/ })).toHaveCount(0);
  assert.deepEqual(pageErrors, []);
  const evidence = { source: 'native dev:cdp', profile, shellCommandsSent: 0, pageErrors, results };
  await fs.writeFile(path.join(profile, 'focus-after.json'), JSON.stringify(evidence, null, 2));
  console.log(JSON.stringify(evidence, null, 2));
} finally {
  await browser.close();
}
