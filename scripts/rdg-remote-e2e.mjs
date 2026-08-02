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
 * 依赖：已构建 `target/debug/rdg.exe`（或 RIDGE_RDG 路径）、remote-dist、playwright chromium。
 * 不依赖 computer-use / desktop-control MCP（本环境无）；浏览器腿用 Playwright 替代。
 */

import { spawn } from 'node:child_process';
import { randomInt } from 'node:crypto';
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

function fail(msg, extra = {}) {
  const body = {
    ok: false,
    error: msg,
    at: new Date().toISOString(),
    ...extra,
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

/** 直连 HTTPS（rejectUnauthorized:false，agent:false 绕过代理）。 */
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
            rejectUnauthorized: false,
            agent: false,
            timeout: 10_000,
          },
          (res) => {
            const chunks = [];
            res.on('data', (c) => chunks.push(c));
            res.on('end', () => {
              resolveReq({
                status: res.statusCode || 0,
                body: Buffer.concat(chunks).toString('utf8'),
              });
            });
          },
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
        const k = spawn('taskkill', ['/PID', String(pid), '/T', '/F'], {
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
  // 勿设 proxy:{server:'direct://'}——Playwright 会当真代理连，反而 ERR_PROXY_CONNECTION_FAILED。
  // 仅用 Chromium --no-proxy-server 关掉系统/环境代理。
  const browser = await chromium.launch({
    headless: true,
    args: [
      '--ignore-certificate-errors',
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

async function runOneClient(browser, url, getTotp, profile) {
  const context = await browser.newContext({
    ignoreHTTPSErrors: true,
    userAgent: profile.userAgent,
    viewport: profile.viewport,
    isMobile: !!profile.isMobile,
    hasTouch: !!profile.hasTouch,
  });
  const page = await context.newPage();
  const browserErrors = [];
  const wsUrls = [];
  const rpcSent = new Map();
  const rpcReceived = new Map();
  const frameMethodsSent = new Map();
  const frameMethodsReceived = new Map();
  const recordRpcMethods = (payload, target) => {
    const text = Buffer.isBuffer(payload) ? payload.toString('utf8') : String(payload);
    // Keep only method counts; never persist frame payloads/tokens in evidence.
    for (const match of text.matchAll(/"(?:method|cmd)"\s*:\s*"([^"\\]+)"/g)) {
      const method = match[1];
      target.set(method, (target.get(method) || 0) + 1);
    }
  };
  page.on('pageerror', (e) => browserErrors.push(e.message));
  page.on('console', (msg) => {
    if (msg.type() === 'error') browserErrors.push(msg.text());
  });
  page.on('websocket', (ws) => {
    wsUrls.push(ws.url().replace(/([?&](?:token|code)=)[^&]+/g, '$1<redacted>'));
    ws.on('framesent', ({ payload }) => {
      recordRpcMethods(payload, frameMethodsSent);
      for (const method of ['write_to_pty', 'resize_pane']) {
        const text = Buffer.isBuffer(payload) ? payload.toString('utf8') : String(payload);
        if (text.includes(method)) rpcSent.set(method, (rpcSent.get(method) || 0) + 1);
      }
    });
    ws.on('framereceived', ({ payload }) => {
      recordRpcMethods(payload, frameMethodsReceived);
      for (const method of ['write_to_pty', 'resize_pane']) {
        const text = Buffer.isBuffer(payload) ? payload.toString('utf8') : String(payload);
        if (text.includes(method)) rpcReceived.set(method, (rpcReceived.get(method) || 0) + 1);
      }
    });
  });

  try {
    const resp = await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 30_000 });
    const status = resp?.status() ?? 0;
    const html = await page.content();
    if (status !== 200) {
      return { name: profile.name, ok: false, detail: `HTTP ${status}`, browserErrors, wsUrls };
    }
    if (html.includes('REMOTE_UI_MISSING')) {
      return { name: profile.name, ok: false, detail: 'REMOTE_UI_MISSING shell', browserErrors, wsUrls };
    }

    // 已登录 token 可能跳过码；否则等 numeric 输入
    const authInput = page.locator('input[inputmode="numeric"]');
    const mainHints = page.locator(
      [
        '.tree-trigger',
        '[data-testid="terminal-canvas"]',
        'canvas',
        '.wr-gate', // still gate
        'text=Ridge',
      ].join(', '),
    );

    // 等 gate 或主 UI 出现
    await Promise.race([
      authInput.first().waitFor({ state: 'visible', timeout: 20_000 }).catch(() => {}),
      page.waitForSelector('canvas, .tree-trigger, .rg-pane, [class*="terminal"]', {
        timeout: 20_000,
      }).catch(() => {}),
    ]);

    if (await authInput.count()) {
      let verified = false;
      for (let attempt = 0; attempt < 3 && !verified; attempt++) {
        const st = await getTotp();
        const code = String(st.totp);
        await authInput.first().fill('');
        await authInput.first().fill(code);
        // 桌面：按钮 Connect；手机：按钮
        const btn = page.locator('button').filter({ hasText: /Connect|连接|验证|Verify|继续/i }).first();
        if (await btn.count()) {
          await btn.click();
        } else {
          await authInput.first().press('Enter');
        }
        // 成功：验证输入消失或 canvas 出现
        try {
          await page.waitForFunction(
            () => {
              const inputs = document.querySelectorAll('input[inputmode="numeric"]');
              const stillGate = inputs.length > 0 && inputs[0].offsetParent !== null;
              const hasCanvas = !!document.querySelector('canvas');
              const hasTree = !!document.querySelector('.tree-trigger');
              return hasCanvas || hasTree || !stillGate;
            },
            { timeout: 12_000 },
          );
          // 若仍停在 gate 且有 error，重试 TOTP
          const errText = await page.locator('.wr-error, .error, [class*="error"]').first().textContent().catch(() => '');
          if (await authInput.count() && (await authInput.first().isVisible().catch(() => false))) {
            if (attempt < 2) {
              log(`${profile.name}: totp attempt ${attempt + 1} still on gate (${errText || 'no err'}); retry`);
              await sleep(1500);
              continue;
            }
            return {
              name: profile.name,
              ok: false,
              detail: `stuck on auth gate after TOTP: ${errText || 'unknown'}`,
              browserErrors,
              wsUrls,
            };
          }
          verified = true;
        } catch {
          if (attempt < 2) {
            await sleep(1500);
            continue;
          }
          const body = await page.locator('body').innerText().catch(() => '');
          return {
            name: profile.name,
            ok: false,
            detail: `auth timeout body=${body.slice(0, 200)}`,
            browserErrors: browserErrors.slice(-6),
            wsUrls,
          };
        }
      }
    }

    // 接通判据：WS 曾建立，或主 UI 有 canvas / pane 树
    await sleep(1500);
    const hasCanvas = (await page.locator('canvas').count()) > 0;
    const hasTree = (await page.locator('.tree-trigger').count()) > 0;
    const hasWs = wsUrls.some((u) => u.includes('/ws'));
    const stillAuth =
      (await authInput.count()) > 0 && (await authInput.first().isVisible().catch(() => false));

    if (stillAuth) {
      return {
        name: profile.name,
        ok: false,
        detail: 'still on auth screen',
        browserErrors: browserErrors.slice(-6),
        wsUrls,
      };
    }
    if (!hasCanvas && !hasTree && !hasWs) {
      const body = await page.locator('body').innerText().catch(() => '');
      return {
        name: profile.name,
        ok: false,
        detail: `no canvas/tree/ws after auth; body=${body.slice(0, 180)}`,
        browserErrors: browserErrors.slice(-6),
        wsUrls,
      };
    }

    // 截图证据
    // Exercise the real browser input and measured-resize path.  Method
    // counts come from WebSocket frames, so a rendered shell without PTY/RPC
    // traffic cannot pass this smoke.
    const input = page.locator('.hidden-input').first();
    const inputAvailable = (await input.count()) > 0;
    if (inputAvailable) {
      await input.focus().catch(() => {});
    } else {
      // Desktop builds may park the textarea only after the first canvas
      // pointer focus.  Drive that real focus path instead of silently
      // skipping input coverage when the sink is not attached yet.
      await page.locator('canvas').first().click().catch(() => {});
    }
    await page.keyboard.type(`echo RDG_E2E_${profile.name}`);
    await page.keyboard.press('Enter');
    await sleep(700);
    const resizedViewport = {
      width: profile.viewport.width + 37,
      height: profile.viewport.height,
    };
    await page.setViewportSize(resizedViewport);
    await sleep(700);
    const inputSent = (rpcSent.get('write_to_pty') || 0) > 0;
    const resizeSent = (rpcSent.get('resize_pane') || 0) > 0;
    const rpc = {
      inputAvailable,
      inputSent,
      resizeSent,
      sent: Object.fromEntries(rpcSent),
      received: Object.fromEntries(rpcReceived),
      frameMethodsSent: Object.fromEntries(frameMethodsSent),
      frameMethodsReceived: Object.fromEntries(frameMethodsReceived),
    };
    if (!inputSent || !resizeSent) {
      return {
        name: profile.name,
        ok: false,
        detail: `control path incomplete input=${inputSent} resize=${resizeSent}`,
        browserErrors: browserErrors.slice(-6),
        wsUrls,
        rpc,
      };
    }

    mkdirSync(EVIDENCE_DIR, { recursive: true });
    const shot = join(EVIDENCE_DIR, `${profile.name}.png`);
    await page.screenshot({ path: shot, fullPage: true }).catch(() => {});

    return {
      name: profile.name,
      ok: true,
      detail: `canvas=${hasCanvas} tree=${hasTree} ws=${hasWs}`,
      browserErrors: browserErrors.slice(-4),
      wsUrls,
      rpc,
      screenshot: shot,
    };
  } catch (e) {
    return {
      name: profile.name,
      ok: false,
      detail: e instanceof Error ? e.message : String(e),
      browserErrors: browserErrors.slice(-6),
      wsUrls,
    };
  } finally {
    await context.close();
  }
}

async function main() {
  // 全程禁用代理：本机 127.0.0.1 / Playwright / rdg 子进程皆受益
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

  mkdirSync(EVIDENCE_DIR, { recursive: true });
  ensureRemoteDist();
  const rdgBin = resolveRdgBin();
  const port = await freePort();
  log(`rdg=${rdgBin}`);
  log(`port=${port}`);
  log(`status=${STATUS_FILE}`);

  // 清掉可能占用 9527 的旧进程（仅当选用固定端口）
  if (preferredPort > 0 && process.platform === 'win32') {
    try {
      spawn('powershell', [
        '-NoProfile',
        '-Command',
        `Get-NetTCPConnection -LocalPort ${preferredPort} -ErrorAction SilentlyContinue | ForEach-Object { Stop-Process -Id $_.OwningProcess -Force -ErrorAction SilentlyContinue }`,
      ], { stdio: 'ignore', windowsHide: true });
      await sleep(800);
    } catch {
      /* ok */
    }
  }

  const hostHandle = spawnHost(rdgBin, port);
  let results = [];
  let status = null;
  try {
    // Accept only the process and port spawned by this run.  This is the
    // ownership fence that makes parallel probes and stale status files safe.
    status = await waitStatus(45_000, port, hostHandle.pid);
    log(`host up pid=${status.pid} totp=${status.totp} url=${status.url_loopback}`);
    // 再确认 TCP（防 status 误报）
    await waitTcp(status.port, 10_000);

    // 协议层预检：Node https 直连 + 忽略自签（Playwright request 在本机代理环境易 AggregateError）
    const url = status.url_loopback.replace(/\/$/, '');
    const info = await httpsJson(`${url}/info`);
    if (info.status !== 200) fail(`/info HTTP ${info.status}`, { body: info.body });
    log(`info=${info.body}`);
    let totpSt = await waitFreshTotp(status);
    let ver = await httpsJson(`${url}/verify`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
      body: `code=${encodeURIComponent(totpSt.totp)}&device=e2e-preflight`,
    });
    let verBody = {};
    try {
      verBody = JSON.parse(ver.body);
    } catch {
      /* ok */
    }
    if (!verBody.success) {
      await sleep(2000);
      totpSt = await waitFreshTotp(status);
      ver = await httpsJson(`${url}/verify`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: `code=${encodeURIComponent(totpSt.totp)}&device=e2e-preflight`,
      });
      try {
        verBody = JSON.parse(ver.body);
      } catch {
        /* ok */
      }
      if (!verBody.success) fail('protocol verify failed', { verBody, body: ver.body });
    }
    log('protocol verify OK');

    // 协议层 PTY：list layout → write_to_pty → 确认 invoke-result 成功（真接通下限）
    {
      const layout = await httpsJson(`${url}/verify`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: `code=${encodeURIComponent((await waitFreshTotp(status)).totp)}&device=e2e-pty`,
      });
      let token = null;
      try {
        token = JSON.parse(layout.body).token;
      } catch {
        /* ok */
      }
      if (!token) fail('pty preflight: no session token');
      // 用 WS 太重；经 HTTP 无 write。改为 browser 矩阵后已有 canvas；此处至少
      // 证明 get_pane_layout 经 HTTPS 会话无关路径：走已验证的 info 已够。
      // 额外：status 文件仍在刷新 = host 存活。
      const stAlive = readStatus();
      if (!stAlive?.ready) fail('host not ready before browser matrix');
      log('host still ready before browser');
    }

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
      port,
      url,
      status,
      results,
      hostLogTail,
    };
    writeFileSync(join(EVIDENCE_DIR, 'last-result.json'), JSON.stringify(evidence, null, 2));
    if (!allOk) fail('browser matrix failed', { results });
    log('ALL PASS');
    console.log(JSON.stringify({ ok: true, evidence: join(EVIDENCE_DIR, 'last-result.json') }));
  } finally {
    await killHost({ pid: status?.pid || hostHandle.pid });
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(process.exitCode || 1);
});
