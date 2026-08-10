#!/usr/bin/env node
// CDP-driven END-TO-END test for the PTY output parsers that live in
// `ridge_core::pty` (decode / cwd / title). Runs against a REAL running Ridge
// (the `tauri:dev:cdp` debug instance) so the desktop PTY read loop, the
// ridge-core parsers, and the LAN forwarding are all exercised together.
//
// What it proves (the parsers extracted into ridge_core::pty, 2026-06-07):
//   1. decode (incremental UTF-8)  — a line built from 3- and 4-byte code
//      points (∑, 你好, 😀, 🇯🇵) round-trips INTACT through the PTY byte
//      stream → binary WS frame. A broken decoder yields U+FFFD garbage.
//   2. title  (OSC 0/1/2)          — an injected `ESC ] 2 ; <title> BEL` is
//      parsed and surfaces as a `pty-meta` message with the exact title.
//   3. cwd    (OSC 7)              — an injected `ESC ] 7 ; file:///C:/<dir> BEL`
//      is parsed (drive-letter + slash normalisation) and surfaces as a
//      `pty-meta` / `pane-cwd-changed` cwd containing the marker dir.
//
// (find_prompt_osc is NOT forwarded over the LAN WS, so it stays covered by the
//  ridge-core unit tests; this e2e covers the three observable signals.)
//
// The injected command is PURE ASCII source that GENERATES the multi-byte
// output via code points, so the test isolates the OUTPUT decode path (not
// stdin encoding); it forces `[Console]::OutputEncoding = UTF8` so Windows
// PowerShell 5.1 emits UTF-8 regardless of the console code page.
//
// Usage:
//   Terminal 1: pnpm tauri:dev:cdp     (wait for the Ridge window)
//   Terminal 2: pnpm cdp:pty           (this script)
// Exit 0 = all three parsers confirmed; non-zero = failure (see summary).
import http from 'node:http';
import { resolveCdpPort } from './cdp-port.mjs';

const CDP_PORT = resolveCdpPort();
const log = (...a) => console.log('[pty-e2e]', ...a);
const fail = (m) => { console.error('[pty-e2e] FAIL:', m); process.exit(1); };

// ── Markers ──────────────────────────────────────────────────────────────────
// EMOJI: ∑ (U+2211, 3-byte) · 😀 (U+1F600, 4-byte) · 你好 (U+4F60/U+597D, 3-byte)
//        · 🇯🇵 (U+1F1EF U+1F1F5 regional indicators, 4-byte each).
const EMOJI = 'RIDGE_E2E_∑_\u{1F600}_你好_\u{1F1EF}\u{1F1F5}_END';
// Per-run nonce so the test is IDEMPOTENT: the desktop de-dups an unchanged
// pane title (a title that equals last run's would emit no PaneTitleChanged),
// so a fresh title every run guarantees the title-change event fires. (cwd is
// naturally non-deduped — the PowerShell prompt re-emits the real cwd each
// prompt, toggling it off our injected marker — but we nonce it too for symmetry.)
const NONCE = Date.now().toString(36);
const TITLE = 'RIDGE_E2E_TITLE_' + NONCE;
const CWD_MARKER = 'ridge_e2e_cwd_' + NONCE;

// Single PowerShell line. ASCII source → multi-byte output via code points.
const GENERATED_CMD = [
  '[Console]::OutputEncoding=[System.Text.Encoding]::UTF8',
  '$e=[char]27',
  '$b=[char]7',
  `[Console]::Write($e+']2;${TITLE}'+$b)`,
  `[Console]::Write($e+']7;file:///C:/${CWD_MARKER}'+$b)`,
  "$s='RIDGE_E2E_'+[char]0x2211+'_'+[char]::ConvertFromUtf32(0x1F600)+'_'+[char]0x4F60+[char]0x597D+'_'+[char]::ConvertFromUtf32(0x1F1EF)+[char]::ConvertFromUtf32(0x1F1F5)+'_END'",
  '[Console]::Write($s+[char]10)',
].join('; ');
const CMD = process.env.RIDGE_PTY_COMMAND || GENERATED_CMD;

function httpJson(path) {
  return new Promise((resolve, reject) => {
    http.get({ host: '127.0.0.1', port: CDP_PORT, path, timeout: 3000 }, (res) => {
      let b = ''; res.on('data', (c) => (b += c)); res.on('end', () => {
        try { resolve(JSON.parse(b)); } catch (e) { reject(e); }
      });
    }).on('error', reject);
  });
}

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

async function waitForRidgeTarget(maxMs = 90000) {
  const start = Date.now();
  let lastErr = '';
  while (Date.now() - start < maxMs) {
    try {
      const list = await httpJson('/json/list');
      const t = list.find((x) => x.type === 'page' && (x.title === 'Ridge' || /127\.0\.0\.1:\d+|tauri\.localhost/.test(x.url || '')));
      if (t) return t;
      lastErr = 'no Ridge page target yet (' + list.length + ' targets)';
    } catch (e) { lastErr = e.code || e.message; }
    await new Promise((r) => setTimeout(r, 2000));
  }
  fail('timed out waiting for Ridge CDP target on :' + CDP_PORT + ' — ' + lastErr);
}

async function waitForTauriBridge(cdp, maxMs = 180000) {
  const deadline = Date.now() + maxMs;
  while (Date.now() < deadline) {
    const ready = await cdp.evalAsync('Boolean(window.__TAURI__?.core?.invoke)');
    if (ready) return;
    await new Promise((r) => setTimeout(r, 250));
  }
  fail(`Tauri bridge did not become ready within ${maxMs}ms`);
}

try {
  await (async () => {
  log('waiting for Ridge CDP target on :' + CDP_PORT + ' …');
  const t = await waitForRidgeTarget();
  log('ridge target:', t.url);
  const cdp = new Cdp(t.webSocketDebuggerUrl);
  await cdp.open();
  await cdp.send('Runtime.enable');
  await waitForTauriBridge(cdp);

  log('invoke set_remote_enabled(true)…');
  await cdp.evalAsync(`window.__TAURI__.core.invoke('set_remote_enabled',{enabled:true})`);
  let info = null;
  for (let i = 0; i < 20; i++) {
    info = await cdp.evalAsync(`window.__TAURI__.core.invoke('get_remote_info')`);
    if (info?.port > 0 && info.ready) break;
    await new Promise((r) => setTimeout(r, 500));
  }
  if (!info?.port) fail('get_remote_info never reported a bound port');
  log('remote info:', JSON.stringify({ port: info.port, ready: info.ready }));
  cdp.close();

  const url = `wss://127.0.0.1:${info.port}/ws?code=${info.totpCode}&device=cdp-pty-e2e-${NONCE}`;
  log('connecting host:', url.replace(/code=[^&]+/, 'code=***'));

  const summary = {
    pane: null, binaryFrames: 0, metas: 0,
    decodeOk: false, titleOk: false, cwdOk: false,
    seenTitle: null, seenCwd: null, errors: [],
  };
  const ws = new WebSocket(url);
  ws.binaryType = 'arraybuffer';
  let binConcat = '';
  let finished = false;

  const done = () => {
    if (finished) return;
    finished = true;
    try { ws.close(); } catch {}
    console.log('\n==== PTY PARSER E2E SUMMARY ====');
    console.log(JSON.stringify(summary, null, 2));
    const pass = summary.decodeOk && summary.titleOk && summary.cwdOk;
    console.log(`\n  decode (UTF-8) : ${summary.decodeOk ? 'PASS ✅' : 'FAIL ❌'}`);
    console.log(`  title  (OSC 2) : ${summary.titleOk ? 'PASS ✅' : 'FAIL ❌'}`);
    console.log(`  cwd    (OSC 7) : ${summary.cwdOk ? 'PASS ✅' : 'FAIL ❌'}`);
    console.log('\nRESULT:', pass ? 'PASS ✅ (ridge_core::pty parsers confirmed against live host)' : 'FAIL ❌');
    process.exit(pass ? 0 : 2);
  };

  // Fresh detached kernel may need several seconds to publish its first pane.
  // Keep this bounded without turning cold-start latency into a false negative.
  const hardTimeout = setTimeout(() => { summary.errors.push('hard timeout'); done(); }, 90000);
  let listedWorkspaceId = null;
  let createRequested = false;
  let emptyPanePolls = 0;

  function evaluateAndFinish() {
    summary.decodeOk = binConcat.includes(EMOJI);
    if (!summary.decodeOk || !summary.titleOk || !summary.cwdOk) {
      log('decoded tail:', JSON.stringify(binConcat.slice(-2000)));
    }
    clearTimeout(hardTimeout);
    done();
  }

  function drive(workspaceId, paneId) {
    summary.pane = paneId;
    log('subscribe-pane', paneId);
    ws.send(JSON.stringify({ type: 'subscribe-pane', workspaceId, paneId }));
    setTimeout(() => { log('stdin → marker command'); ws.send(JSON.stringify({ type: 'stdin', workspaceId, paneId, data: CMD + '\r' })); }, 2500);
    // Windows PowerShell may echo a long UTF-8 marker line through ConPTY in
    // several reads before executing it. Keep the bounded hard timeout as the
    // failure gate, but do not stop collection while the input is still being
    // echoed.
    // Long PowerShell lines can be echoed/executed after several redraw
    // bursts on a warm ConPTY. Give the live stream a bounded settling window.
    setTimeout(evaluateAndFinish, 30000);
  }

  ws.onopen = () => {
    log('WS open → wait for host pane sync');
    // The detached rdg host creates its first kernel-backed pane after the
    // TLS listener is ready. Avoid turning that startup window into a false
    // create-pane failure in this E2E harness.
    setTimeout(() => {
      if (!summary.pane && !createRequested) ws.send(JSON.stringify({ type: 'list-panes' }));
    }, 5000);
  };
  ws.onerror = (e) => { summary.errors.push('ws error: ' + (e.message || e.type)); };
  ws.onclose = (e) => { if (!summary.pane) { summary.errors.push('closed before pane (code ' + e.code + ')'); } };

  ws.onmessage = (ev) => {
    if (typeof ev.data === 'string') {
      let m; try { m = JSON.parse(ev.data); } catch { return; }
      if (m.type === 'panes') {
        // The host may replay a snapshot after subscribe/PTY state changes.
        // Drive one pane per run; resubscribing on every snapshot races the
        // marker command against the next pane and drops the binary stream.
        listedWorkspaceId = m.workspaceId || listedWorkspaceId;
        if (summary.pane || createRequested) return;
        if (m.panes.length) {
          emptyPanePolls = 0;
          drive(m.workspaceId, m.panes[0].id);
          return;
        }
        if (emptyPanePolls < 8) {
          emptyPanePolls++;
          log(`empty pane snapshot; wait for host sync (${emptyPanePolls}/8)`);
          setTimeout(() => ws.send(JSON.stringify({ type: 'list-panes' })), 500);
          return;
        }
        createRequested = true;
        log('no panes -> create pane');
        ws.send(JSON.stringify({ type: 'create-pane' }));
      } else if (m.type === 'create-pane-result') {
        log('create-pane-result:', JSON.stringify(m));
        if (m.success && m.paneId && listedWorkspaceId) drive(listedWorkspaceId, m.paneId);
        else if (m.success && m.paneId) ws.send(JSON.stringify({ type: 'list-panes' }));
        else { summary.errors.push('create-pane failed: ' + (m.error || '?')); evaluateAndFinish(); }
      } else if (m.type === 'pty-meta') {
        summary.metas++;
        if (typeof m.title === 'string' && m.title === TITLE) { summary.titleOk = true; summary.seenTitle = m.title; }
        if (typeof m.cwd === 'string' && m.cwd.includes(CWD_MARKER)) { summary.cwdOk = true; summary.seenCwd = m.cwd; }
      } else if (m.type === 'event' && typeof m.name === 'string' && m.name.startsWith('pane-cwd-changed-')) {
        const cwd = m.payload?.cwd;
        if (typeof cwd === 'string' && cwd.includes(CWD_MARKER)) { summary.cwdOk = true; summary.seenCwd = cwd; }
      }
      return;
    }
    // Binary frame: 16-byte pane UUID + raw PTY bytes (UTF-8 from the decoder).
    const buf = new Uint8Array(ev.data);
    if (buf.length < 16) return;
    summary.binaryFrames++;
    binConcat += Buffer.from(buf.subarray(16)).toString('utf8');
  };
  })();
} catch (e) {
  fail(e.stack || e.message || String(e));
}
