// Real Chromium + production native RPC/event facades + owned existing ACP fixture.
// Native grant issuance is supplied by the owned fixture; no external Agent claim.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import net from 'node:net';
import { chromium } from '@playwright/test';

const root = fs.mkdtempSync(path.join(os.tmpdir(), 'ridge-070-permission-ui-'));
const kernelBinary = path.resolve(process.env.RIDGE_TEST_KERNEL_BINARY || 'target/debug/ridge-kernel.exe');
const fixtureBinary = process.env.RIDGE_TEST_COMMUNE_BINARY ? path.resolve(process.env.RIDGE_TEST_COMMUNE_BINARY)
  : fs.readdirSync('target/debug/deps').filter(name => /^commune_registration-[a-f0-9]+(?:\.exe)?$/.test(name))
    .map(name => path.resolve('target/debug/deps', name)).sort((a, b) => fs.statSync(b).mtimeMs - fs.statSync(a).mtimeMs)[0];
assert.ok(fixtureBinary, 'build the commune_registration integration fixture first');
let devUrl = process.env.RIDGE_TEST_DEV_URL;
const output = path.resolve('.webview2-dev-cdp/070-commune-permission-ui.json');
const screenshot = path.resolve('.webview2-dev-cdp/070-commune-permission-ui.png');
const evidence = { scope: 'Owned Kernel/PTY ACP protocol fixture + real Chromium production approval component; fixture supplies OS bootstrap grant', completeChange: false, checks: [] };
let endpoint, browser, page, kernel, vite, paneId, identity;
const pageErrors = [];
const until = async condition => {
  const deadline = Date.now() + 15000;
  while (Date.now() < deadline) { if (await condition()) return; await new Promise(resolve => setTimeout(resolve, 25)); }
  throw Error('owned fixture readiness timeout');
};
const call = async (method, url, body) => {
  const response = await fetch(`http://127.0.0.1:${endpoint.port}${url}`, {
    method, headers: { 'x-ridge-kernel-token': endpoint.token, 'content-type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined, signal: AbortSignal.timeout(6000),
  });
  assert.ok(response.ok, `Kernel HTTP ${response.status}`);
  return response.json();
};
try {
  if (!devUrl) {
    const port = await new Promise((resolve, reject) => {
      const listener = net.createServer(); listener.on('error', reject);
      listener.listen(0, '127.0.0.1', () => { const port = listener.address().port; listener.close(() => resolve(port)); });
    });
    devUrl = `http://127.0.0.1:${port}`;
    const log = fs.openSync(path.join(root, 'vite.log'), 'a');
    vite = spawn(process.execPath, ['node_modules/vite/bin/vite.js'], { windowsHide: true,
      env: { ...process.env, RIDGE_WEB_REMOTE: '', RIDGE_NATIVE_RUNTIME: '1', RIDGE_DEV_SERVER_PORT: String(port) }, stdio: ['ignore', log, log] });
    fs.closeSync(log);
    await until(async () => { assert.equal(vite.exitCode, null, 'owned Vite exited'); return fetch(devUrl + '/@vite/client', { signal: AbortSignal.timeout(1000) }).then(response => response.ok, () => false); });
  }
  assert.ok(['localhost', '127.0.0.1'].includes(new URL(devUrl).hostname), 'owned test needs a local Vite server');
  assert.ok((await fetch(devUrl + '/@vite/client', { signal: AbortSignal.timeout(5000) })).ok, 'local Vite server must be running');
  kernel = spawn(kernelBinary, [], { windowsHide: true, env: { ...process.env, RIDGE_KERNEL_DATA_DIR: root, RIDGE_AUTH_FILE: path.join(root, 'auth.json') }, stdio: 'ignore' });
  await until(() => {
    assert.equal(kernel.exitCode, null, 'owned Kernel exited');
    if (!fs.existsSync(path.join(root, 'kernel.json'))) return false;
    endpoint = JSON.parse(fs.readFileSync(path.join(root, 'kernel.json'))); return true;
  });
  assert.equal(endpoint.pid, kernel.pid);
  const workspaceId = (await call('POST', '/v1/domain/workspaces')).workspace_id;
  paneId = crypto.randomUUID();
  const descriptorPath = path.join(root, 'existing-endpoint.json');
  await call('POST', '/v1/domain/ptys', { pty_id: paneId, workspace_id: workspaceId, program: fixtureBinary,
    args: ['--exact', 'existing_protocol_endpoint_fixture', '--ignored'], cwd: root,
    env: { RIDGE_COMMUNE_FIXTURE: descriptorPath }, role: 'shell', cols: 80, rows: 24 });
  await until(() => fs.existsSync(descriptorPath));
  const descriptor = JSON.parse(fs.readFileSync(descriptorPath));
  const registered = await call('POST', '/v1/domain/commune/register', { workspaceId, paneId,
    process: descriptor.process, endpointOwner: descriptor.process, endpoint: descriptor.endpoint, sessionId: descriptor.sessionId, cwd: root });
  assert.equal(registered.ok, true, registered.error);
  identity = registered.value;
  const grant = await call('POST', '/v1/local/clients', { deviceId: 'owned-approval-window', origin: new URL(devUrl).origin });
  browser = await chromium.launch({ headless: true });
  page = await browser.newPage({ viewport: { width: 900, height: 700 } });
  page.setDefaultTimeout(15000);
  await page.route('**/__commune_permission_ui', route => route.fulfill({ contentType: 'text/html', body: '<!doctype html><title>Owned Commune permission UI</title><body><main id="component"></main></body>' }));
  await page.exposeFunction('fixtureGrant', () => grant);
  page.on('pageerror', error => pageErrors.push(error.message));
  await page.goto(devUrl + '/__commune_permission_ui');
  const target = { workspaceId, paneId, agentId: identity.agentId, generation: identity.generation, lease: identity.lease };
  await page.evaluate(async target => {
    const osCalls = [];
    window.isTauri = true;
    window.__TAURI_INTERNALS__ = { invoke: async command => {
      osCalls.push(command); if (command === 'bootstrap_runtime_client') return window.fixtureGrant();
      throw Error('Unexpected OS business call: ' + command);
    }, transformCallback: () => { throw Error('Unexpected native Channel'); } };
    const [{ connectPresentationRuntime }, { bridge }, { invoke }, { mount, unmount }, { default: Modal }] = await Promise.all([
      import('/src/lib/transport/runtimeClient.ts'), import('/src/lib/transport/tauriShim/bridge.ts'),
      import('/src/lib/transport/nativeRuntimeCore.ts'), import('/scripts/commune-permission-ui-client.mjs'),
      import('/src/lib/teammate/HitlApprovalModal.svelte'),
    ]);
    const connection = await connectPresentationRuntime();
    bridge.attach(connection.transport, { useGlobalWorkspace: false });
    const [{ initThemeSystem, getThemeIds }, { applyTheme }] = await Promise.all([
      import('/src/lib/stores/themes.ts'), import('/src/lib/stores/settings.ts'),
    ]);
    await initThemeSystem(); applyTheme(getThemeIds()[0]);
    const modal = mount(Modal, { target: document.getElementById('component') });
    window.ownedApproval = { invoke, target, osCalls, dispose: async () => { await unmount(modal); bridge.detach(); connection.disconnect(); } };
  }, target);
  const receipt = await page.evaluate(async () => window.ownedApproval.invoke('send_agent_message', { ...window.ownedApproval.target, message: 'permission-needed', idempotency_key: crypto.randomUUID() }));
  const dialog = page.getByRole('alertdialog');
  await dialog.waitFor();
  await dialog.getByRole('button', { name: 'Allow once', exact: true }).waitFor();
  await dialog.getByRole('button', { name: 'Allow always', exact: true }).waitFor();
  assert.equal(await dialog.getByRole('button', { name: '修改', exact: true }).count(), 0);
  assert.ok((await dialog.textContent()).includes('private-argument'), 'native reviewer must see actual tool input');
  await page.screenshot({ path: screenshot });
  await dialog.getByRole('button', { name: 'Allow once', exact: true }).click();
  await until(async () => await dialog.count() === 0);
  await until(async () => {
    const value = await page.evaluate(async deliveryId => window.ownedApproval.invoke('get_agent_message_delivery_status', { ...window.ownedApproval.target, deliveryId }), receipt.deliveryId);
    return value.execution?.state === 'turn_ended';
  });
  const calls = await page.evaluate(() => window.ownedApproval.osCalls);
  assert.deepEqual(calls, ['bootstrap_runtime_client']);
  await page.evaluate(() => window.ownedApproval.dispose());
  assert.deepEqual(pageErrors, [], 'production component reported browser errors');
  const ptys = await call('GET', '/v1/domain/ptys');
  assert.ok(ptys.ptys.some(pty => pty.id === paneId && pty.child_pid === descriptor.process.pid));
  assert.deepEqual(JSON.parse(fs.readFileSync(path.join(root, 'permission-decisions'), 'utf8').trim()), { optionId: 'once', outcome: 'selected' });
  evidence.checks = ['Actual peer options appear in the production modal with actual tool input', 'No Modify action for standard ACP permissions',
    'A real click resolves the original permission ID and the original prompt ends', 'Business RPC and permission events use Runtime; OS IPC only issues bootstrap grant',
    'Unmounting the presentation client preserves the exact existing Agent process'];
  evidence.screenshot = screenshot; evidence.result = 'PASS';
} catch (error) {
  evidence.result = 'FAIL'; evidence.error = error.message; evidence.pageErrors = pageErrors;
  if (page) {
    evidence.browser = await page.evaluate(async () => ({ body: document.body.innerText,
      osCalls: window.ownedApproval?.osCalls, pendingCount: window.ownedApproval ? (await window.ownedApproval.invoke('list_hitl_pending_local', {})).length : null })).catch(failure => ({ error: failure.message }));
  }
  process.exitCode = 1;
}
finally {
  await browser?.close();
  if (endpoint && endpoint.pid === kernel?.pid) await call('POST', '/v1/shutdown').catch(() => {});
  if (vite && vite.exitCode === null) {
    if (process.platform === 'win32') await new Promise(resolve => spawn('taskkill', ['/PID', String(vite.pid), '/T', '/F'], { windowsHide: true, stdio: 'ignore' }).once('exit', resolve));
    else vite.kill('SIGTERM');
  }
  fs.mkdirSync(path.dirname(output), { recursive: true }); fs.writeFileSync(output, JSON.stringify(evidence, null, 2) + '\n');
}
console.log(JSON.stringify(evidence, null, 2));
