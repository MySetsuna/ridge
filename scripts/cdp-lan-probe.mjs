#!/usr/bin/env node
// CDP-driven LAN remote-control protocol probe.
//
// Purpose: validate the desktop LAN host wire protocol END-TO-END against a
// REAL running Ridge (the `tauri:dev:cdp` debug instance) BEFORE writing the
// Rust `LanControllerSession` driver — so the driver is written to observed
// ground truth, not guesses. It also exercises the exact path the Rust driver
// will take: a non-browser WS client that accepts the host's self-signed TLS
// cert (here via NODE_TLS_REJECT_UNAUTHORIZED=0; the driver uses a rustls
// danger verifier).
//
// Flow:
//   1. CDP-attach to the Ridge page on :9222, call invoke('set_remote_enabled',
//      {enabled:true}) then poll invoke('get_remote_info') for port + TOTP.
//   2. Open wss://127.0.0.1:<port>/ws?code=<TOTP> and run the handshake:
//      hello → list-panes → subscribe-pane → stdin(echo) → claim-pane → ping.
//   3. Parse binary frames (16-byte UUID + PTY bytes) and assert the UUID
//      layout matches the paneId from list-panes (the core lan_proto.rs claim).
//
// Usage: node scripts/cdp-lan-probe.mjs   (with tauri:dev:cdp already running)
process.env.NODE_TLS_REJECT_UNAUTHORIZED = '0';
import http from 'node:http';

const CDP_PORT = Number(process.env.CDP_PORT ?? 9222);
const log = (...a) => console.log('[probe]', ...a);
const fail = (m) => { console.error('[probe] FAIL:', m); process.exit(1); };

function httpJson(path) {
  return new Promise((resolve, reject) => {
    http.get({ host: '127.0.0.1', port: CDP_PORT, path, timeout: 3000 }, (res) => {
      let b = ''; res.on('data', (c) => (b += c)); res.on('end', () => {
        try { resolve(JSON.parse(b)); } catch (e) { reject(e); }
      });
    }).on('error', reject);
  });
}

// ── Minimal CDP client over the page target's debugger websocket ──
class Cdp {
  constructor(url) { this.ws = new WebSocket(url); this.id = 0; this.pend = new Map(); }
  open() {
    return new Promise((res, rej) => {
      this.ws.onopen = () => res();
      this.ws.onerror = (e) => rej(new Error('CDP ws error: ' + (e.message || e.type)));
      this.ws.onmessage = (ev) => {
        const m = JSON.parse(ev.data);
        if (m.id && this.pend.has(m.id)) { this.pend.get(m.id)(m); this.pend.delete(m.id); }
      };
    });
  }
  send(method, params = {}) {
    const id = ++this.id;
    return new Promise((res) => { this.pend.set(id, res); this.ws.send(JSON.stringify({ id, method, params })); });
  }
  async evalAsync(expression) {
    const r = await this.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (r.result?.exceptionDetails) throw new Error('eval threw: ' + JSON.stringify(r.result.exceptionDetails));
    return r.result?.result?.value;
  }
  close() { try { this.ws.close(); } catch {} }
}

function uuidFromBytes(b) {
  const h = [...b].map((x) => x.toString(16).padStart(2, '0')).join('');
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20, 32)}`;
}

async function waitForRidgeTarget(maxMs = 90000) {
  const start = Date.now();
  let lastErr = '';
  while (Date.now() - start < maxMs) {
    try {
      const list = await httpJson('/json/list');
      const t = list.find((x) => x.type === 'page' && (x.title === 'Ridge' || /127\.0\.0\.1:5173|tauri\.localhost/.test(x.url || '')));
      if (t) return t;
      lastErr = 'no Ridge page target yet (' + list.length + ' targets)';
    } catch (e) { lastErr = e.code || e.message; }
    await new Promise((r) => setTimeout(r, 2000));
  }
  fail('timed out waiting for Ridge CDP target on :' + CDP_PORT + ' — ' + lastErr);
}

(async () => {
  // 1. Find the Ridge page target (self-wait so we can fire right after launch).
  log('waiting for Ridge CDP target on :' + CDP_PORT + ' …');
  const t = await waitForRidgeTarget();
  log('ridge target:', t.url);
  const cdp = new Cdp(t.webSocketDebuggerUrl);
  await cdp.open();
  await cdp.send('Runtime.enable');

  // 2. Enable remote control + fetch info (poll until the server has bound).
  log('invoke set_remote_enabled(true)…');
  await cdp.evalAsync(`window.__TAURI__.core.invoke('set_remote_enabled',{enabled:true})`);
  let info = null;
  for (let i = 0; i < 20; i++) {
    info = await cdp.evalAsync(`window.__TAURI__.core.invoke('get_remote_info')`);
    if (info && info.port > 0 && info.ready) break;
    await new Promise((r) => setTimeout(r, 500));
  }
  if (!info || !info.port) fail('get_remote_info never reported a bound port');
  log('remote info:', JSON.stringify({ port: info.port, lanIp: info.lanIp, ready: info.ready, totp: info.totpCode }));
  cdp.close();

  const { port, totpCode } = info;
  const url = `wss://127.0.0.1:${port}/ws?code=${totpCode}&device=cdp-probe`;
  log('connecting host:', url);

  // 3. Drive the LAN protocol.
  const summary = { hello: null, theme: false, panes: null, createdPane: null, subscribedPane: null, scrollbackFrames: 0, liveFrames: 0, echoSeen: false, pong: false, uuidMatch: null, errors: [] };
  const ws = new WebSocket(url);
  ws.binaryType = 'arraybuffer';
  let firstPane = null;
  const ECHO = 'RIDGE_CDP_PROBE_42';

  const done = (ok) => {
    clearTimeout(hardTimeout);
    try { ws.close(); } catch {}
    console.log('\n==== PROTOCOL VALIDATION SUMMARY ====');
    console.log(JSON.stringify(summary, null, 2));
    const pass = ok && summary.hello && Array.isArray(summary.panes) && summary.subscribedPane &&
      summary.scrollbackFrames + summary.liveFrames > 0 && summary.uuidMatch === true;
    console.log('\nRESULT:', pass ? 'PASS ✅ (lan_proto.rs format confirmed against live host)' : 'PARTIAL/FAIL ⚠');
    process.exit(pass ? 0 : 2);
  };

  const hardTimeout = setTimeout(() => { summary.errors.push('hard timeout'); done(false); }, 25000);

  // Subscribe to a pane, then exercise stdin/claim-pane/ping to drive live frames.
  function drivePane(paneId) {
    firstPane = paneId;
    summary.subscribedPane = paneId;
    log('subscribe-pane', paneId);
    ws.send(JSON.stringify({ type: 'subscribe-pane', paneId }));
    setTimeout(() => { log('stdin echo →', ECHO); ws.send(JSON.stringify({ type: 'stdin', paneId, data: `echo ${ECHO}\r` })); }, 800);
    setTimeout(() => ws.send(JSON.stringify({ type: 'claim-pane', paneId, rows: 30, cols: 100, pixelWidth: 0, pixelHeight: 0, seq: 1 })), 1500);
    setTimeout(() => ws.send(JSON.stringify({ type: 'ping' })), 2400);
  }

  ws.onopen = () => { log('WS open → list-panes'); ws.send(JSON.stringify({ type: 'list-panes' })); };
  ws.onerror = (e) => { summary.errors.push('ws error: ' + (e.message || e.type)); };
  ws.onclose = (e) => { if (!summary.panes) { summary.errors.push('closed before panes (code ' + e.code + ')'); } };

  ws.onmessage = (ev) => {
    if (typeof ev.data === 'string') {
      let m; try { m = JSON.parse(ev.data); } catch { return; }
      if (m.type === 'hello') { summary.hello = m; log('hello:', JSON.stringify(m)); }
      else if (m.type === 'theme') { summary.theme = true; }
      else if (m.type === 'pong') { summary.pong = true; log('pong received'); setTimeout(() => done(true), 400); }
      else if (m.type === 'panes') {
        summary.panes = m.panes;
        log(`panes: ${m.panes.length} →`, m.panes.map((p) => `${p.id.slice(0, 8)}…(${p.title})`).join(', '));
        if (m.panes.length) drivePane(m.panes[0].id);
        else { log('no panes → create-pane'); ws.send(JSON.stringify({ type: 'create-pane' })); }
      }
      else if (m.type === 'create-pane-result') {
        log('create-pane-result:', JSON.stringify(m));
        if (m.success && m.paneId) { summary.createdPane = m.paneId; setTimeout(() => drivePane(m.paneId), 800); }
        else { summary.errors.push('create-pane failed: ' + (m.error || '?')); done(false); }
      }
      else { log('text frame:', ev.data.slice(0, 120)); }
      return;
    }
    // Binary frame: 16-byte UUID + PTY bytes.
    const buf = new Uint8Array(ev.data);
    if (buf.length < 16) { summary.errors.push('short binary frame ' + buf.length); return; }
    const id = uuidFromBytes(buf.subarray(0, 16));
    const payload = Buffer.from(buf.subarray(16)).toString('utf8');
    const isScroll = summary.liveFrames === 0 && summary.scrollbackFrames === 0;
    if (firstPane && summary.uuidMatch === null) {
      summary.uuidMatch = id === firstPane;
      log(`binary frame paneId=${id} matches subscribed=${summary.uuidMatch}`);
    }
    if (summary.echoSeen === false && payload.includes(ECHO)) { summary.echoSeen = true; log('✓ live echo seen in binary frame'); }
    // Heuristic: the very first binary frame after subscribe is scrollback.
    if (isScroll) summary.scrollbackFrames++; else summary.liveFrames++;
  };
})().catch((e) => fail(e.stack || e.message || String(e)));
