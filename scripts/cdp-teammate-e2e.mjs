#!/usr/bin/env node
// Domain Zero teammate backend e2e — drives the live dev Ridge webview over CDP
// and invokes the new Tauri commands to prove the backend wiring works at runtime.
//
// Usage:
//   Terminal 1: pnpm tauri:dev:cdp        (wait for "CDP ready on port N")
//   Terminal 2: node scripts/cdp-teammate-e2e.mjs
//
// Verifies (Domain D2/D1):
//   - classify_command_risk: 'rm -rf /' → Dangerous(L2), 'ls -la' → ReadOnly(L0),
//     'echo hi > f' → WorkspaceWrite(L1), evasion 'git   push' → Dangerous
//   - get_teammate_topology: returns {roster,leaderId,edges} (roster may be empty)
//   - set_hitl_enabled(true) then (false): both succeed (gateway toggle live)
import http from 'node:http';
import { resolveCdpPort } from './cdp-port.mjs';

const CDP_PORT = resolveCdpPort();
const log = (...a) => console.log('[teammate-e2e]', ...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function httpJson(path) {
  return new Promise((resolve, reject) => {
    http
      .get({ host: '127.0.0.1', port: CDP_PORT, path, timeout: 3000 }, (res) => {
        let b = '';
        res.on('data', (c) => (b += c));
        res.on('end', () => {
          try { resolve(JSON.parse(b)); } catch (e) { reject(e); }
        });
      })
      .on('error', reject);
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
  async invoke(cmd, args = {}) {
    const expr = `window.__TAURI__.core.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)})`;
    const r = await this.send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true });
    if (r.result?.exceptionDetails) {
      throw new Error(`invoke ${cmd} threw: ${r.result.exceptionDetails.exception?.description || JSON.stringify(r.result.exceptionDetails)}`);
    }
    return r.result?.result?.value;
  }
  close() { try { this.ws.close(); } catch {} }
}

async function waitForRidge(maxMs = 90000) {
  const start = Date.now();
  while (Date.now() - start < maxMs) {
    try {
      const list = await httpJson('/json/list');
      const t = list.find((x) => x.type === 'page' && (x.title === 'Ridge' || /127\.0\.0\.1:517\d|localhost:517\d|tauri\.localhost/.test(x.url || '')));
      if (t?.webSocketDebuggerUrl) return t;
    } catch {}
    await sleep(2000);
  }
  throw new Error('no Ridge CDP target found');
}

let pass = 0, fail = 0;
function check(name, cond, detail) {
  if (cond) { pass++; log(`✅ ${name}`); }
  else { fail++; log(`❌ ${name} — ${detail}`); }
}

(async () => {
  log(`CDP port: ${CDP_PORT}`);
  const t = await waitForRidge();
  log(`ridge target: ${t.url}`);
  const cdp = new Cdp(t.webSocketDebuggerUrl);
  await cdp.open();
  await cdp.send('Runtime.enable');

  // D2 — risk classifier
  const r1 = await cdp.invoke('classify_command_risk', { command: 'rm -rf /' });
  check('classify rm -rf / → Dangerous', r1?.level === 'Dangerous', JSON.stringify(r1));
  const r2 = await cdp.invoke('classify_command_risk', { command: 'ls -la' });
  check('classify ls -la → ReadOnly', r2?.level === 'ReadOnly', JSON.stringify(r2));
  const r3 = await cdp.invoke('classify_command_risk', { command: 'echo hi > out.txt' });
  check('classify echo>file → WorkspaceWrite', r3?.level === 'WorkspaceWrite', JSON.stringify(r3));
  const r4 = await cdp.invoke('classify_command_risk', { command: 'git   push' });
  check('classify evasion "git   push" → Dangerous', r4?.level === 'Dangerous', JSON.stringify(r4));

  // D1 — topology snapshot
  const topo = await cdp.invoke('get_teammate_topology', {});
  check('get_teammate_topology shape {roster,edges}', Array.isArray(topo?.roster) && Array.isArray(topo?.edges), JSON.stringify(topo));
  log(`   roster=${topo?.roster?.length ?? '?'} leaderId=${topo?.leaderId ?? 'null'}`);

  // D2 — HITL gateway toggle
  const on = await cdp.invoke('set_hitl_enabled', { enabled: true });
  check('set_hitl_enabled(true) ok', on === null || on === undefined, JSON.stringify(on));
  const off = await cdp.invoke('set_hitl_enabled', { enabled: false });
  check('set_hitl_enabled(false) ok', off === null || off === undefined, JSON.stringify(off));

  cdp.close();
  console.log(`\n==== TEAMMATE E2E: ${pass} passed, ${fail} failed ====`);
  process.exit(fail === 0 ? 0 : 1);
})().catch((e) => { console.error('[teammate-e2e] FAIL:', e.message); process.exit(1); });
