#!/usr/bin/env node
// B1 diagnostic — does `get_directory_children` paginate correctly on the HOST?
//
// The cloud-host path is `cloudHostBridge → invoke('get_directory_children',
// {path,offset,limit})` (normalizeParams passes args verbatim), so driving that
// invoke directly on the host via CDP reproduces the EXACT host-side handling
// the controller would hit over cloud — WITHOUT needing the (postgres-gated)
// ridge-cloud relay. If offset>0 returns empty here, the bug is host-side
// (fixable now); if it returns the next page, B1 is on the controller/transport
// side (needs the full cloud stack to diagnose).
//
// Usage: pnpm tauri:dev:cdp (running) → node scripts/cdp-dirchildren-probe.mjs [path]
process.env.NODE_TLS_REJECT_UNAUTHORIZED = '0';
import http from 'node:http';

const CDP_PORT = Number(process.env.CDP_PORT ?? 9222);
const DIR = process.argv[2] || 'C:/code/wind';
const PAGE = 3;
const log = (...a) => console.log('[dirchildren]', ...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function httpJson(path) {
  return new Promise((resolve, reject) => {
    http.get({ host: '127.0.0.1', port: CDP_PORT, path, timeout: 3000 }, (res) => {
      let b = '';
      res.on('data', (c) => (b += c));
      res.on('end', () => { try { resolve(JSON.parse(b)); } catch (e) { reject(e); } });
    }).on('error', reject);
  });
}

class Cdp {
  constructor(url) { this.ws = new WebSocket(url); this.id = 0; this.pend = new Map(); }
  open() {
    return new Promise((res, rej) => {
      this.ws.onopen = () => res();
      this.ws.onerror = (e) => rej(new Error('CDP ws error: ' + (e.message || e.type)));
      this.ws.onmessage = (ev) => { const m = JSON.parse(ev.data); if (m.id && this.pend.has(m.id)) { this.pend.get(m.id)(m); this.pend.delete(m.id); } };
    });
  }
  send(method, params = {}) { const id = ++this.id; return new Promise((res) => { this.pend.set(id, res); this.ws.send(JSON.stringify({ id, method, params })); }); }
  async evalAsync(expression) {
    const r = await this.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (r.result?.exceptionDetails) throw new Error('eval threw: ' + JSON.stringify(r.result.exceptionDetails));
    return r.result?.result?.value;
  }
  close() { try { this.ws.close(); } catch {} }
}

async function waitForRidge(maxMs = 90000) {
  const start = Date.now();
  while (Date.now() - start < maxMs) {
    try {
      const list = await httpJson('/json/list');
      const t = list.find((x) => x.type === 'page' && (x.title === 'Ridge' || /127\.0\.0\.1:517\d|tauri\.localhost/.test(x.url || '')));
      if (t) return t;
    } catch {}
    await sleep(2000);
  }
  throw new Error('no Ridge CDP target');
}

const dc = (cdp, offset, limit) =>
  cdp.evalAsync(
    `window.__TAURI__.core.invoke('get_directory_children', ${JSON.stringify({ path: DIR, offset, limit })})`,
  );

(async () => {
  const t = await waitForRidge();
  const cdp = new Cdp(t.webSocketDebuggerUrl);
  await cdp.open();
  await cdp.send('Runtime.enable');
  log('probing dir:', DIR);

  const p0 = await dc(cdp, 0, PAGE);
  log(`offset=0 limit=${PAGE}: entries=${p0?.entries?.length} total=${p0?.total} has_more=${p0?.has_more}`);
  log('  page0 names:', (p0?.entries || []).map((e) => e.name).join(', '));

  const p1 = await dc(cdp, PAGE, PAGE);
  log(`offset=${PAGE} limit=${PAGE}: entries=${p1?.entries?.length} total=${p1?.total} has_more=${p1?.has_more}`);
  log('  page1 names:', (p1?.entries || []).map((e) => e.name).join(', '));

  const p2 = await dc(cdp, PAGE * 2, PAGE);
  log(`offset=${PAGE * 2} limit=${PAGE}: entries=${p2?.entries?.length}`);

  cdp.close();

  const lazyOk = (p1?.entries?.length ?? 0) > 0 && p0?.entries?.[0]?.name !== p1?.entries?.[0]?.name;
  console.log('\n==== B1 VERDICT ====');
  if ((p0?.entries?.length ?? 0) === 0) {
    console.log('INCONCLUSIVE: even offset=0 empty (dir has no/too-few entries?). Try a populated [path].');
  } else if (lazyOk) {
    console.log('HOST OK ✅ — offset>0 returns the NEXT page. B1 (empty-over-cloud) is NOT host-side;');
    console.log('it is controller/transport-side → needs the full cloud stack (postgres-gated) to diagnose.');
  } else {
    console.log('HOST BUG ❌ — offset>0 returned empty/duplicate while offset=0 had entries → B1 reproduced host-side.');
  }
})().catch((e) => { console.error('[dirchildren] FAIL:', e.message); process.exit(1); });
