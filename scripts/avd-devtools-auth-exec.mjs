// scripts/avd-devtools-auth-exec.mjs
// Authenticate via TOTP gate + execute marker command — all via DevTools Protocol.
import WebSocket from "ws";
import { spawnSync } from "node:child_process";
import { createHmac } from "node:crypto";

function readSeedHex() {
  const seedFile = `${process.env.APPDATA}/ridge/config/totp/37a8eec1ce19687d.seed`;
  const ps = `
    Add-Type -AssemblyName System.Security
    $bytes = [System.IO.File]::ReadAllBytes('${seedFile.replace(/'/g, "''")}')
    $dec = [System.Security.Cryptography.ProtectedData]::Unprotect($bytes, $null, [System.Security.Cryptography.DataProtectionScope]::CurrentUser)
    [System.BitConverter]::ToString($dec).Replace('-','').ToLower()
  `;
  const out = spawnSync("powershell", ["-NoProfile", "-NonInteractive", "-Command", ps], { encoding: "utf8", timeout: 30000, windowsHide: true });
  return out.stdout.trim();
}
function computeTotp() {
  const secret = Buffer.from(readSeedHex(), "hex");
  const counter = Math.floor(Math.floor(Date.now() / 1000) / 30);
  const cb = Buffer.alloc(8); cb.writeBigUInt64BE(BigInt(counter));
  const mac = createHmac("sha256", secret).update(cb).digest();
  const offset = mac[mac.length - 1] & 0x0f;
  const code = ((mac[offset] & 0x7f) << 24) | (mac[offset + 1] << 16) | (mac[offset + 2] << 8) | mac[offset + 3];
  return String(code % 1000000).padStart(6, "0");
}

const MARKER = process.argv[2] || `RIDGE-AVD-CDP-${Date.now()}`;
console.log("MARKER:", MARKER);

let msgId = 0;
const ws = new WebSocket("ws://localhost:9222/devtools/page/2");
const pending = new Map();

function send(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++msgId;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params }));
    setTimeout(() => { if (pending.has(id)) { pending.delete(id); reject(new Error("timeout " + method)); } }, 15000);
  });
}

async function evaluate(expression) {
  const r = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  return r.result?.result?.value;
}

ws.on("message", (data) => {
  const msg = JSON.parse(data.toString());
  if (msg.id && pending.has(msg.id)) {
    pending.get(msg.id).resolve(msg);
    pending.delete(msg.id);
  }
});

ws.on("open", async () => {
  try {
    // Step 1: Check if we're at the auth gate
    const state = await evaluate(`JSON.stringify({
      hasGate: !!document.querySelector('input[inputmode="numeric"]'),
      hasCanvas: document.querySelectorAll('canvas').length,
      bodyText: (document.body?.innerText||'').slice(0,200)
    })`);
    console.log("Initial state:", state);
    const s = JSON.parse(state);

    if (s.hasGate) {
      // Step 2: Fill TOTP
      const totp = computeTotp();
      console.log("TOTP computed (not printed)");
      await evaluate(`
        (() => {
          const inp = document.querySelector('input[inputmode="numeric"]');
          if (!inp) return 'NO_INPUT';
          inp.focus();
          inp.value = '${totp}';
          inp.dispatchEvent(new Event('input', {bubbles:true}));
          return 'FILLED';
        })()
      `);
      await new Promise(r => setTimeout(r, 500));

      // Step 3: Click Verify & Connect
      await evaluate(`
        (() => {
          const btns = [...document.querySelectorAll('button')];
          const btn = btns.find(b => /verify|connect|验证|连接/i.test(b.textContent));
          if (btn) { btn.click(); return 'CLICKED:' + btn.textContent.trim(); }
          return 'NO_BUTTON';
        })()
      `);
      // Wait for terminal to mount
      console.log("Waiting for terminal...");
      for (let i = 0; i < 20; i++) {
        await new Promise(r => setTimeout(r, 1000));
        const check = await evaluate(`JSON.stringify({
          hasCanvas: document.querySelectorAll('canvas').length,
          hasHooks: !!window.__windE2E,
          hookKeys: window.__windE2E ? Object.keys(window.__windE2E).slice(0,10) : []
        })`);
        const c = JSON.parse(check);
        console.log(`  check ${i}:`, check);
        if (c.hasCanvas > 0 && c.hasHooks) break;
      }
    }

    // Step 4: Check hooks
    const hookCheck = await evaluate(`JSON.stringify({
      hasHooks: !!window.__windE2E,
      hookKeys: window.__windE2E ? Object.keys(window.__windE2E) : [],
      debugDiv: document.getElementById('exec-debug')?.textContent || 'NONE'
    })`);
    console.log("Hook check:", hookCheck);
    const hc = JSON.parse(hookCheck);

    if (!hc.hasHooks) {
      console.log("FATAL: __windE2E hooks not available");
      process.exit(1);
    }

    // Step 5: Find pane key and write marker
    const paneInfo = await evaluate(`JSON.stringify({
      dbg: localStorage.getItem('rg-remote-debug-state') || 'NONE',
      hookKeys: Object.keys(window.__windE2E)
    })`);
    console.log("Pane info:", paneInfo);
    const pi = JSON.parse(paneInfo);

    // Try to find the pane key
    let paneKey = null;
    if (pi.dbg && pi.dbg !== 'NONE') {
      try {
        const d = JSON.parse(pi.dbg);
        if (d.activeWorkspaceId && d.activeWorkspaceId !== '<none>' && d.activePaneId && d.activePaneId !== '<none>') {
          paneKey = `${d.activeWorkspaceId}:${d.activePaneId}`;
        } else if (d.activePaneId && d.activePaneId !== '<none>') {
          paneKey = d.activePaneId;
        }
      } catch {}
    }
    // Fallback: try common keys
    if (!paneKey) {
      for (const k of ['pane-0', 'pane-1', '0', '1']) {
        try {
          const r = await evaluate(`(() => { try { return String(window.__windE2E.visibleText('${k}')?.length ?? -1); } catch(e) { return 'ERR:' + e.message; } })()`);
          console.log(`  visibleText('${k}'):`, r);
          if (r && r !== '-1' && !String(r).startsWith('ERR')) { paneKey = k; break; }
        } catch {}
      }
    }
    if (!paneKey) {
      console.log("FATAL: could not find pane key");
      process.exit(1);
    }
    console.log("Using paneKey:", paneKey);

    // Step 6: Write marker to PTY
    const writeResult = await evaluate(`(async () => {
      try {
        await window.__windE2E.writePty('${paneKey}', 'echo ${MARKER}\\n');
        return 'WRITTEN';
      } catch(e) { return 'ERR:' + e.message; }
    })()`);
    console.log("Write result:", writeResult);

    // Step 7: Wait for output and read visible text
    await new Promise(r => setTimeout(r, 5000));
    const visible = await evaluate(`(() => {
      try {
        const lines = window.__windE2E.visibleText('${paneKey}');
        return JSON.stringify({ lines: lines?.length, tail: (lines||[]).slice(-10) });
      } catch(e) { return JSON.stringify({err: e.message}); }
    })()`);
    console.log("Visible text:", visible);

    // Step 8: Take a screenshot via DevTools
    const screenshot = await send("Page.captureScreenshot", { format: "png" });
    const b64 = screenshot.result?.data;
    if (b64) {
      const fs = await import("node:fs");
      fs.writeFileSync(`artifacts/release/avd-visual/cdp-${MARKER}.png`, Buffer.from(b64, "base64"));
      console.log("Screenshot saved");
    }

    process.exit(0);
  } catch (e) {
    console.error("Error:", e.message);
    process.exit(1);
  }
});

ws.on("error", (e) => { console.error("WS error:", e.message); process.exit(1); });
