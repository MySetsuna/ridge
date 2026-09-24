// scripts/devtools-trace4.mjs — capture localStorage writes during boot
const WebSocket = (await import('ws')).WebSocket;
const ws = new WebSocket('ws://localhost:9222/devtools/page/3');
let id = 1;
const pending = new Map();
ws.on('message', (data) => {
  const msg = JSON.parse(data.toString());
  if (msg.id && pending.has(msg.id)) { pending.get(msg.id)(msg); pending.delete(msg.id); }
});
function send(method, params = {}) {
  const myId = id++;
  return new Promise((resolve) => { pending.set(myId, resolve); ws.send(JSON.stringify({ id: myId, method, params })); });
}
ws.on('open', async () => {
  await send('Runtime.enable');
  await send('Page.enable');
  // Install hook BEFORE any navigation via Page.addScriptToEvaluateOnNewDocument
  await send('Page.addScriptToEvaluateOnNewDocument', {
    source: `
      window.__ridgeLSlog = [];
      var _origSet = Storage.prototype.setItem;
      Storage.prototype.setItem = function(k, v) {
        if (this === localStorage) {
          window.__ridgeLSlog.push({op:'set', k:k, v:String(v).slice(0, 80), ts:Date.now(), stack:(new Error().stack||'').split('\\n').slice(2, 5).join('|')});
        }
        return _origSet.call(this, k, v);
      };
      var _origRem = Storage.prototype.removeItem;
      Storage.prototype.removeItem = function(k) {
        if (this === localStorage) {
          window.__ridgeLSlog.push({op:'rem', k:k, ts:Date.now()});
        }
        return _origRem.call(this, k);
      };
      window.__ridgeBootLog = [];
      window.__ridgeBootLog.push('hook installed at ' + Date.now());
    `,
  });
  // First clear localStorage on existing page
  await send('Runtime.evaluate', { expression: `localStorage.clear()`, returnByValue: true });
  // Now navigate fresh
  await send('Page.navigate', { url: 'https://localhost:9620/_app/?reset=1&debug=pane=1&t=' + Date.now() });
  await new Promise(r => setTimeout(r, 6000));
  // Check state — auth screen or main?
  const s1 = await send('Runtime.evaluate', {
    expression: `JSON.stringify({appRoot: !!document.querySelector('.app-root'), lsKeys: Object.keys(localStorage), url: location.href, log: window.__ridgeLSlog?.length || 0})`,
    returnByValue: true,
  });
  console.log('after nav:', s1.result?.result?.value);
  // Auth flow
  const onAuth = await send('Runtime.evaluate', { expression: `!!document.querySelector('input[type=text]')`, returnByValue: true });
  if (onAuth.result?.result?.value) {
    const { createHmac } = await import('node:crypto');
    const { spawnSync } = await import('node:child_process');
    const ps = `Add-Type -AssemblyName System.Security; $b=[System.IO.File]::ReadAllBytes('${process.env.APPDATA + String.raw`\ridge\config\totp\37a8eec1ce19687d.seed`}'); $d=[System.Security.Cryptography.ProtectedData]::Unprotect($b, $null, [System.Security.Cryptography.DataProtectionScope]::CurrentUser); [System.BitConverter]::ToString($d).Replace('-','').ToLower()`;
    const out = spawnSync('powershell', ['-NoProfile', '-Command', ps], { encoding: 'utf8' });
    const secretHex = out.stdout.trim();
    const counter = Math.floor(Math.floor(Date.now() / 1000) / 30);
    const cb = Buffer.alloc(8); cb.writeBigUInt64BE(BigInt(counter));
    const mac = createHmac('sha256', Buffer.from(secretHex, 'hex')).update(cb).digest();
    const off = mac[mac.length - 1] & 0x0f;
    const code = ((mac[off] & 0x7f) << 24) | (mac[off+1] << 16) | (mac[off+2] << 8) | mac[off+3];
    const totp = String(code % 1_000_000).padStart(6, '0');
    console.log('TOTP:', totp);
    await send('Runtime.evaluate', { expression: `document.querySelector('input').focus(); document.querySelector('input').value=''`, returnByValue: true });
    await new Promise(r => setTimeout(r, 200));
    await send('Input.insertText', { text: totp });
    await new Promise(r => setTimeout(r, 500));
    await send('Runtime.evaluate', { expression: `Array.from(document.querySelectorAll('button')).find(b=>b.textContent.includes('Verify')).click()`, returnByValue: true });
  }
  for (let i = 0; i < 25; i++) {
    await new Promise(r => setTimeout(r, 1000));
    const state = await send('Runtime.evaluate', { expression: `!!document.querySelector('.app-root')`, returnByValue: true });
    if (state.result?.result?.value) break;
  }
  await new Promise(r => setTimeout(r, 3000));
  const finalState = await send('Runtime.evaluate', {
    expression: `JSON.stringify({
      appRoot: !!document.querySelector('.app-root'),
      overlay: !!document.querySelector('[data-pane-debug-main]'),
      overlayText: document.querySelector('[data-pane-debug-main]')?.innerText.slice(0, 300),
      activeWs: localStorage.getItem('rg-remote-active-ws:lan:localhost:9620'),
      paneMap: localStorage.getItem('rg-remote-pane-map:lan:localhost:9620'),
      log: window.__ridgeLSlog || []
    }, null, 2)`,
    returnByValue: true,
  });
  console.log('=== FINAL ===');
  console.log(finalState.result?.result?.value);
  process.exit(0);
});
ws.on('error', (e) => { console.error('WS error:', e.message); process.exit(1); });