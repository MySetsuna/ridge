#!/usr/bin/env node
/**
 * 无人值守 rdg LAN Remote E2E（REQ-RDG-REMOTE-CONNECT-01）
 *
 * 流程（agent 可单命令跑完，无需人启动 TUI / 手输 TOTP）：
 *   1. 释放/选用端口，spawn `rdg tui --lan-host`
 *   2. 轮询 `.ridge/lan-host-status.json` 取 TOTP + URL
 *   3. Playwright Chromium（ignoreHTTPSErrors + 禁用代理）开桌面/手机 UA
 *   4. 填 TOTP → 等主界面/WS 接通信号
 *   5. 写证据 JSON，杀 host，非 0 退出码 = 失败
 *
 * 用法：
 *   pnpm e2e:rdg-lan
 *   node scripts/rdg-remote-e2e.mjs [--port 9527] [--skip-build]
 *
 * 依赖：已构建 `target/debug/rdg.exe`（或 RIDGE_RDG 路径）、remote-dist、playwright chromium；
 * 若受限环境无法下载 Playwright 浏览器，可设 RIDGE_E2E_CHROMIUM_EXECUTABLE 指向本机 Chromium/Chrome。
 * 不依赖 computer-use / desktop-control MCP（本环境无）；浏览器腿用 Playwright 替代。
 */

import { spawn } from 'node:child_process';
import { randomInt, X509Certificate } from 'node:crypto';
import { createServer } from 'node:net';
import {
  mkdirSync,
  readFileSync,
  writeFileSync,
  existsSync,
  unlinkSync,
  openSync,
  closeSync,
} from 'node:fs';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as sleep } from 'node:timers/promises';
import { systemTool } from './lib/toolPath.mjs';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, '..');
// Keep concurrent probes isolated.  A shared status path lets a stale/parallel
// rdg process overwrite the URL while this run is polling it (the browser then
// connects to the wrong port and the test reports a misleading connection
// refusal).  Explicit paths remain supported for operators that need them.
const STATUS_FILE = resolve(
  process.env.RIDGE_LAN_STATUS_FILE ||
    join(ROOT, '.ridge', `lan-host-status-e2e-${process.pid}.json`),
);
const EVIDENCE_DIR = resolve(ROOT, '.iteration', 'artifacts', 'rdg-remote-e2e');
const args = new Set(process.argv.slice(2));
const portArgIdx = process.argv.indexOf('--port');
const preferredPort =
  portArgIdx >= 0 ? Number(process.argv[portArgIdx + 1]) : Number(process.env.RIDGE_LAN_PORT || 0);
const BROWSER_SAFE_PORT_MIN = 20_000;
const BROWSER_SAFE_PORT_MAX = 29_999;

function log(msg) {
  console.log(`[rdg-remote-e2e] ${msg}`);
}

function redactForEvidence(value) {
  if (Array.isArray(value)) return value.map(redactForEvidence);
  if (value && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value).map(([key, nested]) => {
        const sensitive = /(?:token|totp|secret|password|passphrase|authorization|cookie|code)/i.test(key);
        return [key, sensitive ? '<redacted>' : redactForEvidence(nested)];
      }),
    );
  }
  if (typeof value === 'string') {
    return value
      .replace(/(TOTP\s*[:=]\s*)\d{6}/gi, '$1<redacted>')
      .replace(/([?&](?:token|totp|code)=)[^&\s]+/gi, '$1<redacted>');
  }
  return value;
}

function fail(msg, extra = {}) {
  const body = {
    ok: false,
    error: msg,
    at: new Date().toISOString(),
    ...redactForEvidence(extra),
  };
  mkdirSync(EVIDENCE_DIR, { recursive: true });
  writeFileSync(join(EVIDENCE_DIR, 'last-result.json'), JSON.stringify(body, null, 2));
  console.error(`[rdg-remote-e2e] FAIL ${msg}`);
  process.exitCode = 1;
  throw new Error(msg);
}

async function freePort() {
  if (preferredPort > 0) return preferredPort;
  // OS-assigned ephemeral ports may fall on Chromium's blocked-port list
  // (for example 3659), producing ERR_UNSAFE_PORT before Ridge is contacted.
  // Probe a high range above the Fetch bad-port list and below common ephemeral
  // ranges so browser reachability and collision avoidance are both explicit.
  const width = BROWSER_SAFE_PORT_MAX - BROWSER_SAFE_PORT_MIN + 1;
  const start = randomInt(width);
  for (let offset = 0; offset < width; offset += 1) {
    const candidate = BROWSER_SAFE_PORT_MIN + ((start + offset) % width);
    const available = await new Promise((resolveAvailable) => {
      const server = createServer();
      server.once('error', () => resolveAvailable(false));
      server.listen(candidate, '127.0.0.1', () => {
        server.close(() => resolveAvailable(true));
      });
    });
    if (available) return candidate;
  }
  throw new Error('no browser-safe LAN test port available');
}

/** 直连 HTTPS；仅信任显式配置的 Ridge CA，未配置时保持系统校验。 */
function httpsJson(url, opts = {}) {
  return import('node:https').then(
    (https) =>
      new Promise((resolveReq, rejectReq) => {
        const u = new URL(url);
        const req = https.request(
          {
            hostname: u.hostname,
            port: u.port || 443,
            path: `${u.pathname}${u.search}`,
            method: opts.method || 'GET',
            headers: opts.headers || {},
            ca: opts.ca ?? (process.env.RIDGE_REMOTE_CA_CERT
              ? readFileSync(process.env.RIDGE_REMOTE_CA_CERT)
              : undefined),
            rejectUnauthorized: opts.rejectUnauthorized ?? true,
            agent: false,
            timeout: 10_000,
          },
          (res) => resolveHttpsResponse(res, resolveReq),
        );
        req.on('error', rejectReq);
        req.on('timeout', () => {
          req.destroy();
          rejectReq(new Error(`https timeout ${url}`));
        });
        if (opts.body) req.write(opts.body);
        req.end();
      }),
  );
}

/**
 * The E2E owns the loopback port and the spawned rdg process, so it can perform
 * a single trust-on-first-use fetch of Ridge's public local CA.  Validate that
 * payload before using it as the trust anchor for every protocol probe.  This
 * does not weaken production TLS or any non-loopback request.
 */
async function loadLoopbackRidgeCa(url) {
  const parsed = new URL(url);
  if (!['127.0.0.1', 'localhost', '::1'].includes(parsed.hostname)) {
    throw new Error(`refusing CA bootstrap from non-loopback host: ${parsed.hostname}`);
  }
  const response = await httpsJson(`${url}/ridge-ca.pem`, { rejectUnauthorized: false });
  if (response.status !== 200 || !response.body.includes('BEGIN CERTIFICATE')) {
    throw new Error(`Ridge CA bootstrap failed: HTTP ${response.status}`);
  }
  const certificate = new X509Certificate(response.body);
  if (!certificate.ca || !certificate.subject.includes('CN=Ridge Remote Local CA')) {
    throw new Error(`Ridge CA bootstrap returned an unexpected certificate: ${certificate.subject}`);
  }
  return response.body;
}

async function waitTcp(port, timeoutMs = 15_000) {
  const start = Date.now();
  const net = await import('node:net');
  while (Date.now() - start < timeoutMs) {
    const ok = await new Promise((resolveOk) => {
      const s = net.createConnection({ host: '127.0.0.1', port }, () => {
        s.end();
        resolveOk(true);
      });
      s.on('error', () => resolveOk(false));
      s.setTimeout(500, () => {
        s.destroy();
        resolveOk(false);
      });
    });
    if (ok) return;
    await sleep(150);
  }
  fail(`TCP ${port} not open`);
}

function resolveRdgBin() {
  if (process.env.RIDGE_RDG && existsSync(process.env.RIDGE_RDG)) return process.env.RIDGE_RDG;
  const candidates = [
    join(ROOT, 'target', 'debug', 'rdg.exe'),
    join(ROOT, 'target', 'release', 'rdg.exe'),
    join(ROOT, 'target', 'debug', 'rdg'),
    join(ROOT, 'target', 'release', 'rdg'),
  ];
  const hit = candidates.find((p) => existsSync(p));
  if (!hit) fail('rdg binary missing; build with: cargo build -p ridge-cli');
  return hit;
}

function ensureRemoteDist() {
  const desk = join(ROOT, 'remote-dist', 'desktop', 'index.html');
  const mob = join(ROOT, 'remote-dist', 'mobile', 'index.html');
  if (!existsSync(desk) || !existsSync(mob)) {
    fail('remote-dist missing desktop/mobile; run: pnpm build:remote', { desk, mob });
  }
}

function readStatus() {
  try {
    return JSON.parse(readFileSync(STATUS_FILE, 'utf8'));
  } catch {
    return null;
  }
}

async function waitStatus(timeoutMs = 45_000, expectedPort = 0, expectedPid = 0) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    const st = readStatus();
    if (
      st?.ready === true &&
      st?.totp &&
      st?.port &&
      st?.url_loopback &&
      (!expectedPort || Number(st.port) === expectedPort) &&
      (!expectedPid || Number(st.pid) === expectedPid)
    ) {
      return st;
    }
    await sleep(200);
  }
  fail('timeout waiting for ready lan-host-status.json', {
    statusFile: STATUS_FILE,
    expectedPort,
    expectedPid,
    last: readStatus(),
  });
}

async function waitFreshTotp(st0, timeoutMs = 35_000) {
  // status 每秒刷新；取一次即可。若 verify 失败可再读。
  const st = readStatus() || st0;
  if (!st?.totp || String(st.totp).length !== 6) fail('invalid totp in status', { st });
  return st;
}

function spawnHost(rdgBin, port) {
  try {
    unlinkSync(STATUS_FILE);
  } catch {
    /* ok */
  }
  mkdirSync(EVIDENCE_DIR, { recursive: true });
  const hostLog = join(EVIDENCE_DIR, 'host.log');
  try {
    unlinkSync(hostLog);
  } catch {
    /* ok */
  }
  const env = {
    ...process.env,
    RIDGE_REMOTE_UI_ROOT: join(ROOT, 'remote-dist'),
    RIDGE_LAN_STATUS_FILE: STATUS_FILE,
    // 禁止子进程继承把局域网送进代理的环境（本机浏览器/curl 的同类坑）
    https_proxy: '',
    HTTPS_PROXY: '',
    http_proxy: '',
    HTTP_PROXY: '',
    ALL_PROXY: '',
    all_proxy: '',
    no_proxy: '*',
    NO_PROXY: '*',
    RUST_LOG: process.env.RUST_LOG || 'ridge_cli=info,ridge_remote=info',
  };
  // Windows：管道 stdio 易导致 rdg 秒退；改 detached + 日志文件，生命周期跟 status.pid。
  const outFd = openSync(hostLog, 'a');
  const child = spawn(rdgBin, ['tui', '--lan-host', '--port', String(port)], {
    cwd: ROOT,
    env,
    stdio: ['ignore', outFd, outFd],
    detached: true,
    windowsHide: true,
  });
  child.unref();
  try {
    closeSync(outFd);
  } catch {
    /* ok */
  }
  return { child, hostLog, pid: child.pid };
}

async function killHost(handle) {
  const pid = handle?.pid || handle?.child?.pid;
  if (!pid) {
    try {
      unlinkSync(STATUS_FILE);
    } catch {
      /* ok */
    }
    return;
  }
  try {
    if (process.platform === 'win32') {
      await new Promise((resolveKill) => {
        const k = spawn(systemTool('taskkill'), ['/PID', String(pid), '/T', '/F'], {
          stdio: 'ignore',
          windowsHide: true,
        });
        k.on('exit', () => resolveKill());
        k.on('error', () => resolveKill());
      });
    } else {
      try {
        process.kill(pid, 'SIGTERM');
      } catch {
        /* ignore */
      }
    }
  } catch {
    /* ignore */
  }
  await sleep(400);
  try {
    unlinkSync(STATUS_FILE);
  } catch {
    /* ok */
  }
}

async function runBrowserMatrix(url, getTotp) {
  // Chromium 会读进程环境代理；启动前清空，否则 ERR_PROXY_CONNECTION_FAILED。
  for (const k of [
    'HTTP_PROXY',
    'HTTPS_PROXY',
    'http_proxy',
    'https_proxy',
    'ALL_PROXY',
    'all_proxy',
  ]) {
    delete process.env[k];
  }
  process.env.NO_PROXY = '*';
  process.env.no_proxy = '*';

  const { chromium } = await import('@playwright/test');
  const executablePath = process.env.RIDGE_E2E_CHROMIUM_EXECUTABLE?.trim() || undefined;
  // 勿设 proxy:{server:'direct://'}——Playwright 会当真代理连，反而 ERR_PROXY_CONNECTION_FAILED。
  // 仅用 Chromium --no-proxy-server 关掉系统/环境代理。
  const browser = await chromium.launch({
    headless: true,
    executablePath,
    args: [
      '--ignore-certificate-errors',
      // This matrix is also the controlled clean-profile comparison for the
      // mobile `runtime.lastError` attribution gate.  Do not let an installed
      // extension or component extension make a product warning look owned
      // by Remote.
      '--disable-extensions',
      '--disable-component-extensions-with-background-pages',
      '--no-proxy-server',
      '--proxy-bypass-list=<-loopback>;*',
    ],
  });

  const matrix = [
    {
      name: 'desktop',
      userAgent:
        'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36',
      viewport: { width: 1280, height: 800 },
      isMobile: false,
    },
    {
      name: 'mobile',
      userAgent:
        'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1',
      viewport: { width: 390, height: 844 },
      isMobile: true,
      hasTouch: true,
    },
  ];

  const results = [];
  try {
    for (const m of matrix) {
      const result = await runOneClient(browser, url, getTotp, m);
      results.push(result);
      log(`${m.name}: ${result.ok ? 'PASS' : 'FAIL'} ${result.detail || ''}`);
      if (!result.ok) break;
    }
  } finally {
    await browser.close();
  }
  return results;
}

function clientFailure(profile, detail, telemetry, extra = {}) {
  return {
    name: profile.name,
    ok: false,
    detail,
    browserErrors: telemetry.browserErrors.slice(-6),
    wsUrls: telemetry.wsUrls,
    ...extra,
  };
}

function installClientTelemetry(page) {
  const telemetry = {
    browserErrors: [],
    wsUrls: [],
    rpcSent: new Map(),
    rpcReceived: new Map(),
    frameMethodsSent: new Map(),
    frameMethodsReceived: new Map(),
  };
  const recordRpcMethods = (payload, target) => {
    const text = Buffer.isBuffer(payload) ? payload.toString('utf8') : String(payload);
    for (const match of text.matchAll(/"(?:method|cmd)"\s*:\s*"([^"\\]+)"/g)) {
      target.set(match[1], (target.get(match[1]) || 0) + 1);
    }
  };
  page.on('pageerror', (error) => telemetry.browserErrors.push(error.message));
  page.on('console', (message) => {
    if (message.type() === 'error') telemetry.browserErrors.push(message.text());
  });
  page.on('websocket', (socket) => {
    telemetry.wsUrls.push(socket.url().replace(/([?&](?:token|code)=)[^&]+/g, '$1<redacted>'));
    for (const [event, target, methods] of [
      ['framesent', telemetry.frameMethodsSent, telemetry.rpcSent],
      ['framereceived', telemetry.frameMethodsReceived, telemetry.rpcReceived],
    ]) {
      socket.on(event, ({ payload }) => {
        recordRpcMethods(payload, target);
        const text = Buffer.isBuffer(payload) ? payload.toString('utf8') : String(payload);
        for (const method of ['write_to_pty', 'resize_pane']) {
          if (text.includes(method)) methods.set(method, (methods.get(method) || 0) + 1);
        }
      });
    }
  });
  return telemetry;
}

async function waitForClientAuth(page, authInput) {
  await Promise.race([
    authInput.first().waitFor({ state: 'visible', timeout: 20_000 }).catch(() => {}),
    page.waitForSelector('canvas, .tree-trigger, .rg-pane, [class*="terminal"]', { timeout: 20_000 }).catch(() => {}),
  ]);
}

async function submitClientAuthAttempt(page, profile, getTotp, authInput, telemetry, attempt) {
  const st = await getTotp();
  await authInput.first().fill(String(st.totp));
  const button = page.locator('button').filter({ hasText: /Connect|连接|验证|Verify|继续/i }).first();
  if (await button.count()) await button.click();
  else await authInput.first().press('Enter');
  try {
    await page.waitForFunction(() => {
      const inputs = document.querySelectorAll('input[inputmode="numeric"]');
      const stillGate = inputs.length > 0 && inputs[0].offsetParent !== null;
      return !!document.querySelector('canvas, .tree-trigger') || !stillGate;
    }, { timeout: 12_000 });
    const errorText = await page.locator('.wr-error, .error, [class*="error"]').first().textContent().catch(() => '');
    const stillOpen = await authInput.count() > 0 && await authInput.first().isVisible().catch(() => false);
    if (!stillOpen) return null;
    if (attempt < 2) {
      log(profile.name + ': TOTP attempt ' + (attempt + 1) + ' still on gate (' + (errorText || 'no err') + '); retry');
      await sleep(1500);
      return 'retry';
    }
    return clientFailure(profile, 'stuck on auth gate after TOTP: ' + (errorText || 'unknown'), telemetry);
  } catch {
    if (attempt < 2) {
      await sleep(1500);
      return 'retry';
    }
    const body = await page.locator('body').innerText().catch(() => '');
    return clientFailure(profile, 'auth timeout body=' + body.slice(0, 200), telemetry);
  }
}

async function authenticateClient(page, profile, getTotp, authInput, telemetry) {
  await waitForClientAuth(page, authInput);
  if (!(await authInput.count())) return null;
  for (let attempt = 0; attempt < 3; attempt += 1) {
    const result = await submitClientAuthAttempt(page, profile, getTotp, authInput, telemetry, attempt);
    if (result !== 'retry') return result;
  }
  return clientFailure(profile, 'auth attempts exhausted', telemetry);
}


async function collectClientEvidence(page, profile, telemetry) {
  await sleep(1500);
  const hasCanvas = (await page.locator('canvas').count()) > 0;
  const hasTree = (await page.locator('.tree-trigger').count()) > 0;
  const hasWs = telemetry.wsUrls.some((url) => url.includes('/ws'));
  const authInput = page.locator('input[inputmode="numeric"]');
  if (await authInput.count() && await authInput.first().isVisible().catch(() => false)) {
    return clientFailure(profile, 'still on auth screen', telemetry);
  }
  if (!hasCanvas && !hasTree && !hasWs) {
    const body = await page.locator('body').innerText().catch(() => '');
    return clientFailure(profile, `no canvas/tree/ws after auth; body=${body.slice(0, 180)}`, telemetry);
  }
  const input = page.locator('[data-rg-pane-active="true"] .rg-ime-helper, .rg-ime-helper, .hidden-input').first();
  const inputAvailable = (await input.count()) > 0;
  if (inputAvailable) await input.focus().catch(() => {});
  else await page.locator('canvas').first().click().catch(() => {});
  await page.keyboard.type(`echo RDG_E2E_${profile.name}`);
  await page.keyboard.press('Enter');
  await sleep(700);
  await page.setViewportSize({ width: profile.viewport.width + 37, height: profile.viewport.height });
  await sleep(700);
  const inputSent = (telemetry.rpcSent.get('write_to_pty') || 0) > 0;
  const resizeSent = (telemetry.rpcSent.get('resize_pane') || 0) > 0;
  let paneSwitch = { attempted: false, ok: true };
  if (profile.isMobile) {
    const tree = page.locator('.tree-trigger').first();
    if (!(await tree.count())) {
      paneSwitch = { attempted: true, ok: false, reason: 'mobile pane switch controls missing' };
    } else {
      await tree.click();
      const paneNew = page.locator('.pane-new').first();
      if (!(await paneNew.count())) {
        paneSwitch = { attempted: true, ok: false, reason: 'mobile pane creation control missing' };
      } else {
        let phase = 'count-before';
        try {
          const before = await page.locator('.pane-row').count();
          phase = 'create-pane';
          await paneNew.click();
          phase = 'wait-new-row';
          await page.waitForFunction(
            (expected) => document.querySelectorAll('.pane-row').length > expected,
            before,
            { timeout: 12_000 },
          );
          const after = await page.locator('.pane-row').count();
          const newIndex = after - 1;
          phase = 'select-new-row';
          await page.locator('.pane-row').nth(newIndex).click();
          phase = 'wait-new-close';
          await page.waitForFunction(
            () => !document.querySelector('.tree-popup'),
            undefined,
            { timeout: 5_000 },
          );
          phase = 'reopen-tree-new';
          await tree.click();
          phase = 'wait-new-active';
          await page.waitForFunction(
            (expected) => {
              const rows = [...document.querySelectorAll('.pane-row')];
              return rows.filter((row) => row.classList.contains('active')).length === 1
                && rows.findIndex((row) => row.classList.contains('active')) === expected;
            },
            newIndex,
            { timeout: 12_000 },
          );
          phase = 'select-first-row';
          await page.locator('.pane-row').first().click();
          phase = 'wait-first-close';
          await page.waitForFunction(
            () => !document.querySelector('.tree-popup'),
            undefined,
            { timeout: 5_000 },
          );
          phase = 'reopen-tree-first';
          await tree.click();
          phase = 'wait-first-active';
          await page.waitForFunction(
            () => {
              const rows = [...document.querySelectorAll('.pane-row')];
              return rows.filter((row) => row.classList.contains('active')).length === 1
                && rows.findIndex((row) => row.classList.contains('active')) === 0;
            },
            undefined,
            { timeout: 12_000 },
          );
          paneSwitch = { attempted: true, ok: true, before, after };
        } catch (error) {
          const rows = await page.locator('.pane-row').count().catch(() => -1);
          const activeRows = await page.locator('.pane-row.active').count().catch(() => -1);
          paneSwitch = {
            attempted: true,
            ok: false,
            reason: `${phase}: ${error instanceof Error ? error.message : String(error)}`,
            rows,
            activeRows,
          };
        }
      }
    }
  }
  const rpc = {
    inputAvailable, inputSent, resizeSent,
    paneSwitch,
    sent: Object.fromEntries(telemetry.rpcSent),
    received: Object.fromEntries(telemetry.rpcReceived),
    frameMethodsSent: Object.fromEntries(telemetry.frameMethodsSent),
    frameMethodsReceived: Object.fromEntries(telemetry.frameMethodsReceived),
  };
  if (!inputSent || !resizeSent) return clientFailure(profile, `control path incomplete input=${inputSent} resize=${resizeSent}`, telemetry, { rpc });
  if (!paneSwitch.ok) return clientFailure(profile, `pane switch incomplete: ${paneSwitch.reason}`, telemetry, { rpc });
  if (telemetry.browserErrors.length > 0) {
    return clientFailure(profile, `browser errors: ${telemetry.browserErrors.slice(-3).join(' | ')}`, telemetry, { rpc });
  }
  mkdirSync(EVIDENCE_DIR, { recursive: true });
  const screenshot = join(EVIDENCE_DIR, `${profile.name}.png`);
  await page.screenshot({ path: screenshot, fullPage: true }).catch(() => {});
  return {
    name: profile.name, ok: true,
    detail: `canvas=${hasCanvas} tree=${hasTree} ws=${hasWs}`,
    browserErrors: telemetry.browserErrors.slice(-4), wsUrls: telemetry.wsUrls, rpc, screenshot,
  };
}

async function runOneClient(browser, url, getTotp, profile) {
  const context = await browser.newContext({
    ignoreHTTPSErrors: true,
    userAgent: profile.userAgent,
    viewport: profile.viewport,
    isMobile: !!profile.isMobile,
    hasTouch: !!profile.hasTouch,
  });
  const page = await context.newPage();
  const telemetry = installClientTelemetry(page);

  try {
    const resp = await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 30_000 });
    const status = resp?.status() ?? 0;
    const html = await page.content();
    if (status !== 200) return clientFailure(profile, `HTTP ${status}`, telemetry);
    if (html.includes('REMOTE_UI_MISSING')) return clientFailure(profile, 'REMOTE_UI_MISSING shell', telemetry);
    const authInput = page.locator('input[inputmode="numeric"]');
    const authFailure = await authenticateClient(page, profile, getTotp, authInput, telemetry);
    if (authFailure) return authFailure;
    return await collectClientEvidence(page, profile, telemetry);
  } catch (e) {
    return {
      name: profile.name,
      ok: false,
      detail: e instanceof Error ? e.message : String(e),
      browserErrors: telemetry.browserErrors.slice(-6),
      wsUrls: telemetry.wsUrls,
    };
  } finally {
    await context.close();
  }
}

function disableProxyEnvironment() {
  for (const key of ['HTTP_PROXY', 'HTTPS_PROXY', 'http_proxy', 'https_proxy', 'ALL_PROXY', 'all_proxy']) {
    delete process.env[key];
  }
  process.env.NO_PROXY = '*';
  process.env.no_proxy = '*';
}

async function clearPreferredPort() {
  if (preferredPort <= 0 || process.platform !== 'win32') return;
  try {
    spawn(systemTool('powershell'), [
      '-NoProfile', '-Command',
      `Get-NetTCPConnection -LocalPort ${preferredPort} -ErrorAction SilentlyContinue | ForEach-Object { Stop-Process -Id $_.OwningProcess -Force -ErrorAction SilentlyContinue }`,
    ], { stdio: 'ignore', windowsHide: true });
    await sleep(800);
  } catch {
    // Best effort; the selected port is still fenced by waitStatus.
  }
}

async function prepareHost(hostHandle, port) {
  const status = await waitStatus(45_000, port, hostHandle.pid);
  log(`host up pid=${status.pid} totp=<redacted> url=${status.url_loopback}`);
  await waitTcp(status.port, 10_000);
  const url = status.url_loopback.replace(/\/$/, '');
  const ca = await loadLoopbackRidgeCa(url);
  const info = await httpsJson(`${url}/info`, { ca });
  if (info.status !== 200) fail(`/info HTTP ${info.status}`, { body: info.body });
  log(`info=${info.body}`);
  let totpSt = await waitFreshTotp(status);
  let ver = await httpsJson(`${url}/verify`, {
    ca,
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: `code=${encodeURIComponent(totpSt.totp)}&device=e2e-preflight`,
  });
  let verBody = {};
  try { verBody = JSON.parse(ver.body); } catch { /* malformed response handled below */ }
  if (!verBody.success) {
    await sleep(2000);
    totpSt = await waitFreshTotp(status);
    ver = await httpsJson(`${url}/verify`, {
      ca,
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
      body: `code=${encodeURIComponent(totpSt.totp)}&device=e2e-preflight`,
    });
    try { verBody = JSON.parse(ver.body); } catch { /* malformed response handled below */ }
    if (!verBody.success) fail('protocol verify failed', { verBody, body: ver.body });
  }
  log('protocol verify OK');
  const layout = await httpsJson(`${url}/verify`, {
    ca,
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: `code=${encodeURIComponent((await waitFreshTotp(status)).totp)}&device=e2e-pty`,
  });
  let token = null;
  try { token = JSON.parse(layout.body).token; } catch { /* malformed response handled below */ }
  if (!token) fail('pty preflight: no session token');
  if (!readStatus()?.ready) fail('host not ready before browser matrix');
  log('host still ready before browser');
  return { status, url };
}

async function main() {
  disableProxyEnvironment();

  mkdirSync(EVIDENCE_DIR, { recursive: true });
  ensureRemoteDist();
  const rdgBin = resolveRdgBin();
  const port = await freePort();
  log(`rdg=${rdgBin}`);
  log(`port=${port}`);
  log(`status=${STATUS_FILE}`);

  await clearPreferredPort();

  const hostHandle = spawnHost(rdgBin, port);
  let results = [];
  let status = null;
  let url = '';
  try {
    ({ status, url } = await prepareHost(hostHandle, port));

    results = await runBrowserMatrix(url, async () => waitFreshTotp(status));
    const allOk = results.length > 0 && results.every((r) => r.ok);
    let hostLogTail = '';
    try {
      hostLogTail = readFileSync(hostHandle.hostLog, 'utf8').slice(-4000);
    } catch {
      /* ok */
    }
    const evidence = {
      ok: allOk,
      at: new Date().toISOString(),
      browserIsolation: {
        extensionsDisabled: true,
        isolatedContext: true,
      },
      port,
      url,
      status: redactForEvidence(status),
      results: redactForEvidence(results),
      hostLogTail: redactForEvidence(hostLogTail),
    };
    writeFileSync(join(EVIDENCE_DIR, 'last-result.json'), JSON.stringify(evidence, null, 2));
    if (!allOk) fail('browser matrix failed', { results });
    log('ALL PASS');
    console.log(JSON.stringify({ ok: true, evidence: join(EVIDENCE_DIR, 'last-result.json') }));
  } finally {
    await killHost({ pid: status?.pid || hostHandle.pid });
  }
}

try {
  await main();
} catch (e) {
  console.error(e);
  process.exit(process.exitCode || 1);
}

function resolveHttpsResponse(res, resolveReq) {
  const chunks = [];
  res.on('data', (chunk) => chunks.push(chunk));
  res.on('end', () => resolveReq({
    status: res.statusCode || 0,
    body: Buffer.concat(chunks).toString('utf8'),
  }));
}
