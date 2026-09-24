// scripts/diag-client-chain.mjs
//
// §A.8 client-chain proof (desktop): WS output → client terminal model/grid
// → invalidate → SurfaceHost → canvas visual output.
//
// Types a real marker into the live PTY (real keystrokes → write_to_pty), then
// verifies each hop independently:
//   HOP_WS     : marker bytes appear in a WebSocket frame received from host
//   HOP_MODEL  : __windE2E.visibleText(paneId) contains the marker (grid/model)
//   HOP_CANVAS : a <canvas> exists and its data-renderer-backend is the wasm
//                renderer (WebGPU/WebGL2), plus a screenshot of the canvas
//
// Attaches to the LIVE host on 9620 (serves the current remote-dist bundle).
// Read-only on the SPA source — no transport/wire changes; does not push/tag/deploy.

import { chromium } from "@playwright/test";
import { spawnSync } from "node:child_process";
import { writeFileSync, mkdirSync, existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { createHmac } from "node:crypto";
import { loadHostCa } from "./tls-host.mjs";

const ROOT = resolve(".");
const HOST_PORT = Number(process.env.RIDGE_DIAG_HOST_PORT ?? "9620");
const OUT_DIR = join(ROOT, "artifacts/release/avd-visual");
mkdirSync(OUT_DIR, { recursive: true });
const log = (...a) => console.log(a.join(" "));

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

const probe = spawnSync("curl", ["-sk", "-o", "/dev/null", "-w", "%{http_code}", `https://localhost:${HOST_PORT}/`], { encoding: "utf8" });
if (String(probe.stdout).trim() !== "200") { log("[chain] FATAL host not alive"); process.exit(10); }
try { const caPem = loadHostCa(); const p = join(OUT_DIR, "diag-host-ca.pem"); writeFileSync(p, caPem); spawnSync("certutil", ["-user", "-addstore", "Root", p], { encoding: "utf8" }); } catch { /* ignore */ }

const totp = computeTotp();
// RIDGE_EXTRA_ARGS: extra Chromium flags (e.g. --use-angle=swiftshader to
// reproduce the AVD emulator's GLES stack on desktop).
const EXTRA_ARGS = (process.env.RIDGE_EXTRA_ARGS ?? "").split(/\s+/).filter(Boolean);
const browser = await chromium.launch({ headless: false, slowMo: 0, args: ["--no-proxy-server", ...EXTRA_ARGS] });
const context = await browser.newContext({ viewport: { width: 1280, height: 900 }, locale: "en-US", ignoreHTTPSErrors: true });
const page = await context.newPage();

// Enable the __windE2E hooks (gated on __RIDGE_E2E__ in production builds),
// and capture every WS frame received from the host.
// RIDGE_FORCE_BACKEND=webgl2 → suppress navigator.gpu so the wasm renderer
// falls back to its WebGL2 backend (AVD has no WebGPU; repro the path here).
const FORCE_BACKEND = (process.env.RIDGE_FORCE_BACKEND ?? "").toLowerCase();
const GL_PROBE = process.env.RIDGE_GL_PROBE === "1";
await page.addInitScript(({ force, probe }) => {
  window.__RIDGE_E2E__ = true;
  if (force === "webgl2") {
    try { Object.defineProperty(navigator, "gpu", { get: () => undefined }); } catch { /* ignore */ }
  }
  if (probe) {
    // Record every texture creation (target/format/size) and every FBO attach
    // that leaves a GL error, so "INVALID_ENUM: invalid attachment" can be
    // traced to the exact texture + attachment point.
    window.__glLog = [];
    window.__glTex = new WeakMap();
    const tag = (t, info) => { try { window.__glTex.set(t, info); } catch { /* ignore */ } };
    for (const proto of [window.WebGL2RenderingContext?.prototype]) {
      if (!proto) continue;
      const ct = proto.createTexture;
      proto.createTexture = function () {
        const t = ct.call(this);
        tag(t, { target: null, fmt: null, w: 0, h: 0, storage: null });
        return t;
      };
      const tt2 = proto.texImage2D;
      proto.texImage2D = function (...a) {
        try {
          const t = a[0];
          const info = window.__glTex.get(t) ?? {};
          // texImage2D(target, level, internalformat, w, h, border, fmt, type, px)
          if (a.length === 9) { info.target = a[0]; info.fmt = a[2]; info.w = a[3]; info.h = a[4]; }
          window.__glTex.set(t, info);
        } catch { /* ignore */ }
        return tt2.apply(this, a);
      };
      const tts = proto.texStorage2D;
      if (tts) proto.texStorage2D = function (...a) {
        try {
          const t = a[0];
          const info = window.__glTex.get(t) ?? {};
          // texStorage2D(target, levels, internalformat, w, h)
          info.target = a[0]; info.storage = a[2]; info.w = a[3]; info.h = a[4];
          window.__glTex.set(t, info);
        } catch { /* ignore */ }
        return tts.apply(this, a);
      };
      const fta = proto.framebufferTexture2D;
      proto.framebufferTexture2D = function (...a) {
        const r = fta.apply(this, a);
        const err = this.getError();
        if (err) {
          const info = window.__glTex.get(a[3]) ?? null;
          window.__glLog.push({ fn: "framebufferTexture2D", args: [a[0], a[1], a[2], a[4]], err, tex: info });
        }
        return r;
      };
      const ftl = proto.framebufferTextureLayer;
      if (ftl) proto.framebufferTextureLayer = function (...a) {
        const r = ftl.apply(this, a);
        const err = this.getError();
        if (err) {
          const info = window.__glTex.get(a[3]) ?? null;
          window.__glLog.push({ fn: "framebufferTextureLayer", args: [a[0], a[1], a[4], a[5]], err, tex: info });
        }
        return r;
      };
    }
  }
}, { force: FORCE_BACKEND, probe: GL_PROBE });
const consoleLines = [];
const pageErrors = [];
page.on("console", (m) => consoleLines.push(`[${m.type()}] ${m.text().slice(0, 300)}`));
page.on("pageerror", (e) => pageErrors.push(String(e).slice(0, 300)));
const wsFrames = [];
page.on("websocket", (s) => {
  s.on("framereceived", (e) => {
    const p = e?.payload ?? e;
    const text = typeof p === "string" ? p : (p instanceof Uint8Array ? new TextDecoder().decode(p) : null);
    if (typeof text === "string") wsFrames.push({ t: Math.round(Date.now()), text: text.slice(0, 4000) });
  });
});

await page.goto(`https://localhost:${HOST_PORT}/?debug=pane=1`, { waitUntil: "domcontentloaded", timeout: 30000 });
const totpInput = page.locator('input[inputmode="numeric"]').first();
try {
  await totpInput.waitFor({ state: "visible", timeout: 8000 });
  await totpInput.fill(totp);
  const btn = page.locator("button").filter({ hasText: /Connect|连接|验证|Verify|继续/i }).first();
  if (await btn.count()) await btn.click(); else await totpInput.press("Enter");
} catch { log("[chain] no totp gate"); }
await page.waitForFunction(() => !document.querySelector(".wr-gate") && !/Verify & Connect|验证失败/.test(document.body?.innerText ?? ""), null, { timeout: 15000 }).catch(() => {});
await sleep(5000);

const MARKER = `RIDGE-CHAIN-${Date.now()}`;

// Identify the active pane + backend from the app.
const info = await page.evaluate(() => {
  const hooks = window.__windE2E;
  const canvases = [...document.querySelectorAll("canvas")].map((c) => ({
    w: c.width, h: c.height,
    backend: c.closest("[data-renderer-backend]")?.getAttribute("data-renderer-backend") ?? c.getAttribute("data-renderer-backend"),
  }));
  // Try to find the active pane id from the debug title or hooks.
  return {
    hasHooks: !!hooks,
    hooksKeys: hooks ? Object.keys(hooks) : [],
    canvases,
    title: document.title,
    bodyText: (document.body?.innerText ?? "").slice(0, 500),
  };
});
log("[chain] pre-input hasHooks=", info.hasHooks, "canvases=", JSON.stringify(info.canvases), "title=", info.title);

// Real keystrokes into the focused hidden textarea (same path the user types).
const hidden = page.locator("textarea.hidden-input").first();
if (await hidden.count()) await hidden.focus({ force: true }).catch(() => {});
await page.keyboard.type(`echo ${MARKER}`, { delay: 12 });
await sleep(300);
await page.keyboard.press("Enter");
log(`[chain] typed 'echo ${MARKER}' + Enter`);
await sleep(4000);

// HOP_WS: did the marker bytes arrive in a WS frame?
const wsHit = wsFrames.filter((f) => f.text.includes(MARKER));
log(`[chain] HOP_WS frames=${wsFrames.length} marker_frames=${wsHit.length}`);

// HOP_MODEL: does the client terminal model/grid contain the marker?
const model = await page.evaluate((marker) => {
  const hooks = window.__windE2E;
  if (!hooks?.visibleText) return { ok: false, reason: "no visibleText hook (is __RIDGE_E2E__ set?)" };
  // Resolve the active pane's composite manager key: `${workspaceId}:${paneId}`
  // (see paneRefKey). The manager.panes map is keyed by this composite, NOT the
  // bare pane UUID. Read full UUIDs from rg-remote-debug-state.
  let wsId = null, paneId = null;
  try {
    const dbg = JSON.parse(localStorage.getItem("rg-remote-debug-state") || "{}");
    if (dbg.activeWorkspaceId && dbg.activeWorkspaceId !== "<none>") wsId = dbg.activeWorkspaceId;
    if (dbg.activePaneId && dbg.activePaneId !== "<none>") paneId = dbg.activePaneId;
  } catch { /* ignore */ }
  const candidates = [];
  if (wsId && paneId) candidates.push(`${wsId}:${paneId}`); // composite key
  if (paneId) candidates.push(paneId);                      // bare uuid fallback
  const result = { ok: false, wsId, paneId, candidates, checked: [] };
  for (const key of candidates) {
    try {
      const lines = hooks.visibleText(key);
      const joined = (lines || []).join("\n");
      const backend = hooks.backendName ? hooks.backendName(key) : null;
      result.checked.push({ key, lines: (lines || []).length, hasMarker: joined.includes(marker), backend, tail: joined.slice(-200) });
      if (joined.includes(marker)) { result.ok = true; result.backend = backend; result.matchedKey = key; }
    } catch (e) { result.checked.push({ key, err: String(e).slice(0, 80) }); }
  }
  return result;
}, MARKER);
log("[chain] HOP_MODEL", JSON.stringify(model));

// HOP_CANVAS: canvas present + backend + a screenshot.
const canvasShot = join(OUT_DIR, `chain-canvas-${Date.now()}.png`);
try {
  const canvasEl = page.locator("canvas").first();
  if (await canvasEl.count()) await canvasEl.screenshot({ path: canvasShot });
} catch (e) { log("[chain] canvas screenshot err", String(e).slice(0, 80)); }
const post = await page.evaluate(() => {
  const canvases = [...document.querySelectorAll("canvas")].map((c) => ({
    w: c.width, h: c.height,
    cssW: Math.round(c.getBoundingClientRect().width), cssH: Math.round(c.getBoundingClientRect().height),
    backend: c.closest("[data-renderer-backend]")?.getAttribute("data-renderer-backend") ?? c.getAttribute("data-renderer-backend"),
    dataBackend: c.getAttribute("data-renderer-backend"),
  }));
  return { canvases, title: document.title };
});
log("[chain] HOP_CANVAS canvases=", JSON.stringify(post.canvases), "title=", post.title);
log(`[chain] console (${consoleLines.length}):`, consoleLines.slice(-12).join(" || "));
if (pageErrors.length) log(`[chain] pageerrors (${pageErrors.length}):`, pageErrors.slice(-8).join(" || "));
let glLog = null;
if (GL_PROBE) {
  glLog = await page.evaluate(() => {
    const l = window.__glLog ?? [];
    // Collapse repeats: same fn+args+tex shape → count.
    const seen = new Map();
    for (const e of l) {
      const k = JSON.stringify({ fn: e.fn, args: e.args, err: e.err, tex: e.tex });
      if (!seen.has(k)) seen.set(k, { ...e, count: 0 });
      seen.get(k).count++;
    }
    return [...seen.values()];
  });
  log("[chain] GL_PROBE distinct errors:", JSON.stringify(glLog, null, 1).slice(0, 3000));
}

const verdict = {
  schema_version: 1,
  generated_at: new Date().toISOString(),
  marker: MARKER,
  forced_backend: FORCE_BACKEND || null,
  hop_ws: { frames_total: wsFrames.length, marker_frames: wsHit.length, pass: wsHit.length > 0 },
  hop_model: { pass: model.ok === true, detail: model },
  hop_canvas: { pass: post.canvases.length > 0, canvases: post.canvases, screenshot: canvasShot },
  backend: post.canvases.map((c) => c.backend),
  console_tail: consoleLines.slice(-30),
  page_errors: pageErrors.slice(-20),
  gl_log: glLog,
};
verdict.chain_pass = verdict.hop_ws.pass && verdict.hop_model.pass && verdict.hop_canvas.pass;
const verdictPath = join(OUT_DIR, `diag-client-chain${FORCE_BACKEND ? `-webgl2` : ""}.json`);
writeFileSync(verdictPath, JSON.stringify(verdict, null, 2));
log(`[chain] VERDICT ws=${verdict.hop_ws.pass} model=${verdict.hop_model.pass} canvas=${verdict.hop_canvas.pass} → CHAIN_PASS=${verdict.chain_pass}`);
log(`[chain] wrote ${verdictPath}`);

try { await context.close(); await browser.close(); } catch { /* ignore */ }
process.exit(verdict.chain_pass ? 0 : 5);
