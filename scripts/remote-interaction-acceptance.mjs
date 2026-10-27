// Current-source host + real browser Remote input probe. No credentials/payload
// logs; only harmless fixture bytes and aggregate checks enter the evidence.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { chromium } from '@playwright/test';
import { resolveCdpPort, DEV_USER_DATA_DIR } from './cdp-port.mjs';

const root = path.resolve(import.meta.dirname, '..');
const config = JSON.parse(fs.readFileSync(path.join(DEV_USER_DATA_DIR, 'tauri-dev-cdp.config.json'), 'utf8'));
const targets = await fetch(`http://127.0.0.1:${resolveCdpPort()}/json/list`).then(response => response.json());
const target = targets.find(item => item.type === 'page' && item.url.startsWith(config.build.devUrl));
assert.ok(target, 'current-source tauri:dev:cdp target required');
const socket = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
let sequence = 0;
const pending = new Map();
socket.onmessage = event => {
  const response = JSON.parse(event.data);
  if (response.id) { pending.get(response.id)?.(response); pending.delete(response.id); }
};
async function evaluate(expression) {
  const id = ++sequence;
  const response = await Promise.race([
    new Promise(resolve => { pending.set(id, resolve); socket.send(JSON.stringify({ id, method: 'Runtime.evaluate',
      params: { expression, awaitPromise: true, returnByValue: true } })); }),
    new Promise((_, reject) => { const timer = setTimeout(() => reject(new Error('CDP evaluation timeout')), 15_000); timer.unref(); }),
  ]);
  assert.ok(!response.result?.exceptionDetails, 'current-source host command failed');
  return response.result?.result?.value;
}
const invoke = (command, args = {}) => evaluate(`import('/src/lib/transport/nativeRuntimeCore.ts').then(({invoke}) => invoke(${JSON.stringify(command)},${JSON.stringify(args)}))`);
if (process.argv.includes('--reload-frontend')) await invoke('set_remote_enabled', { enabled: false });
await invoke('set_remote_enabled', { enabled: true });
const host = await invoke('get_remote_info');
assert.ok(host.ready && host.port > 0, 'current-source Remote host ready');
const frontend = process.env.RIDGE_INTERACTION_URL || (process.argv.includes('--production') ? `https://127.0.0.1:${host.port}` : 'http://127.0.0.1:5174');
const evidence = { frontend, hostBuild: 'current tauri:dev:cdp source', transport: 'LAN HTTPS/WSS',
  mobileEvidence: 'Chromium emulation', checks: [], contexts: [] };
const devices = process.argv.includes('--mobile-only') ? [true] : process.argv.includes('--desktop-only') ? [false] : [false, true];
const evidencePath = path.join(DEV_USER_DATA_DIR, `remote-interaction-evidence${devices.length === 1 ? devices[0] ? '-mobile' : '-desktop' : ''}${process.argv.includes('--session-only') ? '-session' : process.argv.includes('--history-only') ? '-history' : ''}.json`);
const fixturePath = path.join(DEV_USER_DATA_DIR, 'remote-input-fixture.cjs');
fs.writeFileSync(fixturePath, String.raw`process.stdin.setRawMode(true); process.stdin.resume();
const reset=()=>process.stdout.write('\x1b[?1003l\x1b[?1006l\x1b[?1l');
process.stdout.write('\x1b[?1003h'+(process.argv[3]==='legacy'?'\x1b[?1006l':'\x1b[?1006h')+'\x1b[?1h\r\n'+process.argv[2]+'\r\n');
process.stdin.on('data', bytes=>{if(bytes.includes(3)){reset();process.exit(0);}
process.stdout.write('\r\n'+process.argv[2]+':'+bytes.toString('hex')+'\r\n');});
process.on('exit',reset);
`);
const browser = await chromium.launch({ headless: true,
  args: ['--enable-unsafe-swiftshader', '--use-angle=swiftshader'] });
let activePane;
async function check(name, run) {
  try { await run(); evidence.checks.push({ name, status: 'PASS' }); }
  catch (error) { evidence.checks.push({ name, status: 'FAIL', reason: error.message }); }
  fs.writeFileSync(evidencePath, JSON.stringify(evidence, null, 2));
}
try {
  for (const stylesheet of frontend.startsWith('https:') ? [false, true] : [false]) await check(`failed lazy auth ${stylesheet ? 'CSS' : 'JS'} has visible retry and recovers in the browser`, async () => {
    const context = await browser.newContext({ ignoreHTTPSErrors: true });
    try {
      const page = await context.newPage();
      const asset = stylesheet
        ? /\/assets\/AuthScreen-[^/?]+\.css(?:\?.*)?$/
        : /\/(?:assets\/)?AuthScreen(?:-[^/?]+)?\.(?:svelte|js)(?:\?.*)?$/;
      await page.route(asset, route => route.abort('failed'));
      await page.goto(frontend);
      await page.locator('[role="alert"]').waitFor({ timeout: 10_000 });
      await page.unroute(asset);
      await page.locator('[role="alert"] button').click();
      await page.locator('input[inputmode="numeric"]').waitFor({ timeout: 10_000 });
    } finally { await context.close(); }
  });
  for (const mobile of devices) {
    const context = await browser.newContext({ ignoreHTTPSErrors: true,
      viewport: mobile ? { width: 412, height: 915 } : { width: 1440, height: 1000 },
      isMobile: mobile, hasTouch: mobile });
    const page = await context.newPage();
    const pageErrors = [];
    const controlErrors = [];
    const wasmRequests = new Set();
    page.on('request', request => { if (/\.wasm(?:\?|$)/.test(request.url())) wasmRequests.add(new URL(request.url()).pathname); });
    page.on('pageerror', error => { if (pageErrors.length < 5) pageErrors.push(error.message); });
    page.on('console', message => {
      if (message.text().startsWith('[remote] roster refresh failed') && controlErrors.length < 5) controlErrors.push(message.text());
    });
    await page.addInitScript(() => {
      window.__RIDGE_E2E__ = true;
      performance.setResourceTimingBufferSize(5000);
      window.__interactionWrites = [];
      window.__interactionFrameTags = [];
      window.__interactionHistoryRequests = 0;
      window.__interactionSockets = [];
      const OriginalWebSocket = WebSocket;
      window.WebSocket = class extends OriginalWebSocket {
        constructor(...args) {
          super(...args);
          if (new URL(args[0], location.href).pathname === '/ws') window.__interactionSockets.push(this);
          this.addEventListener('message', event => {
            if (event.data instanceof ArrayBuffer) window.__interactionFrameTags.push(new Uint8Array(event.data)[0]);
          });
        }
      };
      const send = WebSocket.prototype.send;
      WebSocket.prototype.send = function (data) {
        if (typeof data === 'string') {
          try {
            const message = JSON.parse(data);
            const command = message.type === 'invoke-request' ? message.cmd : message.jsonrpc === '2.0' ? message.method : undefined;
            const args = message.type === 'invoke-request' ? message.args : message.params;
            if (command === 'get_terminal_history') window.__interactionHistoryRequests++;
            if (command === 'write_terminal_input') {
              window.__interactionWrites.push(args.input.bytes);
            }
          } catch { /* binary/other control messages are not input evidence */ }
        }
        return send.call(this, data);
      };
    });
    await page.goto(frontend);
    await page.locator('input[inputmode="numeric"]').waitFor({ timeout: 20_000 });
    const fresh = await invoke('get_remote_info');
    await page.locator('input[inputmode="numeric"]').fill(fresh.totpCode);
    await page.locator('.card button').click();
    await page.locator('.hidden-input:not([disabled])').waitFor({ timeout: 30_000 });
    await page.waitForFunction(() => window.__windE2E?.paneKeys().length > 0 || document.querySelector('[role="alert"]'));
    const initializationError = await page.locator('[role="alert"]').allTextContents();
    assert.equal(initializationError.length, 0, `terminal initialization: ${initializationError.join(' ')} ${controlErrors.join(' ')}`);
    if (process.argv.includes('--font-split')) await check('Remote renders with the smaller terminal WASM and never requests the selected-font WASM', async () => {
      const resources = await page.evaluate(() => performance.getEntriesByType('resource').filter(entry => /\.wasm(?:\?|$)/.test(entry.name)).map(entry => ({ path: new URL(entry.name).pathname, decodedBytes: entry.decodedBodySize, durationMs: entry.duration })));
      evidence.fontSplit = { requests: [...wasmRequests], resources };
      assert.ok([...wasmRequests].some(path => /ridge_term_bg/.test(path)), 'terminal WASM requested');
      assert.ok([...wasmRequests].every(path => !path.includes('ridge_term_font')), 'selected-font WASM must stay unloaded in Remote');
      const expectedBytes = fs.statSync(path.join(root, 'packages/ridge-term/pkg/ridge_term_bg.wasm')).size;
      assert.ok(resources.some(entry => entry.decodedBytes === expectedBytes), 'current terminal WASM consumed');
    });
    if (process.argv.includes('--history-only')) {
      await check('bounded semantic seed and near-top row paging on the real Host', async () => {
        const key = await page.evaluate(() => window.__windE2E.paneKeys()[0]);
        const separator = key.indexOf(':');
        const pane = { workspaceId: key.slice(0, separator), paneId: key.slice(separator + 1) };
        const marker = `ROWPAGE_${process.pid}_`;
        await invoke('write_to_pty', { ...pane, data: `node -e "for(let i=0;i<320;i++)process.stdout.write('${marker}'+i+'\\r\\n')"\r` });
        await page.waitForFunction(({ key, marker }) => window.__windE2E.visibleText(key).join('\n').includes(`${marker}319`), { key, marker });
        await page.reload();
        await page.locator('.hidden-input:not([disabled])').waitFor({ timeout: 30_000 });
        await page.waitForFunction(key => window.__windE2E?.paneKeys().includes(key) && window.__windE2E.scrollbackLen(key) > 0,
          key, { timeout: 30_000 });
        const seedRows = await page.evaluate(key => window.__windE2E.scrollbackLen(key), key);
        assert.ok(seedRows > 0 && seedRows <= 256 && seedRows < 320, `bounded seed rows=${seedRows}`);
        const stage = await page.locator('.term-stage .container').boundingBox();
        assert.ok(stage);
        await page.mouse.move(stage.x + 30, stage.y + 30);
        for (let step = 0; step < Math.ceil(seedRows / 3); step++) await page.mouse.wheel(0, -100);
        await page.waitForFunction(() => window.__interactionHistoryRequests > 0, undefined, { timeout: 10_000 });
        await page.waitForFunction(({ key, seedRows }) => window.__windE2E.scrollbackLen(key) > seedRows, { key, seedRows }, { timeout: 15_000 });
        const loadedRows = await page.evaluate(key => window.__windE2E.scrollbackLen(key), key);
        evidence.history = { seedRows, loadedRows, requests: await page.evaluate(() => window.__interactionHistoryRequests) };
        assert.ok(loadedRows <= seedRows + 128, `one bounded row page rows=${loadedRows}`);
        assert.equal(await page.locator('.scrollback-error').count(), 0);
      });
      await context.close();
      continue;
    }
    let verificationRequests = 0;
    page.on('request', request => {
      if (new URL(request.url()).pathname === '/verify') verificationRequests++;
    });
    await check(`${mobile ? 'mobile' : 'desktop'} valid session reload and unavailable validity check need no code`, async () => {
      const sessionRoute = /\/session\?/;
      await page.route(sessionRoute, route => route.abort('failed'));
      await page.reload();
      await page.locator('.hidden-input:not([disabled])').waitFor({ timeout: 30_000 });
      assert.equal(await page.locator('input[inputmode="numeric"]').count(), 0);
      assert.equal(verificationRequests, 0);
      await page.unroute(sessionRoute);
    });
    await check(`${mobile ? 'mobile' : 'desktop'} foreground recovers a silently dead socket and restores pane`, async () => {
        await page.evaluate(async () => {
          const url = performance.getEntriesByType('resource').map(entry => entry.name).find(url => /\/manager-[^/]+\.js/.test(url));
          const module = await import(url); const Manager = Object.values(module).find(value => typeof value === 'function' && typeof value.tryInstance === 'function');
          const manager = Manager.tryInstance(); window.__mountTrace = [];
          for (const name of ['park', 'unpark', 'attach']) {
            const original = manager[name]; manager[name] = function (...args) {
              const entry = this.panes.get(args[0]);
              window.__mountTrace.push({ name, connected: entry?.container.isConnected, parked: entry?.parked, expectedConnected: args[2]?.isConnected, same: entry?.container === args[2], stack: new Error().stack.split('\n').slice(1, 4) });
              return original.apply(this, args);
            };
          }
        });

      await page.evaluate(() => {
        const ws = window.__interactionSockets.at(-1);
        ws.onmessage = null;
        window.__interactionSocketCount = window.__interactionSockets.length;
        window.dispatchEvent(new Event('focus'));
      });
      await page.waitForFunction(() => window.__interactionSockets.length > window.__interactionSocketCount,
        undefined, { timeout: 20_000 });
      await page.waitForFunction(() => window.__interactionSockets.at(-1)?.readyState === WebSocket.OPEN);
      await page.locator('.conn-banner').waitFor({ state: 'detached', timeout: 30_000 });
      await page.locator('.hidden-input:not([disabled])').waitFor({ timeout: 30_000 });
      assert.equal(verificationRequests, 0);
    });
    if (process.argv.includes('--session-only')) {
      await check(`${mobile ? 'mobile' : 'desktop'} offline-online resumes without verification and renders fresh Host output`, async () => {
        try {
          await context.setOffline(true);
          await page.evaluate(() => { for (const socket of window.__interactionSockets) { if (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING) socket.close(); } });
          await page.locator('.conn-banner').waitFor({ timeout: 10_000 });
        } finally { await context.setOffline(false); }
        await page.evaluate(() => window.dispatchEvent(new Event('online')));
        await page.locator('.conn-banner').waitFor({ state: 'detached', timeout: 30_000 });
        await page.locator('.hidden-input:not([disabled])').waitFor({ timeout: 30_000 });
        const key = await page.evaluate(() => window.__windE2E.paneKeys()[0]);
        const separator = key.indexOf(':');
        const marker = `REMOTE_RESTORED_${process.pid}`;
        await invoke('write_to_pty', { workspaceId: key.slice(0, separator), paneId: key.slice(separator + 1), data: `echo '${marker}'\r` });
        await page.waitForFunction(({ key, marker }) => window.__windE2E.visibleText(key).some(line => line.trim() === marker), { key, marker });
        assert.equal(verificationRequests, 0);
      });
      await check(`${mobile ? 'mobile' : 'desktop'} authoritative invalid session clears credential and offers verification`, async () => {
        await page.evaluate(() => localStorage.setItem('ridge_remote_token', 'invalid-session-fixture'));
        await page.reload();
        await page.locator('input[inputmode="numeric"]').waitFor({ timeout: 15_000 });
        assert.equal(await page.evaluate(() => localStorage.getItem('ridge_remote_token')), null);
        assert.equal(verificationRequests, 0);
      });
      await context.close();
      continue;
    }
    const paneKey = await page.evaluate(() => window.__windE2E.paneKeys()[0]);
    const separator = paneKey.indexOf(':');
    activePane = { workspaceId: paneKey.slice(0, separator), paneId: paneKey.slice(separator + 1) };
    const ready = `RDY${process.pid}${mobile ? 'M' : 'D'}`;
    await invoke('write_to_pty', { ...activePane, data: '\x03' });
    await invoke('write_to_pty', { ...activePane, data: `node "${fixturePath}" ${ready}\r` });
    try {
      await page.waitForFunction(({ key, ready }) => window.__windE2E.visibleText(key).some(line => line.trim() === ready), { key: paneKey, ready });
    } catch (error) {
      // This instance is an isolated development kernel and this pane contains
      // only the harmless fixture command. Do not log other panes or auth data.
      console.error('fixture diagnostic', { expectedReady: ready, ...await page.evaluate(key => ({
        rows: window.__windE2E.visibleText(key).filter(Boolean).slice(-6),
        binding: document.querySelector('.container')?.dataset.bindingUncertain,
        frameTags: window.__interactionFrameTags,
      }), paneKey) });
      console.error('page errors', pageErrors);
      await page.screenshot({ path: path.join(DEV_USER_DATA_DIR, 'remote-interaction-failure.png') });
      throw error;
    }
    evidence.contexts.push({ mobile, backend: await page.evaluate(key => window.__windE2E.backendName(key), paneKey) });
    const resetWrites = () => page.evaluate(() => { window.__interactionWrites = []; });
    const writes = () => page.evaluate(() => new TextDecoder().decode(new Uint8Array(window.__interactionWrites.flat())));
    const waitWrites = () => page.waitForFunction(() => window.__interactionWrites.length > 0, undefined, { timeout: 3_000 });
    if (process.argv.includes('--gpu-recovery')) await check(`${mobile ? 'mobile' : 'desktop'} real shared WebGL context loss rebuilds resources and preserves terminal input`, async () => {
      const alternateName=`Owned GPU switch ${process.pid} ${mobile?'mobile':'desktop'}`;
      const alternate=await invoke('create_workspace',{name:alternateName});
      let originalIndex;
      try {
      for (let cycle=0;cycle<2;cycle++) {
        await page.evaluate(async key => {
          const url=performance.getEntriesByType('resource').map(entry=>entry.name).find(url=>/\/manager-[^/]+\.js/.test(url));
          const module=await import(url), Manager=Object.values(module).find(value=>typeof value==='function'&&typeof value.tryInstance==='function');
          const manager=Manager.tryInstance(), entry=manager.panes.get(key), canvas=manager.globalHost.canvas;
          if(entry.handle.backendName()!=='WebGL2')throw Error('WebGL2 backend required for actual context-loss injection');
          const extension=canvas.getContext('webgl2').getExtension('WEBGL_lose_context');
          if(!extension)throw Error('WEBGL_lose_context unavailable');
          window.__gpuRecoveryState={manager,kernel:entry.kernel,inputContext:JSON.stringify(entry.kernel.terminalInputContext()),oldHandle:entry.handle,canvas,extension,key};
          extension.loseContext();
        },paneKey);
        await page.locator('.webgpu-error').waitFor({timeout:10000});
        if(cycle===1){
          await page.locator('.tree-trigger').click();
          originalIndex=await page.locator('.ws-row').evaluateAll(rows=>rows.findIndex(row=>row.classList.contains('active')));
          assert.ok(originalIndex>=0);
          await page.locator('.ws-row').filter({has:page.locator('.ws-name',{hasText:alternateName})}).click();
          await page.waitForFunction(()=>window.__gpuRecoveryState.manager.panes.get(window.__gpuRecoveryState.key)?.parked===true);
          await page.locator('.tree-backdrop').click({position:{x:5,y:5}});
        }
        await page.evaluate(()=>window.__gpuRecoveryState.extension.restoreContext());
        if(cycle===1){
          await page.waitForFunction(()=>!window.__gpuRecoveryState.manager.hostRecoveryError);
          await page.locator('.tree-trigger').click();await page.locator('.ws-row').nth(originalIndex).click();
          await page.locator('.tree-backdrop').click({position:{x:5,y:5}});
        }
        await page.waitForFunction(()=>{
          const s=window.__gpuRecoveryState,entry=s.manager.panes.get(s.key);
          const before=JSON.parse(s.inputContext),after=entry?.kernel.terminalInputContext();
          return entry?.kernel===s.kernel && before[1]===after?.[1] && entry.handle && entry.handle!==s.oldHandle && !entry.parked && !s.manager.hostRecoveryError;
        },undefined,{timeout:30000});
        if(cycle===0)assert.equal(await page.evaluate(()=>JSON.stringify(window.__gpuRecoveryState.kernel.terminalInputContext())),await page.evaluate(()=>window.__gpuRecoveryState.inputContext));
        await page.locator('.webgpu-error').waitFor({state:'detached',timeout:10000});
        const key=String.fromCharCode(120+cycle), hex=Buffer.from(key).toString('hex');
        // Remounting may legitimately report focus; settle it before measuring this key.
        await page.locator('.hidden-input:not([disabled])').focus();
        await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
        await resetWrites();await page.locator('.hidden-input').press(key);await waitWrites();
        assert.equal(await writes(),key,'recovery must neither replay nor duplicate a keystroke');
        await page.waitForFunction(({key,marker})=>window.__windE2E.visibleText(key).join('\n').includes(marker),{key:paneKey,marker:`${ready}:${hex}`});
      }
      evidence.gpuRecovery={cycles:2,backend:'WebGL2',actualLossExtension:true,workspaceSwitchDuringLoss:true,kernelPreserved:true,epochPreserved:true,activationPreservedWithoutSwitch:true,inputRoundTrip:true};
      } finally {await invoke('switch_workspace',{workspaceId:activePane.workspaceId});await invoke('close_workspace',{workspaceId:typeof alternate==='string'?alternate:alternate.workspaceId??alternate.id});}
    });
    await check(`${mobile ? 'mobile' : 'desktop'} Ctrl+Enter kernel LF and real Host echo`, async () => {
      await resetWrites();
      await page.locator('.hidden-input').press('Control+Enter');
      await waitWrites();
      assert.equal(await writes(), '\n');
      await page.waitForFunction(({ key, marker }) => window.__windE2E.visibleText(key).join('\n').includes(marker), { key: paneKey, marker: `${ready}:0a` });
    });
    await check(`${mobile ? 'mobile' : 'desktop'} soft delete reaches Host once`, async () => {
      await resetWrites();
      await page.locator('.hidden-input').evaluate(element => {
        element.dispatchEvent(new InputEvent('beforeinput', { inputType: 'deleteContentBackward', bubbles: true, cancelable: true }));
        element.dispatchEvent(new InputEvent('input', { inputType: 'deleteContentBackward', bubbles: true }));
      });
      await waitWrites();
      assert.equal(await writes(), '\x7f');
      await page.waitForFunction(({ key, marker }) => window.__windE2E.visibleText(key).join('\n').includes(marker), { key: paneKey, marker: `${ready}:7f` });
    });
    await check(`${mobile ? 'mobile' : 'desktop'} soft newline beforeinput plus input is one Enter`, async () => {
      await resetWrites();
      await page.locator('.hidden-input').evaluate(element => {
        element.dispatchEvent(new InputEvent('beforeinput', { inputType: 'insertLineBreak', bubbles: true, cancelable: true }));
        element.value = '\n';
        element.dispatchEvent(new InputEvent('input', { inputType: 'insertLineBreak', bubbles: true }));
      });
      await waitWrites();
      assert.equal(await writes(), '\r');
      await page.waitForFunction(({ key, marker }) => window.__windE2E.visibleText(key).join('\n').includes(marker), { key: paneKey, marker: `${ready}:0d` });
    });
    if (!mobile) await check('desktop 1003 hover uses shared pointer encoder', async () => {
      const bounds = await page.locator('.term-stage .container').boundingBox();
      assert.ok(bounds);
      await resetWrites();
      await page.mouse.move(bounds.x + 40, bounds.y + 40);
      evidence.pointer = await page.evaluate(async ({ key, x, y }) => {
        const url = performance.getEntriesByType('resource').map(entry => entry.name).find(url => /\/manager-[^/]+\.js/.test(url));
        const module = await import(url);
        const Manager = Object.values(module).find(value => typeof value === 'function' && typeof value.tryInstance === 'function');
        const manager = Manager?.tryInstance(); const entry = manager?.panes.get(key);
        return { mounts: window.__mountTrace, target: document.elementFromPoint(x, y)?.className, modes: entry?.kernel.mouseReportingModes(), parked: entry?.parked, parkReason: entry?.parkReason, containerConnected: entry?.container.isConnected, currentContainer: entry?.container === document.querySelector('.term-stage .container'), lastMouseSent: entry?.lastMouseSent, selected: entry?.pointerSelectionMode, handler: !!entry?.dataHandler, rows: entry?.kernel.rows(), cols: entry?.kernel.cols() };
      }, { key: paneKey, x: bounds.x + 40, y: bounds.y + 40 });
      await waitWrites();
      assert.match(await writes(), /^\x1b\[<35;\d+;\d+M$/);
    });
    if (!mobile) await check(process.platform === 'win32'
      ? 'desktop legacy X10 transport bytes retained; Windows console child UTF-8 limitation recorded'
      : 'desktop legacy X10 non-UTF8 mouse reaches the real Host unchanged', async () => {
      try {
      // A workspace switch lets the native viewer reclaim the shared grid.
      // Explicitly claim this wide viewport before requiring a column above 95.
      await evaluate(`(async()=>{const url=performance.getEntriesByType('resource').map(item=>item.name).filter(url=>url.includes('/shared/terminal/manager.ts')).at(-1);const {TerminalManager}=await import(url);for(let count=0;count<100;count++){const entry=TerminalManager.tryInstance()?.panes.get(${JSON.stringify(activePane.paneId)});if(!entry || (entry.initialFitTimer===null && entry.pendingFitTimer===null))return;await new Promise(resolve=>setTimeout(resolve,50));}throw Error('Native workspace layout did not settle');})()`);
      await page.locator('.ctrl-btn').nth(1).click();
      await page.waitForFunction(async key=>{
        const url=performance.getEntriesByType('resource').map(entry=>entry.name).find(url=>/\/manager-[^/]+\.js/.test(url));
        const module=await import(url),Manager=Object.values(module).find(value=>typeof value==='function'&&typeof value.tryInstance==='function');
        window.__classicEntry=Manager.tryInstance().panes.get(key);
        return window.__classicEntry?.kernel.cols()>100;
      },paneKey);
      await invoke('write_to_pty', { ...activePane, data: '\x03' });
      const legacyReady = `${ready}_X10`;
      await invoke('write_to_pty', { ...activePane, data: `node "${fixturePath}" ${legacyReady} legacy\r` });
      await page.waitForFunction(({ key, ready }) => window.__windE2E.visibleText(key).some(line => line.trim() === ready), { key: paneKey, ready: legacyReady });
      await resetWrites();
      const bounds = await page.locator('.term-stage .container').boundingBox();
      await page.mouse.move(bounds.x + bounds.width - 24, bounds.y + 40);
      await page.waitForFunction(()=>window.__interactionWrites.some(bytes=>bytes[0]===27&&bytes[1]===91&&bytes[2]===77&&bytes.some(byte=>byte>=128)));
      const bytes = await page.evaluate(() => window.__interactionWrites.find(bytes=>bytes[0]===27&&bytes[1]===91&&bytes[2]===77&&bytes.some(byte=>byte>=128)));
      assert.deepEqual(bytes.slice(0, 3), [27, 91, 77]);
      assert.ok(bytes.some(byte => byte >= 128), 'fixture must exercise a non-UTF8 mouse coordinate');
      // Windows ConPTY + Node ReadConsole is a Unicode channel, not a raw byte
      // tap. A direct private PTY/base64 probe reproduces the same conversion.
      // Keep the transport-byte assertion, but do not claim arbitrary-byte
      // child delivery on Windows. Unix PTYs must preserve every byte.
      const hex = process.platform === 'win32'
        ? Buffer.from(Buffer.from(bytes).toString('utf8')).toString('hex')
        : Buffer.from(bytes).toString('hex');
      if (process.platform === 'win32') evidence.platformLimitations = ['Legacy non-UTF8 X10 coordinates are converted by the Windows console/Node child input path; arbitrary-byte child delivery remains unsupported. SGR mouse is tested separately.'];
      evidence.classicBytes = bytes;
      try {
        await page.waitForFunction(({ key, marker }) => window.__windE2E.visibleText(key).join('').includes(marker), { key: paneKey, marker: `${legacyReady}:${hex}` }, { timeout: 5_000 });
      } finally {
        evidence.classicEcho = await page.evaluate(({ key, marker }) => window.__windE2E.visibleText(key).filter(line => line.includes(marker)), { key: paneKey, marker: legacyReady });
      }
      } finally {evidence.classicProbe=await page.evaluate(()=>({cols:window.__classicEntry?.kernel.cols(),geometry:window.__classicEntry?.geometry,viewport:window.__classicEntry?.viewport,mouse:window.__classicEntry?.lastMouseSent,writes:window.__interactionWrites.slice(-8)}));}
    });
    await check(`${mobile ? 'mobile' : 'desktop'} current host default simple UI and explicit desktop`, async () => {
      const url = `https://127.0.0.1:${host.port}`;
      const simple = await context.request.get(url);
      assert.equal(simple.status(), 200);
      assert.match(await simple.text(), /\/assets\/index-/);
      const desktop = await context.request.get(`${url}/?ui=desktop`);
      assert.equal(desktop.status(), 200);
      assert.match(await desktop.text(), /_app\/immutable/);
    });
    await invoke('write_to_pty', { ...activePane, data: '\x03' });
    await page.waitForFunction(key => {
      const lines = window.__windE2E.visibleText(key).filter(line => line.trim());
      return /^PS .*?>\s*$/.test(lines.at(-1) ?? '');
    }, paneKey);
    activePane = undefined;
    await context.close();
  }
} finally {
  if (activePane) await invoke('write_to_pty', { ...activePane, data: '\x03' }).catch(() => {});
  await browser.close();
  socket.close();
  fs.writeFileSync(evidencePath, JSON.stringify(evidence, null, 2));
}
console.log(JSON.stringify(evidence, null, 2));
if (evidence.checks.some(item => item.status !== 'PASS')) process.exitCode = 1;
