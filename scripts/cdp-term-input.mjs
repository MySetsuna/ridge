#!/usr/bin/env node
// Inject a line of input into the VISIBLE desktop terminal pane (for renderer
// testing via CDP). Enables remote over CDP, opens the LAN WS, finds the active
// workspace's first pane (creating one if none), and sends it as `stdin` — the
// PTY is shared, so the desktop renders the output and an MCP screenshot can
// capture it.
//
// Usage: node scripts/cdp-term-input.mjs            # default emoji test sheet
//        node scripts/cdp-term-input.mjs "echo hi"  # custom command
process.env.NODE_TLS_REJECT_UNAUTHORIZED = '0';
import http from 'node:http';

const CDP_PORT = Number(process.env.CDP_PORT ?? 9222);

// Default emoji conformance sheet (single PowerShell line; `cls` first for a
// clean frame). Covers: basic, CJK width, ZWJ sequences, skin-tone modifiers,
// RIS flags, VS16 text→emoji, and an alignment ruler.
const SHEET = [
  'cls',
  'Write-Host "1 basic   :  A 😀 🎉 🔥 🚀 ⭐ 🙂 🧠 🐙 🍕"',
  'Write-Host "2 cjk     :  你好世界 こんにちは 한국어 ABC"',
  'Write-Host "3 zwj     :  family 👨‍👩‍👧‍👦  dev 👩‍💻  flag 🏳️‍🌈  pirate 🏴‍☠️"',
  'Write-Host "4 skintone:  👍 👍🏻 👍🏽 👍🏿  ✋🏿  🧑🏽‍🚀"',
  'Write-Host "5 flags   :  🇯🇵 🇺🇸 🇨🇳 🇰🇷 🇪🇺"',
  'Write-Host "6 vs16    :  heart ❤️  check ✔️  warn ⚠️  sun ☀️  star ⭐"',
  'Write-Host "7 ruler   :  [😀][AB][你][🎉][CD]  |edge"',
  'Write-Host "8 mono    :  ✓ ✗ ❯ ★ ☆ ✻ ✽ → ─ │ └ ┤ ┌"',
].join('; ');

const cmd = process.argv[2] || SHEET;

function httpJson(path) {
  return new Promise((resolve, reject) => {
    http.get({ host: '127.0.0.1', port: CDP_PORT, path, timeout: 3000 }, (res) => {
      let b = ''; res.on('data', (c) => (b += c)); res.on('end', () => { try { resolve(JSON.parse(b)); } catch (e) { reject(e); } });
    }).on('error', reject);
  });
}

async function cdpEnableRemote() {
  const list = await httpJson('/json/list');
  const t = list.find((x) => x.type === 'page' && (x.title === 'Ridge' || /127\.0\.0\.1:5173|tauri\.localhost/.test(x.url || '')));
  if (!t) throw new Error('no Ridge CDP target');
  const ws = new WebSocket(t.webSocketDebuggerUrl);
  let id = 0; const pend = new Map();
  const send = (m, p) => new Promise((r) => { const i = ++id; pend.set(i, r); ws.send(JSON.stringify({ id: i, method: m, params: p || {} })); });
  await new Promise((res, rej) => { ws.onopen = res; ws.onerror = () => rej(new Error('cdp ws err')); ws.onmessage = (e) => { const m = JSON.parse(e.data); if (m.id && pend.has(m.id)) { pend.get(m.id)(m); pend.delete(m.id); } }; });
  await send('Runtime.enable');
  const ev = async (expr) => { const r = await send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true }); return r.result?.result?.value; };
  await ev(`window.__TAURI__.core.invoke("set_remote_enabled",{enabled:true})`);
  let info = null;
  for (let i = 0; i < 20; i++) { info = await ev(`window.__TAURI__.core.invoke("get_remote_info")`); if (info && info.port > 0 && info.ready) break; await new Promise((r) => setTimeout(r, 500)); }
  ws.close();
  if (!info || !info.port) throw new Error('remote not ready');
  return info;
}

(async () => {
  const info = await cdpEnableRemote();
  const url = `wss://127.0.0.1:${info.port}/ws?code=${info.totpCode}&device=cdp-input`;
  const ws = new WebSocket(url);
  ws.binaryType = 'arraybuffer';
  let pane = null;
  const sendJson = (o) => ws.send(JSON.stringify(o));
  const finish = (ok, msg) => { try { ws.close(); } catch {} console.log(msg); process.exit(ok ? 0 : 1); };
  const tmo = setTimeout(() => finish(false, 'timeout'), 12000);
  ws.onopen = () => sendJson({ type: 'list-panes' });
  ws.onerror = (e) => finish(false, 'ws error: ' + (e.message || e.type));
  ws.onmessage = (ev) => {
    if (typeof ev.data !== 'string') return;
    let m; try { m = JSON.parse(ev.data); } catch { return; }
    if (m.type === 'panes') {
      if (m.panes.length) { pane = m.panes[0].id; drive(); }
      else sendJson({ type: 'create-pane' });
    } else if (m.type === 'create-pane-result' && m.success) { pane = m.paneId; setTimeout(drive, 600); }
  };
  function drive() {
    console.log('pane =', pane);
    sendJson({ type: 'subscribe-pane', paneId: pane });
    setTimeout(() => { sendJson({ type: 'stdin', paneId: pane, data: cmd + '\r' }); }, 300);
    setTimeout(() => { clearTimeout(tmo); finish(true, 'injected ✓'); }, 1500);
  }
})().catch((e) => { console.error('FAIL:', e.message); process.exit(1); });
