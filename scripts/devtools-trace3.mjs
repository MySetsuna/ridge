// scripts/devtools-trace3.mjs — instrument reset block execution
const WebSocket = (await import('ws')).WebSocket;
const ws = new WebSocket('ws://localhost:9222/devtools/page/3');
let id = 1;
const pending = new Map();
ws.on('message', (data) => {
  const msg = JSON.parse(data.toString());
  if (msg.id && pending.has(msg.id)) { pending.get(msg.id)(msg); pending.delete(msg.id); }
  if (msg.method === 'Runtime.consoleAPICalled') {
    const args = msg.params.args.map(a => a.value || a.description).join(' ');
    console.log(`[${msg.params.type}] ${args}`);
  }
});
function send(method, params = {}) {
  const myId = id++;
  return new Promise((resolve) => { pending.set(myId, resolve); ws.send(JSON.stringify({ id: myId, method, params })); });
}
ws.on('open', async () => {
  await send('Runtime.enable');
  await send('Page.enable');
  // Set up a global hook to log every localStorage.setItem call
  await send('Runtime.evaluate', {
    expression: `
      (function(){
        var _origSet = localStorage.setItem.bind(localStorage);
        window.__ridgeLSlog = [];
        localStorage.setItem = function(k, v) {
          window.__ridgeLSlog.push({op: 'set', k: k, v: String(v).slice(0, 60), at: Date.now(), stack: new Error().stack.split('\\n').slice(1, 4).join(' | ')});
          return _origSet(k, v);
        };
        var _origRem = localStorage.removeItem.bind(localStorage);
        localStorage.removeItem = function(k) {
          window.__ridgeLSlog.push({op: 'rem', k: k, at: Date.now(), stack: new Error().stack.split('\\n').slice(1, 4).join(' | ')});
          return _origRem(k);
        };
        window.__ridgeBootOrder = [];
        window.__ridgeBootOrder.push('hook installed at ' + Date.now());
        return 'hook installed';
      })()
    `,
    returnByValue: true,
  });
  // Wipe and reload
  await send('Runtime.evaluate', { expression: `localStorage.clear()`, returnByValue: true });
  await send('Page.navigate', { url: 'https://localhost:9620/_app/?reset=1&debug=pane=1&ts=' + Date.now() });
  await new Promise(r => setTimeout(r, 5000));
  // On auth?
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
  // Wait for MainApp mount
  for (let i = 0; i < 20; i++) {
    await new Promise(r => setTimeout(r, 1000));
    const state = await send('Runtime.evaluate', { expression: `!!document.querySelector('.app-root')`, returnByValue: true });
    if (state.result?.result?.value) break;
  }
  await new Promise(r => setTimeout(r, 2000));
  // Get LS log
  const log = await send('Runtime.evaluate', {
    expression: `JSON.stringify({
      lsLog: window.__ridgeLSlog || [],
      bootOrder: window.__ridgeBootOrder || [],
      activeWs: localStorage.getItem('rg-remote-active-ws:lan:localhost:9620'),
    }, null, 2)`,
    returnByValue: true,
  });
  console.log('=== LS log ===');
  console.log(log.result?.result?.value);
  process.exit(0);
});
ws.on('error', (e) => { console.error('WS error:', e.message); process.exit(1); });