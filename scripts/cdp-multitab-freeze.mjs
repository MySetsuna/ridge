#!/usr/bin/env node
// CDP e2e：把「开两个以上工作区 tab → 整个前端交互面板卡死」变成可复现的数字，
// 并直接指出是**哪个 JS 函数**在占主线程（CPU profile 按 self-time 归并）。
//
// 为什么必须是 e2e：这个 bug 不在任何单个纯函数里，而在「挂载几十个 pane 之后
// 主线程还剩多少」这件事上。单测量不到，只有真开 tab 才现形。
//
// 用法：
//   终端 1: pnpm tauri:dev:cdp
//   终端 2: node scripts/cdp-multitab-freeze.mjs [tabs]     (默认 3)
//
// 输出：每个阶段的事件循环延迟（p50/p95/max）、longtask 统计、以及卡顿窗口内
// CPU profile 的热调用链。**门禁**：出现 >2s 的 long task、点「+」没能新增工作区、
// 或页面抛出未捕获异常 → 退出码 1。
import http from 'node:http';
import { resolveCdpPort } from './cdp-port.mjs';

const CDP_PORT = Number(process.env.CDP_PORT ?? resolveCdpPort());
const TABS = Number(process.argv[2] ?? 3);
const log = (...a) => console.log('[freeze]', ...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function httpJson(path) {
  return new Promise((resolve, reject) => {
    http
      .get({ host: '127.0.0.1', port: CDP_PORT, path, timeout: 5000 }, (res) => {
        let b = '';
        res.on('data', (c) => (b += c));
        res.on('end', () => {
          try {
            resolve(JSON.parse(b));
          } catch (e) {
            reject(e);
          }
        });
      })
      .on('error', reject);
  });
}

class Cdp {
  constructor(url) {
    this.ws = new WebSocket(url);
    this.id = 0;
    this.pend = new Map();
  }
  open() {
    this.events = [];
    return new Promise((res, rej) => {
      this.ws.onopen = () => res();
      this.ws.onerror = (e) => rej(new Error('CDP ws error: ' + (e.message || e.type)));
      this.ws.onmessage = (ev) => {
        const m = JSON.parse(ev.data);
        if (m.id && this.pend.has(m.id)) {
          this.pend.get(m.id)(m);
          this.pend.delete(m.id);
        } else if (m.method) {
          this.events.push(m);
        }
      };
    });
  }
  send(method, params = {}) {
    const id = ++this.id;
    return new Promise((res) => {
      this.pend.set(id, res);
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }
  async ev(expression, { awaitPromise = false } = {}) {
    const r = await this.send('Runtime.evaluate', {
      expression,
      awaitPromise,
      returnByValue: true,
    });
    if (r.result?.exceptionDetails) {
      throw new Error('eval threw: ' + JSON.stringify(r.result.exceptionDetails).slice(0, 600));
    }
    return r.result?.result?.value;
  }
  close() {
    try {
      this.ws.close();
    } catch {}
  }
}

async function findTarget() {
  for (let i = 0; i < 45; i++) {
    try {
      const list = await httpJson('/json/list');
      const t = list.find((x) => x.type === 'page' && !/devtools/.test(x.url || ''));
      if (t) return t;
    } catch {
      /* retry */
    }
    await sleep(2000);
  }
  throw new Error('no Ridge page target on :' + CDP_PORT);
}

/** 页内埋点：事件循环延迟采样 + longtask 观察器。 */
const INSTRUMENT = `(() => {
  if (window.__rgLag) { window.__rgLag.reset(); return 'reused'; }
  const s = { samples: [], longtasks: [], t0: performance.now() };
  const TICK = 50;
  let last = performance.now();
  setInterval(() => {
    const now = performance.now();
    s.samples.push(Math.max(0, now - last - TICK));
    last = now;
  }, TICK);
  try {
    new PerformanceObserver((l) => {
      for (const e of l.getEntries()) s.longtasks.push(Math.round(e.duration));
    }).observe({ entryTypes: ['longtask'] });
  } catch {}
  window.__rgLag = {
    reset() { s.samples.length = 0; s.longtasks.length = 0; },
    read() {
      const a = s.samples.slice().sort((x, y) => x - y);
      const q = (p) => (a.length ? Math.round(a[Math.floor((a.length - 1) * p)]) : 0);
      const lt = s.longtasks.slice().sort((x, y) => y - x);
      return {
        n: a.length, p50: q(0.5), p95: q(0.95), max: Math.round(a[a.length - 1] ?? 0),
        longtasks: lt.length, ltMax: lt[0] ?? 0, ltSum: lt.reduce((x, y) => x + y, 0),
      };
    },
  };
  return 'installed';
})()`;

/** 页内探针：当前工作区数 / 已挂载 pane 容器数 / manager 里的 pane 条目数。 */
const PROBE = `(() => {
  const tabs = document.querySelectorAll('[data-rg-ws-pane-host]').length;
  const panes = document.querySelectorAll('.rg-pane-container').length;
  const canvases = document.querySelectorAll('canvas').length;
  return { mountedWorkspaces: tabs, mountedPanes: panes, canvases };
})()`;

// Cold Tauri + WebGPU startup can exceed 30s while the dev sidecar and Vite
// graph warm up. Keep probing instead of turning a cold-start race into a
// multitab regression.
async function waitForMounted(cdp, maxMs = 90_000) {
  const deadline = Date.now() + maxMs;
  let last = null;
  while (Date.now() < deadline) {
    last = await cdp.ev(PROBE);
    if (last?.mountedWorkspaces > 0 && last.mountedPanes > 0 && last.canvases > 0) return last;
    await sleep(250);
  }
  throw new Error(`Ridge page did not mount a workspace: ${JSON.stringify(last)}`);
}

async function clickNewWorkspace(cdp, maxMs = 20_000) {
  const deadline = Date.now() + maxMs;
  while (Date.now() < deadline) {
    const clicked = await cdp.ev(
      `(() => {
         const b = [...document.querySelectorAll('button')]
           .find((x) => x.textContent.trim() === '+' && x.className.includes('border-dashed'));
         if (!b) return 'no-button';
         b.click();
         return 'clicked';
       })()`,
    );
    if (clicked === 'clicked') return clicked;
    await sleep(250);
  }
  return 'no-button';
}

/** 把 CPU profile 归并成「谁在烧主线程」——按**调用链**看，而非只看叶子。
 *  叶子往往是 `get_stack` 这类通用 helper，真正的责任人在上游几帧。 */
function hotPaths(profile, topN = 12) {
  const byId = new Map(profile.nodes.map((n) => [n.id, n]));
  const parent = new Map();
  for (const n of profile.nodes) for (const c of n.children ?? []) parent.set(c, n.id);
  const total = profile.samples.length || 1;
  const self = new Map();
  for (const id of profile.samples) self.set(id, (self.get(id) ?? 0) + 1);

  const fmt = (id) => {
    const f = byId.get(id)?.callFrame ?? {};
    const file = (f.url || '').replace(/^.*\/(?=[^/]+$)/, '').split('?')[0];
    const location = file ? ` @${file}:${(f.lineNumber ?? 0) + 1}` : '';
    return `${f.functionName || '(anon)'}${location}`;
  };
  // 每个叶子往上取 6 层，形成一条可读的责任链。
  const chains = new Map();
  for (const [id, hits] of self) {
    const chain = [];
    let cur = id;
    for (let i = 0; i < 6 && cur != null; i++) {
      chain.push(fmt(cur));
      cur = parent.get(cur);
    }
    const key = chain.join('  ←  ');
    chains.set(key, (chains.get(key) ?? 0) + hits);
  }
  return [...chains.entries()]
    .map(([key, hits]) => ({ pct: (hits / total) * 100, key }))
    .sort((a, b) => b.pct - a.pct)
    .slice(0, topN);
}

async function inspectTab(cdp, index, wsCounts, longtaskMax, errorsSeen) {
  await cdp.ev(INSTRUMENT);
  await cdp.ev('window.__rgLag.reset()');
  await cdp.send('Profiler.start');
  const clicked = await clickNewWorkspace(cdp);
  log(`tab ${index}: ${clicked}`);
  await sleep(6000);
  await cdp.ev(INSTRUMENT);
  const prof = await cdp.send('Profiler.stop');
  const lag = await cdp.ev('window.__rgLag.read()');
  const probe = await cdp.ev(PROBE);
  wsCounts.push(probe.mountedWorkspaces);
  longtaskMax.push(lag.ltMax ?? 0, lag.max ?? 0);
  log(`AFTER tab ${index} → lag ${JSON.stringify(lag)}`);
  log(`AFTER tab ${index} → dom  ${JSON.stringify(probe)}`);
  const rows = hotPaths(prof.result?.profile ?? { nodes: [], samples: [] });
  log(`AFTER tab ${index} → hot call chains (leaf ← caller ← …):`);
  for (const row of rows) {
    if (row.pct >= 1.5) console.log(`         ${row.pct.toFixed(1).padStart(5)}%  ${row.key}`);
  }
  for (const event of cdp.events) {
    if (event.method === 'Runtime.exceptionThrown') {
      errorsSeen.push((event.params.exceptionDetails?.text ?? '') +
        ' ' + (event.params.exceptionDetails?.exception?.description ?? '').slice(0, 200));
    }
  }
  const messages = cdp.events.filter((event) => event.method === 'Runtime.consoleAPICalled');
  cdp.events.length = 0;
  const tally = new Map();
  for (const message of messages) {
    const head = (message.params.args ?? [])
      .map((arg) => arg.value ?? arg.description ?? '')
      .join(' ')
      .toString()
      .slice(0, 240);
    const key = `${message.params.type}: ${head}`;
    tally.set(key, (tally.get(key) ?? 0) + 1);
  }
  log(`AFTER tab ${index} → console messages: ${messages.length}`);
  for (const [key, count] of [...tally.entries()].sort((a, b) => b[1] - a[1]).slice(0, 8)) {
    console.log(`         ×${String(count).padStart(5)}  ${key}`);
  }
}

const main = async () => {
  const t = await findTarget();
  log('target:', t.title, '|', t.url);
  const cdp = new Cdp(t.webSocketDebuggerUrl);
  await cdp.open();
  await cdp.send('Runtime.enable');
  await cdp.send('Profiler.enable');
  await cdp.send('Profiler.setSamplingInterval', { interval: 200 });

  const wsCounts = [];
  const longtaskMax = [];
  const errorsSeen = [];

  log('instrument:', await cdp.ev(INSTRUMENT));
  const base = await waitForMounted(cdp);
  wsCounts.push(base.mountedWorkspaces);
  log('baseline probe:', JSON.stringify(base));

  await sleep(3000);
  log('BASELINE lag:', JSON.stringify(await cdp.ev('window.__rgLag.read()')));

  for (let i = 2; i <= TABS; i++) await inspectTab(cdp, i, wsCounts, longtaskMax, errorsSeen);

  // 卡顿是否持续：不再操作，静置观察。
  await cdp.ev(INSTRUMENT);
  await cdp.ev('window.__rgLag.reset()');
  await sleep(8000);
  log('IDLE-after lag:', JSON.stringify(await cdp.ev('window.__rgLag.read()')));

  // ── 门禁判定 ──
  // 1) 每次点「+」都必须真的多出一个工作区。iter-62 的自循环抛错后，整个子树
  //    响应性即死，「+」点了毫无反应——所以「计数没涨」本身就是卡死的判据。
  // 2) 任何一次 >LONGTASK_LIMIT 的 long task 视为回归（修复前实测 15,016ms）。
  const LONGTASK_LIMIT = 2000;
  const grew = wsCounts.every((c, i) => i === 0 || c > wsCounts[i - 1]);
  const worst = Math.max(0, ...longtaskMax);
  log(`GATE: workspace counts ${wsCounts.join(' → ')} | worst longtask ${worst}ms`);
  if (!grew) {
    console.error(`[freeze] FAIL: 点「+」没能新增工作区（${wsCounts.join(' → ')}）——交互已僵死`);
    process.exitCode = 1;
  }
  if (worst > LONGTASK_LIMIT) {
    console.error(`[freeze] FAIL: 出现 ${worst}ms 的 long task（上限 ${LONGTASK_LIMIT}ms）`);
    process.exitCode = 1;
  }
  if (errorsSeen.length) {
    console.error(`[freeze] FAIL: 页面抛出未捕获异常：\n  ${errorsSeen.join('\n  ')}`);
    process.exitCode = 1;
  }
  if (!process.exitCode) log('GATE: PASS');

  cdp.close();
};

try {
  await main();
} catch (e) {
  console.error('[freeze] ERROR', e.message);
  process.exit(1);
}
