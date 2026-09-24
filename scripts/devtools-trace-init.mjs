// scripts/devtools-trace-init.mjs — trace MainApp init
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
  // Wipe LS and reload with reset=1
  await send('Runtime.evaluate', { expression: `localStorage.clear()`, returnByValue: true });
  // Set up listener that captures console messages
  await send('Runtime.addBinding', { name: 'ridgeTrace' });
  // reload
  await send('Page.reload', { ignoreCache: true });
  await new Promise(r => setTimeout(r, 4000));
  // Now MainApp should be mounted (if verify is done). We may still be at auth.
  // Force verify programmatically
  await send('Runtime.evaluate', {
    expression: `(function(){const i=document.querySelector('input'); if(i) {return 'has-input-auth-screen';} return 'no-auth-screen';})()`,
    returnByValue: true,
  }).then(r => console.log('state:', r.result?.result?.value));
  // If on auth, type TOTP and verify
  const onAuth = await send('Runtime.evaluate', {
    expression: `!!document.querySelector('input[type=text]')`,
    returnByValue: true,
  });
  if (onAuth.result?.result?.value) {
    // Compute TOTP and type
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
  // Wait for MainApp mount + workspaces list
  for (let i = 0; i < 30; i++) {
    await new Promise(r => setTimeout(r, 1000));
    const state = await send('Runtime.evaluate', {
      expression: `JSON.stringify({appRoot: !!document.querySelector('.app-root'), overlay: !!document.querySelector('[data-pane-debug-main]'), activeWs: localStorage.getItem('rg-remote-active-ws:lan:localhost:9620'), paneMap: localStorage.getItem('rg-remote-pane-map:lan:localhost:9620'), lsKeys: Object.keys(localStorage), visibleText: document.body.innerText.slice(0, 100)})`,
      returnByValue: true,
    });
    const v = JSON.parse(state.result?.result?.value || '{}');
    console.log(`tick ${i}: appRoot=${v.appRoot} overlay=${v.overlay} activeWs=${v.activeWs?.slice(0, 8)} paneMap=${v.paneMap?.slice(0, 40)}`);
    if (v.appRoot && v.overlay) {
      const ov = await send('Runtime.evaluate', { expression: `document.querySelector('[data-pane-debug-main]')?.innerText`, returnByValue: true });
      console.log('overlay:', ov.result?.result?.value);
      break;
    }
  }
  process.exit(0);
});
ws.on('error', (e) => { console.error('WS error:', e.message); process.exit(1); });