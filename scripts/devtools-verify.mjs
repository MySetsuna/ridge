// scripts/devtools-verify.mjs — clean TOTP entry + verify + check MainApp overlay
const WebSocket = (await import('ws')).WebSocket;
import { createHmac } from 'node:crypto';
import { spawnSync } from 'node:child_process';
const ps = `Add-Type -AssemblyName System.Security; $b=[System.IO.File]::ReadAllBytes('${process.env.APPDATA + String.raw`\ridge\config\totp\37a8eec1ce19687d.seed`}'); $d=[System.Security.Cryptography.ProtectedData]::Unprotect($b, $null, [System.Security.Cryptography.DataProtectionScope]::CurrentUser); [System.BitConverter]::ToString($d).Replace('-','').ToLower()`;
const out = spawnSync('powershell', ['-NoProfile', '-Command', ps], { encoding: 'utf8' });
const secretHex = out.stdout.trim();
function totpNow() {
  const counter = Math.floor(Math.floor(Date.now() / 1000) / 30);
  const cb = Buffer.alloc(8); cb.writeBigUInt64BE(BigInt(counter));
  const mac = createHmac('sha256', Buffer.from(secretHex, 'hex')).update(cb).digest();
  const off = mac[mac.length - 1] & 0x0f;
  const code = ((mac[off] & 0x7f) << 24) | (mac[off+1] << 16) | (mac[off+2] << 8) | mac[off+3];
  return String(code % 1_000_000).padStart(6, '0');
}

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
  const totp = totpNow();
  console.log('TOTP:', totp);
  // Reload to start fresh
  await send('Page.reload', { ignoreCache: true });
  await new Promise(r => setTimeout(r, 4000));
  // Focus input
  await send('Runtime.evaluate', {
    expression: `(function() { const i = document.querySelector('input[type=text]'); if (i) { i.focus(); i.value=''; i.dispatchEvent(new Event('input', {bubbles:true})); } return !!i; })()`,
    returnByValue: true,
  });
  await new Promise(r => setTimeout(r, 200));
  // Use insertText which sets the value cleanly
  await send('Input.insertText', { text: totp });
  await new Promise(r => setTimeout(r, 500));
  const before = await send('Runtime.evaluate', {
    expression: `JSON.stringify({val: document.querySelector('input').value, len: document.querySelector('input').value.length})`,
    returnByValue: true,
  });
  console.log('input state:', before.result?.result?.value);
  if (before.result?.result?.value && JSON.parse(before.result.result.value).val !== totp) {
    console.log('MISMATCH, retrying with dispatchKeyEvent');
    // Clear and retry
    await send('Runtime.evaluate', { expression: `document.querySelector('input').value=''; document.querySelector('input').dispatchEvent(new Event('input', {bubbles:true}))`, returnByValue: true });
    await send('Runtime.evaluate', { expression: `document.querySelector('input').focus()`, returnByValue: true });
    for (const ch of totp) {
      await send('Input.dispatchKeyEvent', { type: 'keyDown', text: ch });
      await send('Input.dispatchKeyEvent', { type: 'char', text: ch });
      await send('Input.dispatchKeyEvent', { type: 'keyUp', text: ch });
      await new Promise(r => setTimeout(r, 30));
    }
    const check = await send('Runtime.evaluate', { expression: `JSON.stringify({val: document.querySelector('input').value, expected: '${totp}'})`, returnByValue: true });
    console.log('retry state:', check.result?.result?.value);
  }
  // Wait for new TOTP window in case we're close to boundary
  const startTime = Math.floor(Date.now() / 1000) % 30;
  const remaining = 30 - startTime;
  if (remaining < 5) {
    console.log(`waiting ${remaining}s for fresh TOTP`);
    await new Promise(r => setTimeout(r, remaining * 1000 + 500));
  }
  // Click verify
  await send('Runtime.evaluate', {
    expression: `(function() { const btn = Array.from(document.querySelectorAll('button')).find(b => b.textContent.includes('Verify')); if (btn && !btn.disabled) btn.click(); return btn ? 'clicked' : 'no-btn'; })()`,
    returnByValue: true,
  });
  // Wait for main app mount
  for (let i = 0; i < 30; i++) {
    await new Promise(r => setTimeout(r, 1000));
    const state = await send('Runtime.evaluate', {
      expression: `JSON.stringify({appRoot: !!document.querySelector('.app-root'), overlay: !!document.querySelector('[data-pane-debug-main]'), hasPane: !!document.querySelector('[class*="pane"]'), url: location.href, vis: document.body.innerText.slice(0, 200)})`,
      returnByValue: true,
    });
    const v = JSON.parse(state.result?.result?.value || '{}');
    console.log(`tick ${i}: appRoot=${v.appRoot} overlay=${v.overlay} vis="${v.vis?.slice(0, 80)}"`);
    if (v.appRoot) {
      console.log('=== MainApp mounted! Overlay text ===');
      const detail = await send('Runtime.evaluate', {
        expression: `document.querySelector('[data-pane-debug-main]')?.innerText || 'NO OVERLAY'`,
        returnByValue: true,
      });
      console.log(detail.result?.result?.value);
      break;
    }
  }
  process.exit(0);
});
ws.on('error', (e) => { console.error('WS error:', e.message); process.exit(1); });