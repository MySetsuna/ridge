#!/usr/bin/env node
// CDP-driven CHARACTERIZATION e2e for pane-tree CRUD + its broadcast
// (D11 Wave A gate — see docs/plans/d11-workspace-graph-pane-decoupling-design.md §1.5 G).
//
// Captures the CURRENT desktop behaviour that the P2 `WorkspaceGraph` surgery
// MUST preserve, so the refactor can be proven non-regressive against it:
//
//   1. tree mutation   — `split_pane` adds exactly one leaf; `close_pane` of the
//      new leaf returns to baseline. Read authoritatively via `get_pane_layout`
//      (the pane tree, which `ridge_core::workspace::pane_tree` owns) — NOT the
//      LAN `panes` list, which counts panes-with-PTYs (a headless CDP split has
//      no PTY until a frontend mounts it).
//   2. broadcast firing — each CRUD re-pushes a `{"type":"panes"}` frame to a
//      connected LAN-WS subscriber (the `PanesChanged` → re-enumeration path in
//      server.rs that P2 must keep, NOT drop / NOT double-emit).
//
// NOT covered here (needs a teammate native view, impractical to script): the
// "native detach = 0 rebuild" suppression — covered by design review + manual.
//
// Usage:
//   Terminal 1: pnpm tauri:dev:cdp
//   Terminal 2: pnpm cdp:pane-graph
// Exit 0 = both characterized behaviours hold; non-zero = regression/failure.
process.env.NODE_TLS_REJECT_UNAUTHORIZED = '0';
import http from 'node:http';

const CDP_PORT = Number(process.env.CDP_PORT ?? 9222);
const log = (...a) => console.log('[pane-graph]', ...a);
const fail = (m) => {
  console.error('[pane-graph] FAIL:', m);
  process.exit(1);
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function httpJson(path) {
  return new Promise((resolve, reject) => {
    http
      .get({ host: '127.0.0.1', port: CDP_PORT, path, timeout: 3000 }, (res) => {
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
    return new Promise((res, rej) => {
      this.ws.onopen = () => res();
      this.ws.onerror = (e) => rej(new Error('CDP ws error: ' + (e.message || e.type)));
      this.ws.onmessage = (ev) => {
        const m = JSON.parse(ev.data);
        if (m.id && this.pend.has(m.id)) {
          this.pend.get(m.id)(m);
          this.pend.delete(m.id);
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
  async evalAsync(expression) {
    const r = await this.send('Runtime.evaluate', {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (r.result?.exceptionDetails) {
      throw new Error('eval threw: ' + JSON.stringify(r.result.exceptionDetails));
    }
    return r.result?.result?.value;
  }
  close() {
    try {
      this.ws.close();
    } catch {}
  }
}

async function waitForRidgeTarget(maxMs = 90000) {
  const start = Date.now();
  let lastErr = '';
  while (Date.now() - start < maxMs) {
    try {
      const list = await httpJson('/json/list');
      const t = list.find(
        (x) =>
          x.type === 'page' &&
          (x.title === 'Ridge' || /127\.0\.0\.1:517\d|tauri\.localhost/.test(x.url || '')),
      );
      if (t) return t;
      lastErr = 'no Ridge page target yet (' + list.length + ' targets)';
    } catch (e) {
      lastErr = e.code || e.message;
    }
    await sleep(2000);
  }
  fail('timed out waiting for Ridge CDP target on :' + CDP_PORT + ' — ' + lastErr);
}

function countLeaves(node) {
  if (!node) return 0;
  if (node.type === 'leaf') return 1;
  if (node.type === 'split') return (node.children || []).reduce((a, c) => a + countLeaves(c), 0);
  return 0;
}
function firstLeafId(node) {
  if (!node) return null;
  if (node.type === 'leaf') return node.id;
  if (node.type === 'split') {
    for (const c of node.children || []) {
      const r = firstLeafId(c);
      if (r) return r;
    }
  }
  return null;
}

const invoke = (cdp, cmd, args) =>
  cdp.evalAsync(
    `window.__TAURI__.core.invoke('${cmd}', ${args ? JSON.stringify(args) : '{}'})`,
  );

(async () => {
  const t = await waitForRidgeTarget();
  log('ridge target:', t.url);
  const cdp = new Cdp(t.webSocketDebuggerUrl);
  await cdp.open();
  await cdp.send('Runtime.enable');

  await invoke(cdp, 'set_remote_enabled', { enabled: true });
  let info = null;
  for (let i = 0; i < 20; i++) {
    info = await invoke(cdp, 'get_remote_info');
    if (info && info.port > 0 && info.ready) break;
    await sleep(500);
  }
  if (!info || !info.port) fail('get_remote_info never reported a bound port');
  log('remote ready on port', info.port);

  // LAN-WS observer: count `panes` frames (the broadcast re-enumeration).
  let lanPanes = 0;
  const ws = new WebSocket(`wss://127.0.0.1:${info.port}/ws?code=${info.totpCode}&device=cdp-pane-graph`);
  await new Promise((res, rej) => {
    ws.onopen = res;
    ws.onerror = (e) => rej(new Error('LAN ws error: ' + (e.message || e.type)));
    setTimeout(() => rej(new Error('LAN ws open timeout')), 8000);
  });
  ws.onmessage = (ev) => {
    if (typeof ev.data !== 'string') return;
    let m;
    try {
      m = JSON.parse(ev.data);
    } catch {
      return;
    }
    if (m.type === 'panes') lanPanes++;
  };
  ws.send(JSON.stringify({ type: 'list-panes' }));
  await sleep(500); // let the initial panes frame land

  const summary = {
    baselineLeaves: null,
    afterSplitLeaves: null,
    afterCloseLeaves: null,
    newPane: null,
    treeSplitOk: false,
    treeCloseOk: false,
    splitBroadcastOk: false,
    closeBroadcastOk: false,
    errors: [],
  };

  const finish = () => {
    try {
      ws.close();
    } catch {}
    cdp.close();
    console.log('\n==== PANE-GRAPH CHARACTERIZATION SUMMARY ====');
    console.log(JSON.stringify(summary, null, 2));
    const pass =
      summary.treeSplitOk &&
      summary.treeCloseOk &&
      summary.splitBroadcastOk &&
      summary.closeBroadcastOk;
    console.log(`\n  tree split  (+1 leaf)        : ${summary.treeSplitOk ? 'PASS ✅' : 'FAIL ❌'}`);
    console.log(`  tree close  (back to base)   : ${summary.treeCloseOk ? 'PASS ✅' : 'FAIL ❌'}`);
    console.log(`  split broadcast (panes frame): ${summary.splitBroadcastOk ? 'PASS ✅' : 'FAIL ❌'}`);
    console.log(`  close broadcast (panes frame): ${summary.closeBroadcastOk ? 'PASS ✅' : 'FAIL ❌'}`);
    console.log('\nRESULT:', pass ? 'PASS ✅ (pane CRUD behaviour characterized)' : 'FAIL ❌');
    process.exit(pass ? 0 : 2);
  };

  try {
    const layout0 = await invoke(cdp, 'get_pane_layout');
    summary.baselineLeaves = countLeaves(layout0);
    const target = firstLeafId(layout0);
    if (!target) {
      summary.errors.push('no leaf to split');
      return finish();
    }
    log(`baseline leaves=${summary.baselineLeaves}, splitting ${target.slice(0, 8)}…`);

    // ── split ──
    const before = lanPanes;
    const r = await invoke(cdp, 'split_pane', { paneId: target, direction: 'horizontal' });
    summary.newPane = r && r.pane_id;
    const layout1 = await invoke(cdp, 'get_pane_layout');
    summary.afterSplitLeaves = countLeaves(layout1);
    summary.treeSplitOk = summary.afterSplitLeaves === summary.baselineLeaves + 1;
    if (!summary.treeSplitOk)
      summary.errors.push(`split leaves ${summary.baselineLeaves}->${summary.afterSplitLeaves} (want +1)`);
    await sleep(700); // let the PanesChanged broadcast land on the LAN sub
    summary.splitBroadcastOk = lanPanes > before;
    if (!summary.splitBroadcastOk)
      summary.errors.push('no LAN panes frame after split (broadcast missing)');
    log(`after split: leaves=${summary.afterSplitLeaves}, lanFrames+${lanPanes - before}`);

    if (!summary.newPane) {
      summary.errors.push('split returned no pane_id; cannot close');
      return finish();
    }

    // ── close ──
    const before2 = lanPanes;
    await invoke(cdp, 'close_pane', { paneId: summary.newPane });
    const layout2 = await invoke(cdp, 'get_pane_layout');
    summary.afterCloseLeaves = countLeaves(layout2);
    summary.treeCloseOk = summary.afterCloseLeaves === summary.baselineLeaves;
    if (!summary.treeCloseOk)
      summary.errors.push(`close leaves ${summary.afterSplitLeaves}->${summary.afterCloseLeaves} (want ${summary.baselineLeaves})`);
    await sleep(700);
    summary.closeBroadcastOk = lanPanes > before2;
    if (!summary.closeBroadcastOk)
      summary.errors.push('no LAN panes frame after close (broadcast missing)');
    log(`after close: leaves=${summary.afterCloseLeaves}, lanFrames+${lanPanes - before2}`);

    finish();
  } catch (e) {
    summary.errors.push('driver threw: ' + (e.message || String(e)));
    finish();
  }
})().catch((e) => fail(e.stack || e.message || String(e)));
