import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs/promises';
import { once } from 'node:events';
import { createServer } from 'node:net';
import path from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';
import { chromium } from '@playwright/test';
import { cargoTool, pnpmInvocation, systemTool } from './lib/toolPath.mjs';

const repo = path.resolve(import.meta.dirname, '..');
const artifacts = path.join(repo, 'artifacts', 'remote-startup-input-e2e');
await fs.mkdir(artifacts, { recursive: true });
const root = await fs.mkdtemp(path.join(artifacts, 'run-'));
const statusPath = path.join(root, 'lan-status.json');
const markerPath = path.join(root, 'command-result.txt');
const marker = 'REMOTE_STARTUP_INPUT_COMPLETE_中文';
const command = `Write-Output '${marker}' | Add-Content -Encoding UTF8 -LiteralPath '${markerPath.replaceAll("'", "''")}'`;
const evidence = { scope: 'Real Chromium, production mobile Remote and owned LAN Kernel/PTY; attach/context availability delayed by client test doubles', checks: [] };
let kernel, host, browser, endpoint;

function run(command, args) {
  const result = spawnSync(command, args, { cwd: repo, stdio: 'inherit', windowsHide: true });
  assert.equal(result.status, 0, `build failed: ${path.basename(command)}`);
}

async function until(probe, timeout = 20000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (await probe()) return;
    await sleep(25);
  }
  throw Error('owned startup input probe timed out');
}

async function readJson(file) {
  try { return JSON.parse(await fs.readFile(file, 'utf8')); } catch { return null; }
}

async function stop(child) {
  if (!child || child.exitCode !== null) return;
  if (process.platform === 'win32') {
    const result = spawn(systemTool('taskkill'), ['/PID', String(child.pid), '/T', '/F'], { windowsHide: true, stdio: 'ignore' });
    await new Promise(resolve => { result.once('exit', resolve); result.once('error', resolve); });
  } else child.kill('SIGTERM');
}

try {
  assert.equal(process.platform, 'win32', 'owned shell fixture currently requires Windows');
  const pnpm = pnpmInvocation();
  run(pnpm.command, [...pnpm.args, 'build:remote:mobile']);
  run(cargoTool('cargo'), ['build', '--locked', '-p', 'ridge-kernel', '--bin', 'ridge-kernel']);
  run(cargoTool('cargo'), ['build', '--locked', '-p', 'ridge-cli', '--bin', 'ridge']);
  const cli = path.join(root, 'ridge-cli.exe');
  await fs.copyFile(path.join(repo, 'target', 'debug', 'ridge.exe'), cli);
  const port = await new Promise((resolve, reject) => {
    const socket = createServer();
    socket.once('error', reject);
    socket.listen(0, '127.0.0.1', () => { const value = socket.address().port; socket.close(() => resolve(value)); });
  });
  const env = { ...process.env, RIDGE_KERNEL_DATA_DIR: root, RIDGE_KERNEL_FS_ROOT: root,
    RIDGE_REMOTE_UI_ROOT: path.join(repo, 'remote-dist'), RIDGE_LAN_STATUS_FILE: statusPath,
    RIDGE_AUTH_FILE: path.join(root, 'auth.json'), RIDGE_TEST_TLS_DIR: path.join(root, 'tls'),
    NO_PROXY: '*', no_proxy: '*', RUST_LOG: 'error' };
  kernel = spawn(path.join(repo, 'target', 'debug', 'ridge-kernel.exe'), [], {
    cwd: root, env, windowsHide: true, stdio: 'ignore',
  });
  await once(kernel, 'spawn');
  await until(async () => {
    assert.equal(kernel.exitCode, null, 'owned Kernel exited');
    endpoint = await readJson(path.join(root, 'kernel.json'));
    return endpoint?.pid === kernel.pid;
  });
  host = spawn(cli, ['tui', '--lan-host', '--port', String(port), '--cwd', root, '--shell', systemTool('powershell')], {
    cwd: root, env, windowsHide: true, stdio: 'ignore',
  });
  await once(host, 'spawn');
  let status;
  await until(async () => {
    assert.equal(host.exitCode, null, 'owned LAN host exited');
    status = await readJson(statusPath);
    return status?.ready && status.pid === host.pid && Number(status.port) === port;
  }, 45000);
  const assets = await fs.readdir(path.join(repo, 'remote-dist', 'mobile', 'assets'));
  const managerAsset = assets.find(name => /^manager-.*\.js$/.test(name));
  assert.ok(managerAsset, 'mobile manager asset missing');
  browser = await chromium.launch({ channel: 'chrome', headless: true,
    args: ['--ignore-certificate-errors', '--no-proxy-server', '--disable-extensions'] });
  const page = await browser.newPage({ ignoreHTTPSErrors: true, viewport: { width: 390, height: 844 },
    userAgent: 'Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 Chrome/130 Mobile Safari/537.36' });
  const writes = [];
  const commandBytes = () => new TextDecoder().decode(Uint8Array.from(writes)).replace(/\x1b\[[IO]/g, '');
  page.on('websocket', socket => socket.on('framesent', event => {
    try {
      const frame = JSON.parse(String(event.payload));
      if (frame.method === 'write_terminal_input') writes.push(...frame.params.input.bytes);
      if (frame.type === 'invoke-request' && frame.cmd === 'write_terminal_input') writes.push(...frame.args.input.bytes);
    } catch { /* binary frames carry output */ }
  }));
  await page.goto(status.url_loopback + '/?ui=mobile', { waitUntil: 'domcontentloaded' });
  await page.evaluate(async asset => {
    const exports = await import('/assets/' + asset);
    const Class = Object.values(exports).find(value => typeof value === 'function' && value.instance && value.prototype.debugGeometry);
    if (!Class) throw Error('production manager export not found');
    const manager = Class.instance();
    const probe = window.__startupInputProbe = { contextReady: false, attached: false, frames: 0, manager };
    const attach = manager.attach.bind(manager);
    manager.attach = async (...args) => {
      await new Promise(resolve => { probe.releaseAttach = resolve; });
      const result = await attach(...args);
      const kernel = manager.getKernel(args[0]);
      const readContext = kernel.terminalInputContext.bind(kernel);
      kernel.terminalInputContext = () => probe.contextReady ? readContext() : null;
      return result;
    };
    const onData = manager.onData.bind(manager);
    manager.onData = (...args) => { const result = onData(...args); probe.attached = true; return result; };
    const apply = manager.applyRemoteFrame.bind(manager);
    manager.applyRemoteFrame = (...args) => { const result = apply(...args); probe.frames++; return result; };
  }, managerAsset);
  await page.locator('input[inputmode="numeric"]').fill(String(status.totp));
  await page.getByRole('button').filter({ hasText: /Connect|连接|验证|Verify|继续/i }).first().click();
  const input = page.locator('.hidden-input');
  await input.waitFor({ state: 'attached', timeout: 20000 });
  await input.focus();
  await page.keyboard.type(command[0]);
  await page.waitForFunction(() => typeof window.__startupInputProbe.releaseAttach === 'function');
  await page.evaluate(() => window.__startupInputProbe.releaseAttach());
  await page.waitForFunction(() => window.__startupInputProbe.attached);
  await input.focus();
  const imeStart = command.indexOf('中文');
  await page.keyboard.type(command.slice(1, imeStart), { delay: 2 });
  await input.dispatchEvent('compositionstart');
  await input.dispatchEvent('compositionupdate', { data: '中文' });
  await input.dispatchEvent('compositionend', { data: '中文' });
  await input.evaluate(element => {
    element.value = '中文';
    element.dispatchEvent(new InputEvent('input', { bubbles: true, data: '中文', inputType: 'insertCompositionText' }));
  });
  await page.keyboard.type(command.slice(imeStart + '中文'.length), { delay: 2 });
  await page.keyboard.press('Enter');
  assert.equal(writes.length, 0, 'input reached the PTY before canonical binding readiness');
  evidence.checks.push('Zero PTY input before the canonical input context becomes available');
  await page.evaluate(() => { window.__startupInputProbe.contextReady = true; });
  await page.getByRole('button', { name: '锁定渲染尺寸到本端并刷新', exact: true }).click();
  await until(async () => (await fs.readFile(markerPath, 'utf8').catch(() => '')).trim() === marker);
  assert.equal(commandBytes(), command + '\r');
  evidence.checks.push('First character, post-attach text, IME commit with trailing duplicate input, and Enter reached the real PTY once in original order');
  const frames = await page.evaluate(() => window.__startupInputProbe.frames);
  await page.getByRole('button', { name: '锁定渲染尺寸到本端并刷新', exact: true }).click();
  await page.waitForFunction(before => window.__startupInputProbe.frames > before, frames);
  assert.deepEqual((await fs.readFile(markerPath, 'utf8')).trim().split(/\r?\n/), [marker]);
  assert.equal(commandBytes(), command + '\r');
  evidence.focusReports = (new TextDecoder().decode(Uint8Array.from(writes)).match(/\x1b\[[IO]/g) ?? []).length;
  evidence.checks.push('Another canonical refresh neither replayed the buffered command nor executed it twice');
  assert.equal(await page.locator('.input-error').count(), 0);
  evidence.result = 'PASS';
} catch (error) {
  evidence.result = 'FAIL';
  evidence.error = String(error);
  process.exitCode = 1;
} finally {
  await browser?.close();
  await stop(host);
  if (endpoint?.pid === kernel?.pid) {
    await fetch(`http://127.0.0.1:${endpoint.port}/v1/shutdown`, { method: 'POST',
      headers: { 'x-ridge-kernel-token': endpoint.token, 'x-ridge-token': endpoint.token },
      signal: AbortSignal.timeout(5000) }).catch(() => {});
    for (let attempt = 0; attempt < 100 && kernel.exitCode === null; attempt++) await sleep(50);
  }
  await stop(kernel);
  evidence.artifactDirectory = root;
  await fs.writeFile(path.join(root, 'evidence.json'), JSON.stringify(evidence, null, 2));
  console.log(JSON.stringify(evidence, null, 2));
}
