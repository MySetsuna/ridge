// scripts/avd-marker-cdp.mjs
//
// §A.8 AVD visible-marker proof via raw CDP over the emulator's Chrome
// DevTools WebSocket (ws://localhost:9222/devtools/page/3). Reuses the
// ALREADY-RUNNING host on 9620 (current remote-dist) + adb reverse tcp:9620.
//
// Proves, on the AVD page itself:
//   HOP_WS     : marker bytes arrive in a WS frame (via Runtime WS spy)
//   HOP_MODEL  : __windE2E.visibleText(ws:pane) contains the marker (grid)
//   HOP_CANVAS : <canvas> present + Page.captureScreenshot artifact
//
// The DevTools HTTP /json endpoint is not served on the adb forward (only the
// raw page-3 WebSocket is), so this drives CDP directly — the same pattern as
// scripts/devtools-*.mjs. Sets window.__RIDGE_E2E__ = true via
// Page.addScriptToEvaluateOnNewDocument so the production-bundle __windE2E
// hooks install.
//
// Read-only on SPA source; no transport/wire/auth-protocol changes.
// Does not push/tag/release/deploy.

import { WebSocket } from "ws";
import { spawnSync } from "node:child_process";
import { writeFileSync, mkdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { createHmac } from "node:crypto";

const ROOT = resolve(".");
const ADB = `${process.env.LOCALAPPDATA}/Android/Sdk/platform-tools/adb.exe`;
const HOST_PORT = Number(process.env.AVD_HOST_PORT ?? "9620");
const CDP_WS = process.env.AVD_CDP_WS ?? "ws://localhost:9222/devtools/page/3";
const OUT_DIR = join(ROOT, "artifacts/release/avd-visual", `cdp-${Date.now()}`);
mkdirSync(OUT_DIR, { recursive: true });
const log = (...a) => console.log(a.join(" "));

const adb = (args) => spawnSync(ADB, args, { encoding: "utf8", timeout: 30000 });

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

// ── minimal CDP client over WebSocket ─────────────────────────────────────
const ws = new WebSocket(CDP_WS);
let _id = 0;
const pending = new Map();
const eventHandlers = [];
function send(method, params = {}) {
  const id = ++_id;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params }));
    setTimeout(() => { if (pending.has(id)) { pending.delete(id); reject(new Error(`cdp timeout ${method}`)); } }, 30000);
  });
}
ws.on("message", (data) => {
  const msg = JSON.parse(data.toString());
  if (msg.id && pending.has(msg.id)) {
    const { resolve, reject } = pending.get(msg.id);
    pending.delete(msg.id);
    if (msg.error) reject(new Error(msg.error.message)); else resolve(msg.result ?? {});
  } else if (msg.method) {
    for (const h of eventHandlers) h(msg);
  }
});

await new Promise((resolve, reject) => { ws.on("open", resolve); ws.on("error", reject); });
log("[avd] CDP connected", CDP_WS);

// Install the __RIDGE_E2E__ flag before any page script runs.
await send("Page.enable");
await send("Runtime.enable");
await send("Page.addScriptToEvaluateOnNewDocument", {
  source: "window.__RIDGE_E2E__ = true; window.__rgWs = []; (function(){ try { const orig = WebSocket; window.WebSocket = function(...a){ const s = new orig(...a); try { s.addEventListener('message', (e)=>{ try { window.__rgWs.push(String(e.data).slice(0,4000)); } catch(_){} }); } catch(_){} return s; }; window.WebSocket.prototype = orig.prototype; } catch(_){} })();",
});

async function evaluate(expression, awaitPromise = true) {
  const r = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise });
  if (r.exceptionDetails) return { __err: r.exceptionDetails?.exception?.description?.slice(0, 200) ?? "exception" };
  return r.result?.value;
}
async function screenshot(name) {
  const r = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(join(OUT_DIR, `${name}.png`), Buffer.from(r.data, "base64"));
}
async function keyText(text) {
  // Type via Input.insertText (fires through the app's key handlers like IME).
  await send("Input.insertText", { text });
}
async function keyEnter() {
  await send("Input.dispatchKeyEvent", { type: "rawKeyDown", windowsVirtualKeyCode: 13, key: "Enter", code: "Enter" });
  await send("Input.dispatchKeyEvent", { type: "keyUp", windowsVirtualKeyCode: 13, key: "Enter", code: "Enter" });
}

// ── navigate + auth ───────────────────────────────────────────────────────
const APP = `https://localhost:${HOST_PORT}/?reset=1&debug=pane=1`;
log("[avd] navigate", APP);
await send("Page.navigate", { url: APP });
await sleep(4000);
await screenshot("01-loaded");

const totp = computeTotp();
log("[avd] totp computed");
// Fill the TOTP input (inputmode=numeric) and submit.
const authed = await evaluate(`(function(){
  const inp = document.querySelector('input[inputmode="numeric"]');
  if (!inp) return 'no-gate';
  const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set;
  setter.call(inp, ${JSON.stringify(totp)});
  inp.dispatchEvent(new Event('input', { bubbles: true }));
  const btn = [...document.querySelectorAll('button')].find((b) => /Connect|连接|验证|Verify|继续/i.test(b.textContent));
  if (btn) { btn.click(); return 'clicked'; }
  return 'typed-no-btn';
})()`);
log("[avd] auth step:", authed);
await sleep(6000);
await screenshot("02-after-auth");

const shell = await evaluate(`JSON.stringify({
  hasGate: !!document.querySelector('.wr-gate'),
  title: document.title,
  canvas: [...document.querySelectorAll('canvas')].map((c) => ({ w: c.width, h: c.height })),
  bodyHead: (document.body?.innerText || '').slice(0, 160),
})`);
log("[avd] shell:", shell);

const MARKER = `RIDGE-AVD-VIS-${Date.now()}`;
// Focus the hidden terminal input, then type a real echo command.
await evaluate(`(function(){ const t = document.querySelector('textarea.hidden-input'); if (t) t.focus(); else { const c = document.querySelector('canvas'); if (c) c.click(); } return true; })()`);
await sleep(300);
await keyText(`echo ${MARKER}`);
await sleep(300);
await keyEnter();
log(`[avd] typed 'echo ${MARKER}' + Enter`);
await sleep(5000);
await screenshot("03-after-echo");

// HOP_WS: marker frames captured by the WS spy.
const wsHit = await evaluate(`(window.__rgWs || []).filter((t) => t.indexOf(${JSON.stringify(MARKER)}) >= 0).length`);
log("[avd] HOP_WS marker_frames =", wsHit);

// HOP_MODEL via composite key ws:pane.
const modelJson = await evaluate(`(function(){
  const marker = ${JSON.stringify(MARKER)};
  const hooks = window.__windE2E;
  if (!hooks || !hooks.visibleText) return JSON.stringify({ ok:false, reason:'no visibleText hook' });
  let wsId = null, paneId = null;
  try { const dbg = JSON.parse(localStorage.getItem('rg-remote-debug-state') || '{}');
    if (dbg.activeWorkspaceId && dbg.activeWorkspaceId !== '<none>') wsId = dbg.activeWorkspaceId;
    if (dbg.activePaneId && dbg.activePaneId !== '<none>') paneId = dbg.activePaneId;
  } catch(_){}
  const candidates = [];
  if (wsId && paneId) candidates.push(wsId + ':' + paneId);
  if (paneId) candidates.push(paneId);
  const result = { ok:false, wsId, paneId, candidates, checked:[] };
  for (const key of candidates) {
    try {
      const lines = hooks.visibleText(key);
      const joined = (lines || []).join('\\n');
      const backend = hooks.backendName ? hooks.backendName(key) : null;
      result.checked.push({ key, lines: (lines||[]).length, hasMarker: joined.indexOf(marker) >= 0, backend, tail: joined.slice(-160) });
      if (joined.indexOf(marker) >= 0) { result.ok = true; result.backend = backend; result.matchedKey = key; }
    } catch(e) { result.checked.push({ key, err: String(e).slice(0,80) }); }
  }
  return JSON.stringify(result);
})()`);
const model = JSON.parse(modelJson);
log("[avd] HOP_MODEL", JSON.stringify(model));

// HOP_CANVAS + full AVD screencap via adb.
const canvasInfo = JSON.parse(await evaluate(`JSON.stringify({
  canvases: [...document.querySelectorAll('canvas')].map((c) => ({ w: c.width, h: c.height })),
  title: document.title,
})`));
const remote = `/data/local/tmp/avd-cdp-final.png`;
adb(["shell", `screencap -p ${remote}`]);
adb(["pull", remote, join(OUT_DIR, "04-avd-screencap.png")]);
log("[avd] HOP_CANVAS", JSON.stringify(canvasInfo));

const verdict = {
  schema_version: 1,
  generated_at: new Date().toISOString(),
  marker: MARKER,
  hop_ws: { marker_frames: wsHit, pass: wsHit > 0 },
  hop_model: { pass: model.ok === true, detail: model },
  hop_canvas: { pass: canvasInfo.canvases.length > 0, canvases: canvasInfo.canvases },
  canvas: canvasInfo,
};
verdict.avd_visible_marker = verdict.hop_ws.pass && verdict.hop_model.pass && verdict.hop_canvas.pass;
writeFileSync(join(OUT_DIR, "verdict.json"), JSON.stringify(verdict, null, 2));
log(`[avd] VERDICT ws=${verdict.hop_ws.pass} model=${verdict.hop_model.pass} canvas=${verdict.hop_canvas.pass} → AVD_VISIBLE_MARKER=${verdict.avd_visible_marker}`);
log(`[avd] artifacts ${OUT_DIR}`);

try { ws.close(); } catch { /* ignore */ }
process.exit(verdict.avd_visible_marker ? 0 : 5);
