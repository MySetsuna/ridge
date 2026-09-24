// scripts/devtools-totp2.mjs — type via real key events, then check
const WebSocket = (await import('ws')).WebSocket;
import { createHmac } from 'node:crypto';
import { readFileSync, existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
const seedFile = `${process.env.APPDATA}/ridge/config/totp/37a8eec1ce19687d.seed`;
const ps = `Add-Type -AssemblyName System.Security; $b=[System.IO.File]::ReadAllBytes('${seedFile.replace(/'/g, "''")}'); $d=[System.Security.Cryptography.ProtectedData]::Unprotect($b, $null, [System.Security.Cryptography.DataProtectionScope]::CurrentUser); [System.BitConverter]::ToString($d).Replace('-','').ToLower()`;
const out = spawnSync('powershell', ['-NoProfile', '-Command', ps], { encoding: 'utf8' });
const secretHex = out.stdout.trim();
const counter = Math.floor(Math.floor(Date.now() / 1000) / 30);
const cb = Buffer.alloc(8); cb.writeBigUInt64BE(BigInt(counter));
const mac = createHmac('sha256', Buffer.from(secretHex, 'hex')).update(cb).digest();
const off = mac[mac.length - 1] & 0x0f;
const code = ((mac[off] & 0x7f) << 24) | (mac[off+1] << 16) | (mac[off+2] << 8) | mac[off+3];
const totp = String(code % 1_000_000).padStart(6, '0');
console.log('TOTP:', totp);

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
  // Focus input first
  await send('Runtime.evaluate', {
    expression: `document.querySelector('input[type=text]').focus()`,
    returnByValue: true,
  });
  await new Promise(r => setTimeout(r, 200));
  // Type each digit
  for (const ch of totp) {
    await send('Input.dispatchKeyEvent', { type: 'keyDown', text: ch, key: ch, code: `Digit${ch}` });
    await send('Input.dispatchKeyEvent', { type: 'char', text: ch });
    await send('Input.dispatchKeyEvent', { type: 'keyUp', text: ch, key: ch, code: `Digit${ch}` });
    await new Promise(r => setTimeout(r, 60));
  }
  await new Promise(r => setTimeout(r, 400));
  // Inspect input value before click
  const before = await send('Runtime.evaluate', {
    expression: `JSON.stringify({val: document.querySelector('input').value, btnDisabled: document.querySelector('button:not(.toggle)')?.disabled})`,
    returnByValue: true,
  });
  console.log('before click:', before.result?.result?.value);
  // Click verify
  await send('Runtime.evaluate', {
    expression: `document.querySelector('button:not(.toggle)').click()`,
    returnByValue: true,
  });
  await new Promise(r => setTimeout(r, 5000));
  const state = await send('Runtime.evaluate', {
    expression: `JSON.stringify({appRoot: !!document.querySelector('.app-root'), overlay: !!document.querySelector('[data-pane-debug-main]'), bodyClass: document.querySelector('[class*="app-root"]')?.className || null, visibleText: document.body.innerText.slice(0, 300)})`,
    returnByValue: true,
  });
  console.log('after verify:', state.result?.result?.value);
  process.exit(0);
});
ws.on('error', (e) => { console.error('WS error:', e.message); process.exit(1); });