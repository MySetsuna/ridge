// scripts/devtools-totp.mjs — type TOTP via DevTools, verify, then check MainApp
const WebSocket = (await import('ws')).WebSocket;
import { createHmac } from 'node:crypto';
import { readFileSync, existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
const seedFile = `${process.env.APPDATA}/ridge/config/totp/37a8eec1ce19687d.seed`;
if (!existsSync(seedFile)) throw new Error('seed not found');
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
  // Type TOTP
  await send('Input.insertText', { text: totp });
  // Click verify
  const click = await send('Runtime.evaluate', {
    expression: `
      (() => {
        const btn = Array.from(document.querySelectorAll('button')).find(b => b.textContent.includes('Verify'));
        if (!btn) return 'no button';
        btn.click();
        return 'clicked';
      })()
    `,
    returnByValue: true,
  });
  console.log('click:', click.result?.result?.value);
  await new Promise(r => setTimeout(r, 4000));
  // Check state
  const state = await send('Runtime.evaluate', {
    expression: `
      JSON.stringify({
        appRoot: !!document.querySelector('.app-root'),
        overlay: !!document.querySelector('[data-pane-debug-main]'),
        screenCls: document.body.innerHTML.match(/class="screen[^"]*"/)?.[0] || null,
        appRootCls: document.querySelector('[class*="app-root"]')?.className || null,
        overlayText: document.querySelector('[data-pane-debug-main]')?.innerText.slice(0, 300) || null,
        visibleText: document.body.innerText.slice(0, 200),
      })
    `,
    returnByValue: true,
  });
  console.log('after verify:', state.result?.result?.value);
  process.exit(0);
});
ws.on('error', (e) => { console.error('WS error:', e.message); process.exit(1); });