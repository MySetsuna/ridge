#!/usr/bin/env node
// Iteration 63 true-LAN mobile E2E. Uses a fresh browser context and the
// one-time pairing code; never reads cookies/localStorage or prints credentials.
import { chromium } from '@playwright/test';

const CDP = process.env.RIDGE_CDP_URL || '';
const HOST = process.env.RIDGE_URL || 'https://127.0.0.1:9527/?ui=mobile';
const CODE = (process.env.RIDGE_CODE || '').replace(/\D/g, '').slice(0, 6);
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const assert = (ok, message) => {
  if (!ok) throw new Error(message);
  console.log(`[iter63-e2e] PASS ${message}`);
};
const uuidFromBytes = (bytes) => {
  const h = [...bytes.subarray(0, 16)].map((b) => b.toString(16).padStart(2, '0')).join('');
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
};

const browser = CDP
  ? await chromium.connectOverCDP(CDP)
  : await chromium.launch({ channel: 'chrome', headless: true });
// A fresh context makes the test independent from a user's persisted token and
// prevents test workspaces/panes from sharing browser storage with real tabs.
const context = await browser.newContext({
  ignoreHTTPSErrors: true,
  viewport: { width: 390, height: 844 },
  isMobile: true,
  hasTouch: true,
  userAgent:
    'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1',
});
const page = await context.newPage();

const sent = [];
const received = [];
const receivedControls = [];
const browserErrors = [];
let verifyDevice = null;
let socketDevice = null;
page.on('request', (request) => {
  if (request.method() !== 'POST' || new URL(request.url()).pathname !== '/verify') return;
  verifyDevice = new URLSearchParams(request.postData() ?? '').get('device');
});
page.on('console', (message) => {
  if (message.type() === 'error') browserErrors.push(message.text());
});
page.on('pageerror', (error) => browserErrors.push(error.message));
page.on('websocket', (socket) => {
  if (!socket.url().includes('/ws?')) return;
  socketDevice = new URL(socket.url()).searchParams.get('device');
  socket.on('socketerror', (error) => browserErrors.push(`ws: ${error}`));
  socket.on('close', () => browserErrors.push(`ws closed: ${socket.url().replace(/([?&](?:token|code)=)[^&]+/g, '$1<redacted>')}`));
  socket.on('framesent', ({ payload }) => {
    if (typeof payload !== 'string') return;
    try { sent.push({ at: Date.now(), value: JSON.parse(payload) }); } catch { /* non-JSON */ }
  });
  socket.on('framereceived', ({ payload }) => {
    if (typeof payload === 'string') {
      try { receivedControls.push({ at: Date.now(), value: JSON.parse(payload) }); } catch { /* non-JSON */ }
      return;
    }
    const bytes = new Uint8Array(payload);
    if (bytes.length >= 16) received.push({
      at: Date.now(),
      paneId: uuidFromBytes(bytes),
      byteLength: bytes.length - 16,
      ris: bytes[16] === 0x1b && bytes[17] === 0x63,
    });
  });
});

await page.goto(HOST, { waitUntil: 'domcontentloaded' });
const authInput = page.locator('input[inputmode="numeric"]');
if (!(await authInput.count()) && !(await page.locator('.tree-trigger').count())) {
  await Promise.race([
    authInput.waitFor({ state: 'visible', timeout: 45_000 }),
    page.locator('.tree-trigger').waitFor({ state: 'visible', timeout: 45_000 }),
  ]);
}
if (await authInput.count()) {
  if (!CODE) throw new Error('remote tab requires RIDGE_CODE');
  await authInput.fill(CODE);
  await authInput.press('Enter');
}
try {
  await page.waitForSelector('.tree-trigger', { timeout: 20_000 });
} catch (error) {
  const state = await page.locator('body').innerText().catch(() => '');
  const binding = `device_match=${verifyDevice !== null && verifyDevice === socketDevice}`;
  throw new Error(`remote authentication did not reach MainApp: ${state.slice(0, 240)}; ${binding}; ${browserErrors.slice(-4).join(' | ')}`, {
    cause: error,
  });
}
await sleep(1_500);

const openTree = async () => {
  if (!(await page.locator('.tree-popup').count())) {
    await page.locator('.tree-trigger').click();
    await page.waitForSelector('.tree-popup');
  }
};
const closeTree = async () => {
  const backdrop = page.locator('.tree-backdrop');
  if (await backdrop.count()) await backdrop.click({ force: true, position: { x: 1, y: 1 } });
};
const activeSubscription = () => [...sent]
  .reverse()
  .find(({ value }) => value.type === 'subscribe-pane' && value.active === true)?.value;
const waitForActive = async (notPane = '') => {
  const start = Date.now();
  while (Date.now() - start < 10_000) {
    const sub = activeSubscription();
    if (sub?.paneId && sub.paneId !== notPane) return sub;
    await sleep(100);
  }
  throw new Error('active pane subscription did not change');
};

await openTree();
let createdPane = false;
let createdWorkspace = false;
let createdWorkspaceName = '';
const originalWorkspaceName = (await page.locator('.ws-row.active .ws-name').textContent())?.trim();
let paneRows = page.locator('.pane-group .pane-row');
const originalPaneIndex = await paneRows.evaluateAll((rows) =>
  rows.findIndex((row) => row.classList.contains('active')));
const paneA = activeSubscription();
assert(!!paneA?.paneId, 'initial active pane identified from real WS');
if (await paneRows.count() < 2) {
  await page.locator('.pane-new').first().click();
  try {
    await page.waitForFunction(() => document.querySelectorAll('.pane-group .pane-row').length >= 2);
  } catch (error) {
    const treeError = await page.locator('.tree-error').textContent().catch(() => '');
    throw new Error(`failed to create E2E pane: ${treeError}; ${browserErrors.slice(-4).join(' | ')}`, {
      cause: error,
    });
  }
  const created = await waitForActive(paneA.paneId);
  await page.locator('.pane-group .pane-row').nth(originalPaneIndex).click();
  await waitForActive(created.paneId);
  createdPane = true;
}

// Keep A emitting while it is off-screen.
await closeTree();
await page.locator('.hidden-input').focus();
await page.keyboard.type(
  `1..60 | % { Write-Output "ITER63_BG_$($_)"; Start-Sleep -Milliseconds 150 }`,
);
await page.keyboard.press('Enter');
await sleep(400);

await openTree();
paneRows = page.locator('.pane-group .pane-row');
await page.locator('.pane-group .pane-row:not(.active)').last().click();
const paneB = await waitForActive(paneA.paneId);
assert(paneB.paneId !== paneA.paneId, 'pane switch promotes a distinct pane');
const awayFromA = Date.now();
await sleep(1_500);
assert(
  received.some((frame) => frame.paneId === paneA.paneId && frame.at >= awayFromA),
  'pane A continues receiving raw bytes while pane B is active',
);

await openTree();
let wsRows = page.locator('.ws-row');
if (await wsRows.count() < 2) {
  await page.locator('.tree-add:not([data-testid="tree-open-saved"])').click();
  await page.waitForFunction(() => document.querySelectorAll('.ws-row').length >= 2);
  createdWorkspace = true;
  await sleep(500);
  createdWorkspaceName = (await page.locator('.ws-row.active .ws-name').textContent())?.trim() ?? '';
} else {
  await page.locator('.ws-row:not(.active)').first().click();
}
const paneC = await waitForActive(paneB.paneId);
assert(!!paneC.workspaceId && paneC.workspaceId !== paneA.workspaceId, 'workspace switch promotes pane C');
const crossWorkspaceAt = Date.now();
await sleep(1_500);
assert(
  received.some((frame) => frame.paneId === paneA.paneId && frame.at >= crossWorkspaceAt),
  'pane A continues receiving raw bytes across workspace switch',
);

await openTree();
wsRows = page.locator('.ws-row');
await wsRows.filter({ hasText: originalWorkspaceName }).first().click();
await waitForActive(paneC.paneId);
await openTree();
// Select A by returning to the row that was active at test start: the first
// promotion in the original workspace uses resume=true and must not replay RIS.
const beforeReturn = Date.now();
await page.locator('.pane-group .pane-row').nth(originalPaneIndex).click();
const returned = await waitForActive(paneC.paneId);
assert(returned.paneId === paneA.paneId, 'returned to original pane A');
await sleep(500);

assert(!sent.some(({ value }) => value.type === 'unsubscribe-pane'), 'ordinary switches send no unsubscribe');
assert(
  !received.some((frame) => frame.paneId === paneA.paneId && frame.at >= beforeReturn && frame.ris),
  'return to live pane A sends no full RIS replay',
);

await openTree();
const iconStyles = await page.evaluate(() => {
  const style = (selector) => {
    const el = document.querySelector(selector);
    if (!el) return null;
    const cs = getComputedStyle(el);
    return { border: cs.borderStyle, background: cs.backgroundColor };
  };
  return { agent: style('.row-agent'), shell: style('.row-shell') };
});
for (const [name, style] of Object.entries(iconStyles)) {
  if (!style) continue;
  assert(style.border === 'none' && style.background === 'rgba(0, 0, 0, 0)', `${name} control is pure icon`);
}

// Remove only resources this test created.
if (createdPane) {
  const close = page.locator('.pane-row:not(.active) .row-close').last();
  if (await close.count()) await close.click();
}
if (createdWorkspace) {
  const close = page.locator('.ws-row', { hasText: createdWorkspaceName }).locator('.row-close').last();
  if (await close.count()) await close.click();
}

// Soft-keyboard viewport contraction must move only the visual projection.
await closeTree();
await page.locator('.hidden-input').focus();
const terminalGeometry = () => page.evaluate(() => {
  const stage = document.querySelector('.term-stage');
  const container = document.querySelector('.container');
  if (!(stage instanceof HTMLElement) || !(container instanceof HTMLElement)) return null;
  const transform = getComputedStyle(stage).transform;
  return {
    innerHeight: window.innerHeight,
    visualHeight: window.visualViewport?.height ?? 0,
    stageTop: stage.getBoundingClientRect().top,
    transform,
    transformY: transform === 'none' ? 0 : new DOMMatrixReadOnly(transform).m42,
    containerWidth: container.clientWidth,
    containerHeight: container.clientHeight,
    canvases: [...stage.querySelectorAll('canvas')].map((canvas) => ({
      width: canvas.width,
      height: canvas.height,
      cssWidth: canvas.getBoundingClientRect().width,
      cssHeight: canvas.getBoundingClientRect().height,
    })),
  };
});
const keyboardBefore = await terminalGeometry();
assert(!!keyboardBefore, 'terminal geometry is observable before soft keyboard');
await page.evaluate(() => {
  if (!window.visualViewport) throw new Error('visualViewport unavailable');
  Object.defineProperty(window.visualViewport, 'height', {
    configurable: true,
    value: 280,
  });
  window.visualViewport.dispatchEvent(new Event('resize'));
});
await sleep(300);
const keyboardOpen = await terminalGeometry();
await page.evaluate(() => {
  if (!window.visualViewport) return;
  delete window.visualViewport.height;
  window.visualViewport.dispatchEvent(new Event('resize'));
});
await sleep(300);
const keyboardClosed = await terminalGeometry();
assert(
  keyboardOpen.visualHeight < keyboardBefore.visualHeight
    && keyboardOpen.innerHeight === keyboardBefore.innerHeight,
  `soft keyboard contracts visual viewport without changing layout viewport `
    + `(before=${keyboardBefore.innerHeight}/${keyboardBefore.visualHeight}, `
    + `open=${keyboardOpen.innerHeight}/${keyboardOpen.visualHeight})`,
);
assert(
  keyboardOpen.containerWidth === keyboardBefore.containerWidth
    && keyboardOpen.containerHeight === keyboardBefore.containerHeight
    && JSON.stringify(keyboardOpen.canvases) === JSON.stringify(keyboardBefore.canvases),
  'soft keyboard leaves terminal container and canvas dimensions unchanged',
);
assert(
  keyboardOpen.transformY <= 0
    && keyboardOpen.transformY >= -keyboardOpen.containerHeight,
  'soft keyboard visual transform remains finite and upward-bounded',
);
assert(
  Math.abs(keyboardClosed.stageTop - keyboardBefore.stageTop) <= 1
    && keyboardClosed.containerHeight === keyboardBefore.containerHeight,
  'closing soft keyboard restores visual projection without refit',
);

// Build > initial-tail history on the real PTY, reconnect, then prove an
// adjacent older page commits through the visible loading indicator.
const bytesForPane = (paneId) => received
  .filter((frame) => frame.paneId === paneId)
  .reduce((sum, frame) => sum + frame.byteLength, 0);
const beforeBulkBytes = bytesForPane(paneA.paneId);
await page.locator('.hidden-input').focus();
await page.keyboard.type(
  `1..3500 | % { Write-Output ("ITER63_SCROLL_$($_)_" + ("x" * 160)) }`,
);
await page.keyboard.press('Enter');
const bulkDeadline = Date.now() + 30_000;
while (Date.now() < bulkDeadline && bytesForPane(paneA.paneId) - beforeBulkBytes < 500_000) {
  await sleep(100);
}
assert(bytesForPane(paneA.paneId) - beforeBulkBytes >= 500_000, 'real PTY produced paged scrollback history');

await page.reload({ waitUntil: 'domcontentloaded' });
await page.waitForSelector('.tree-trigger', { timeout: 20_000 });
await sleep(1_000);
const requestsBeforeScroll = sent.filter(({ value }) => value.type === 'scrollback-before').length;
const loadingSeen = await page.evaluate(async () => {
  const container = document.querySelector('.container');
  if (!(container instanceof HTMLElement)) return false;
  const initiallySeen = !!document.querySelector('.scrollback-loading');
  let resolveSeen = () => {};
  const seenPromise = new Promise((resolve) => {
    resolveSeen = resolve;
  });
  const observer = new MutationObserver((records) => {
    if (!initiallySeen && records.some((record) =>
      (record.type === 'attributes' && record.target.getAttribute?.('aria-busy') === 'true')
      || [...record.addedNodes].some((node) =>
        node instanceof Element
        && (node.matches('.scrollback-loading') || node.querySelector('.scrollback-loading')))
    )) resolveSeen(true);
  });
  observer.observe(document.body, {
    childList: true,
    subtree: true,
    attributes: true,
    attributeFilter: ['aria-busy'],
  });
  for (let batch = 0; batch < 160; batch += 1) {
    for (let i = 0; i < 8; i += 1) {
      container.dispatchEvent(new WheelEvent('wheel', { deltaY: -120, bubbles: true, cancelable: true }));
    }
    const observed = await Promise.race([
      seenPromise.then(() => true),
      new Promise((resolve) => setTimeout(() => resolve(false), 10)),
    ]);
    if (observed) break;
  }
  const observed = initiallySeen || await Promise.race([
    seenPromise.then(() => true),
    new Promise((resolve) => setTimeout(() => resolve(false), 500)),
  ]);
  observer.disconnect();
  return observed;
});
assert(loadingSeen, 'scrollback fetch exposes the shell-top loading bar');
const scrollRequest = sent
  .filter(({ value }) => value.type === 'scrollback-before')
  .slice(requestsBeforeScroll)[0];
assert(!!scrollRequest, 'real LAN transport requested an older scrollback page');
const scrollResult = receivedControls.find(({ at, value }) =>
  at >= scrollRequest.at && value.type === 'scrollback-before-result');
assert(
  !!scrollResult
    && Number(scrollResult.value.startSeq) < Number(scrollResult.value.endSeq)
    && Number(scrollResult.value.endSeq) === Number(scrollRequest.value.beforeSeq),
  'scrollback page is non-empty and exactly adjacent to its cursor',
);
await page.waitForFunction(() =>
  !document.querySelector('.scrollback-loading')
  && document.querySelector('.container')?.getAttribute('aria-busy') === 'false',
);
assert(true, 'scrollback loading state clears after atomic commit');

// Open desktop Remote's Git tab and verify the real JSON-RPC route that
// previously admitted git_stash_list but had no host dispatch arm.
const desktopPage = await context.newPage();
await desktopPage.setViewportSize({ width: 1280, height: 800 });
let stashRequest = null;
let stashReply = null;
desktopPage.on('websocket', (socket) => {
  if (!socket.url().includes('/ws?')) return;
  socket.on('framesent', ({ payload }) => {
    if (typeof payload !== 'string') return;
    try {
      const value = JSON.parse(payload);
      if (value.method === 'git_stash_list' || value.cmd === 'git_stash_list') {
        stashRequest = value;
      }
    } catch { /* non-JSON */ }
  });
  socket.on('framereceived', ({ payload }) => {
    if (typeof payload !== 'string' || !stashRequest) return;
    try {
      const value = JSON.parse(payload);
      const requestId = stashRequest.id ?? stashRequest._reqId;
      const responseId = value.id ?? value._reqId;
      if (requestId === responseId) stashReply = value;
    } catch { /* non-JSON */ }
  });
});
const desktopUrl = new URL(HOST);
desktopUrl.searchParams.set('ui', 'desktop');
await desktopPage.goto(desktopUrl.toString(), { waitUntil: 'domcontentloaded' });
const desktopAuth = desktopPage.locator('input[inputmode="numeric"]');
await desktopAuth.waitFor({ state: 'visible', timeout: 20_000 }).catch(() => {});
if (await desktopAuth.count()) {
  await desktopAuth.fill(CODE);
  await desktopAuth.press('Enter');
}
const desktopGit = desktopPage.locator('button[title="Git Graph"], button[title="Git"]').first();
await desktopGit.waitFor({ state: 'visible', timeout: 20_000 });
await desktopGit.click();
const directStash = await desktopPage.evaluate(() => new Promise((resolve, reject) => {
  const token = localStorage.getItem('ridge_remote_token');
  const device = localStorage.getItem('ridge_remote_device');
  if (!token || !device) return reject(new Error('desktop auth storage missing'));
  const socket = new WebSocket(
    `wss://${location.host}/ws?token=${encodeURIComponent(token)}&device=${encodeURIComponent(device)}`,
  );
  const requestId = 63_001;
  const timer = setTimeout(() => {
    socket.close();
    reject(new Error('git_stash_list direct route timeout'));
  }, 15_000);
  socket.onopen = () => {
    socket.send(JSON.stringify({
      jsonrpc: '2.0',
      method: '$/hello',
      params: { protocolVersion: 1, capabilities: ['git'] },
    }));
    socket.send(JSON.stringify({
      jsonrpc: '2.0',
      id: requestId,
      method: 'git_stash_list',
      params: { repoRoot: 'C:/code/wind' },
    }));
  };
  socket.onmessage = (event) => {
    const message = JSON.parse(event.data);
    if (message.id !== requestId) return;
    clearTimeout(timer);
    socket.close();
    resolve(message);
  };
  socket.onerror = () => reject(new Error('git_stash_list direct route socket error'));
}));
assert(
  !!directStash && !directStash.error,
  'desktop Remote direct git_stash_list invoke returned successfully',
);
const stashDeadline = Date.now() + 30_000;
while (Date.now() < stashDeadline && !stashReply) await sleep(100);
assert(!!stashRequest, 'desktop Remote requested git_stash_list on the real host');
assert(
  !!stashReply && !stashReply.error && !stashReply._error,
  'desktop Remote git_stash_list route returns without RpcRemoteError',
);
await desktopPage.close();

console.log('[iter63-e2e] RESULT PASS');
await context.close();
await browser.close();
