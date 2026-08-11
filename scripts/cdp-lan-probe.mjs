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
//   1. CDP-attach to the Ridge page on its dynamic dev port, call invoke('set_remote_enabled',
//      {enabled:true}) then poll invoke('get_remote_info') for port + TOTP.
//   2. Open wss://127.0.0.1:<port>/ws?code=<TOTP> and run the handshake:
//      hello → list-panes → subscribe-pane → stdin(echo) → claim-pane → ping.
//   3. Parse binary frames (16-byte UUID + PTY bytes) and assert the UUID
//      layout matches the paneId from list-panes (the core lan_proto.rs claim).
//
// Usage: node scripts/cdp-lan-probe.mjs   (with tauri:dev:cdp already running)
import http from 'node:http';
import { resolveCdpPort } from './cdp-port.mjs';

const CDP_PORT = resolveCdpPort();
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

function parseTextFrame(data) {
  try { return JSON.parse(data); } catch { return null; }
}

function handlePanesMessage(message, context) {
  const { summary, state, ws } = context;
  if (state.driven) return;
  summary.panes = message.panes;
  log(`panes: ${message.panes.length} →`, message.panes.map((p) => `${p.id.slice(0, 8)}…(${p.title})`).join(', '));
  if (message.panes.length) {
    state.driven = true;
    context.drivePane(message.workspaceId, message.panes[0].id);
    return;
  }
  log('no panes → create-pane');
  ws.send(JSON.stringify({ type: 'create-pane' }));
}

function handleCreatePaneResult(message, context) {
  const { summary, ws } = context;
  log('create-pane-result:', JSON.stringify(message));
  if (message.success && message.paneId) {
    summary.createdPane = message.paneId;
    ws.send(JSON.stringify({ type: 'list-panes' }));
    return;
  }
  summary.errors.push('create-pane failed: ' + (message.error || '?'));
  context.done(false);
}

function handleLanTextFrame(data, context) {
  const message = parseTextFrame(data);
  if (!message) return;
  switch (message.type) {
    case 'hello':
      context.summary.hello = message;
      log('hello:', JSON.stringify(message));
      break;
    case 'theme':
      context.summary.theme = true;
      break;
    case 'pong':
      context.summary.pong = true;
      log('pong received');
      context.maybeDone();
      break;
    case 'panes':
      handlePanesMessage(message, context);
      break;
    case 'create-pane-result':
      handleCreatePaneResult(message, context);
      break;
    default:
      log('text frame:', data.slice(0, 120));
  }
}

function handleLanBinaryFrame(data, context) {
  const { summary, state } = context;
  const buf = new Uint8Array(data);
  if (buf.length < 16) {
    summary.errors.push('short binary frame ' + buf.length);
    return;
  }
  const id = uuidFromBytes(buf.subarray(0, 16));
  const payload = Buffer.from(buf.subarray(16)).toString('utf8');
  const isScroll = summary.liveFrames === 0 && summary.scrollbackFrames === 0;
  if (state.firstPane && summary.uuidMatch === null) {
    summary.uuidMatch = id === state.firstPane;
    log(`binary frame paneId=${id} matches subscribed=${summary.uuidMatch}`);
  }
  if (!isScroll && !summary.echoSeen && payload.includes(context.echo)) {
    summary.echoSeen = true;
    log('✓ live echo seen in binary frame');
    context.maybeDone();
  }
  // Heuristic: the very first binary frame after subscribe is scrollback.
  if (isScroll) summary.scrollbackFrames++; else summary.liveFrames++;
}

try {
  await (async () => {
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
    if (info?.port > 0 && info.ready) break;
    await new Promise((r) => setTimeout(r, 500));
  }
  if (!info?.port) fail('get_remote_info never reported a bound port');
  log('remote info:', JSON.stringify({ port: info.port, lanIp: info.lanIp, ready: info.ready, totp: info.totpCode }));
  cdp.close();

  const { port, totpCode } = info;
  const url = `wss://127.0.0.1:${port}/ws?code=${totpCode}&device=cdp-probe`;
  log('connecting host:', url);

  // 3. Drive the LAN protocol.
  const summary = { hello: null, theme: false, panes: null, createdPane: null, subscribedPane: null, scrollbackFrames: 0, liveFrames: 0, echoSeen: false, pong: false, uuidMatch: null, errors: [] };
  const ws = new WebSocket(url);
  ws.binaryType = 'arraybuffer';
  const ECHO = 'RIDGE_CDP_PROBE_' + Date.now().toString(36);

  const done = (ok) => {
    clearTimeout(hardTimeout);
    try { ws.close(); } catch {}
    console.log('\n==== PROTOCOL VALIDATION SUMMARY ====');
    console.log(JSON.stringify(summary, null, 2));
    const pass = ok && summary.hello && Array.isArray(summary.panes) && summary.subscribedPane &&
      summary.scrollbackFrames + summary.liveFrames > 0 && summary.liveFrames > 0 && summary.uuidMatch &&
      summary.echoSeen && summary.pong;
    console.log('\nRESULT:', pass ? 'PASS ✅ (lan_proto.rs format confirmed against live host)' : 'PARTIAL/FAIL ⚠');
    process.exit(pass ? 0 : 2);
  };

  // A freshly-created Windows ConPTY may need a few seconds before its shell
  // consumes the first input. Keep this aligned with cdp-pty-parsers.mjs so
  // cold-start latency does not masquerade as a broken live-output lane.
  const hardTimeout = setTimeout(() => { summary.errors.push('hard timeout'); done(false); }, 90000);
  const maybeDone = () => {
    if (summary.echoSeen && summary.pong) setTimeout(() => done(true), 400);
  };

  // Subscribe to a pane, then exercise stdin/claim-pane/ping to drive live frames.
  function drivePane(workspaceId, paneId) {
    state.firstPane = paneId;
    summary.subscribedPane = paneId;
    log('subscribe-pane', paneId);
    ws.send(JSON.stringify({ type: 'subscribe-pane', workspaceId, paneId }));
    setTimeout(() => { log('stdin echo →', ECHO); ws.send(JSON.stringify({ type: 'stdin', workspaceId, paneId, data: `echo ${ECHO}\r` })); }, 2500);
    setTimeout(() => ws.send(JSON.stringify({ type: 'claim-pane', workspaceId, paneId, rows: 30, cols: 100, pixelWidth: 0, pixelHeight: 0, seq: 1 })), 1500);
    setTimeout(() => ws.send(JSON.stringify({ type: 'ping' })), 2400);
  }

  ws.onopen = () => {
    log('WS open → wait for host pane sync');
    // Detached rdg host may publish the TLS endpoint before its first pane;
    // wait for the normal startup sync instead of invoking create-pane early.
    setTimeout(() => ws.send(JSON.stringify({ type: 'list-panes' })), 5000);
  };
  ws.onerror = (e) => { summary.errors.push('ws error: ' + (e.message || e.type)); };
  ws.onclose = (e) => { if (!summary.panes) { summary.errors.push('closed before panes (code ' + e.code + ')'); } };

  const state = { driven: false, firstPane: null };
  const frameContext = { summary, state, ws, echo: ECHO, done, drivePane, maybeDone };
  ws.onmessage = (ev) => {
    if (typeof ev.data === 'string') {
      handleLanTextFrame(ev.data, frameContext);
      return;
    }
    handleLanBinaryFrame(ev.data, frameContext);
  };
  })();
} catch (e) {
  fail(e.stack || e.message || String(e));
}
