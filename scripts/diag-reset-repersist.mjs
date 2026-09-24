// scripts/diag-reset-repersist.mjs
//
// §A.8 diagnostic: capture WHO re-persists rg-remote-* localStorage keys after
// ?reset=1 clears them. Attaches to the LIVE host on 9620 (serves the current
// remote-dist bundle) and computes TOTP from the DPAPI seed at run time.
//
// Method:
//   1. Connect to https://localhost:9620 (existing host). Install its CA.
//   2. addInitScript patches localStorage.setItem to record (key, value, stack)
//      for every rg-remote-* write — survives navigation.
//   3. Auth via TOTP (DPAPI-derived).
//   4. Seed STALE SENTINEL values into rg-remote-* LS keys, then navigate to
//      ?reset=1&debug=pane=1. Watch every subsequent rg-remote-* setItem: is it
//      the sentinel (reappeared from memory) or a fresh id, and what call stack?
//
// Output: printed trace + artifacts/release/avd-visual/diag-reset-repersist.json
// Read-only on the SPA — no source changes; does not push/tag/deploy.

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

// TOTP from DPAPI seed (identity "default") — same pattern as avd-visual-marker.
function readSeedHex() {
  const seedFile = `${process.env.APPDATA}/ridge/config/totp/37a8eec1ce19687d.seed`;
  if (!existsSync(seedFile)) throw new Error(`seed file not found: ${seedFile}`);
  const ps = `
    Add-Type -AssemblyName System.Security
    $bytes = [System.IO.File]::ReadAllBytes('${seedFile.replace(/'/g, "''")}')
    $dec = [System.Security.Cryptography.ProtectedData]::Unprotect($bytes, $null, [System.Security.Cryptography.DataProtectionScope]::CurrentUser)
    [System.BitConverter]::ToString($dec).Replace('-','').ToLower()
  `;
  const out = spawnSync("powershell", ["-NoProfile", "-NonInteractive", "-Command", ps], { encoding: "utf8", timeout: 30000, windowsHide: true });
  if (out.status !== 0) throw new Error(`DPAPI unprotect failed: ${String(out.stderr || "").slice(0, 120)}`);
  return out.stdout.trim();
}
function computeTotp() {
  const secret = Buffer.from(readSeedHex(), "hex");
  const counter = Math.floor(Math.floor(Date.now() / 1000) / 30);
  const cb = Buffer.alloc(8);
  cb.writeBigUInt64BE(BigInt(counter));
  const mac = createHmac("sha256", secret).update(cb).digest();
  const offset = mac[mac.length - 1] & 0x0f;
  const code = ((mac[offset] & 0x7f) << 24) | (mac[offset + 1] << 16) | (mac[offset + 2] << 8) | mac[offset + 3];
  return String(code % 1000000).padStart(6, "0");
}

// Probe the live host.
const probe = spawnSync("curl", ["-sk", "-o", "/dev/null", "-w", "%{http_code}", `https://localhost:${HOST_PORT}/`], { encoding: "utf8" });
const httpCode = String(probe.stdout).trim();
log(`[diag] host probe https://localhost:${HOST_PORT}/ = ${httpCode}`);
if (httpCode !== "200") { log("[diag] FATAL host not alive; start it first"); process.exit(10); }

// Trust the host CA (already written to %LOCALAPPDATA%\ridge\remote-tls by the host).
let caPem = null;
try { caPem = loadHostCa(); } catch { /* ignore */ }
if (caPem) {
  const caPathLocal = join(OUT_DIR, "diag-host-ca.pem");
  writeFileSync(caPathLocal, caPem);
  const install = spawnSync("certutil", ["-user", "-addstore", "Root", caPathLocal], { encoding: "utf8" });
  log(`[diag] certutil addstore status=${install.status}`);
} else {
  log("[diag] no ca.pem — relying on ignoreHTTPSErrors");
}

const totp = computeTotp();
log(`[diag] totp computed`);

const browser = await chromium.launch({ headless: false, slowMo: 0, args: ["--no-proxy-server"] });
const context = await browser.newContext({ viewport: { width: 1280, height: 900 }, locale: "en-US", ignoreHTTPSErrors: true });
const page = await context.newPage();

// Patch localStorage.setItem BEFORE any app script runs.
await page.addInitScript(() => {
  window.__rgWrites = [];
  const orig = Storage.prototype.setItem;
  Storage.prototype.setItem = function (k, v) {
    try {
      if (typeof k === "string" && k.indexOf("rg-remote-") === 0) {
        const stack = (new Error()).stack || "";
        window.__rgWrites.push({ k, v: String(v).slice(0, 200), t: Math.round(performance.now()), stack: stack.split("\n").slice(1, 9).join(" | ") });
      }
    } catch { /* ignore */ }
    return orig.call(this, k, v);
  };
});

await page.goto(`https://localhost:${HOST_PORT}/`, { waitUntil: "domcontentloaded", timeout: 30000 });

// Auth (TOTP gate).
const totpInput = page.locator('input[inputmode="numeric"]').first();
let gated = false;
try {
  await totpInput.waitFor({ state: "visible", timeout: 8000 });
  gated = true;
  await totpInput.fill(totp);
  const btn = page.locator("button").filter({ hasText: /Connect|连接|验证|Verify|继续/i }).first();
  if (await btn.count()) await btn.click(); else await totpInput.press("Enter");
} catch { log("[diag] no totp gate (auto-token)"); }
await page.waitForFunction(() => !document.querySelector(".wr-gate") && !/Verify & Connect|验证失败/.test(document.body?.innerText ?? ""), null, { timeout: 15000 }).catch(() => {});
log(`[diag] auth done (gated=${gated}); settling 6s`);
await sleep(6000);

// Phase A: read live ids + current LS.
const phaseA = await page.evaluate(() => ({
  ls: Object.fromEntries(Object.keys(localStorage).filter((k) => k.indexOf("rg-remote-") === 0).map((k) => [k, localStorage.getItem(k)])),
  title: document.title,
  bodyHasCanvas: !!document.querySelector("canvas"),
}));
log("[diag] PHASE A ls=", JSON.stringify(phaseA.ls));
log("[diag] PHASE A title=", phaseA.title, "canvas=", phaseA.bodyHasCanvas);

// Phase B: seed STALE SENTINELS then reload with ?reset=1.
const STALE_WS = "STALE-WS-SENTINEL-d256f61c";
const STALE_PANE = "STALE-PANE-SENTINEL-b3884a34";
await page.evaluate(({ ws, pane }) => {
  for (const k of Object.keys(localStorage)) if (k.indexOf("rg-remote-") === 0) localStorage.removeItem(k);
  // Seed under several scopes to be sure we hit the one the app uses.
  for (const scope of ["lan:localhost:9620", "lan:127.0.0.1:9620", "lan:127.0.0.1:5130"]) {
    localStorage.setItem(`rg-remote-active-ws:${scope}`, ws);
    localStorage.setItem(`rg-remote-pane-map:${scope}`, JSON.stringify({ [ws]: pane }));
  }
  window.__rgWrites = [];
}, { ws: STALE_WS, pane: STALE_PANE });
log("[diag] seeded stale sentinels; navigating ?reset=1");

await page.goto(`https://localhost:${HOST_PORT}/_app/?reset=1&debug=pane=1`, { waitUntil: "domcontentloaded", timeout: 30000 });
await sleep(8000);

const phaseB = await page.evaluate(() => ({
  writes: window.__rgWrites,
  ls: Object.fromEntries(Object.keys(localStorage).filter((k) => k.indexOf("rg-remote-") === 0).map((k) => [k, localStorage.getItem(k)])),
  title: document.title,
  bodyHasCanvas: !!document.querySelector("canvas"),
}));
log("[diag] PHASE B ls=", JSON.stringify(phaseB.ls));
log("[diag] PHASE B title=", phaseB.title, "canvas=", phaseB.bodyHasCanvas);
log("[diag] PHASE B writes count=", (phaseB.writes || []).length);
for (const w of phaseB.writes || []) {
  log(`  WRITE t=${w.t} ${w.k} = ${w.v}`);
  log(`    STACK ${w.stack}`);
}

const reappeared = Object.entries(phaseB.ls).some(([k, v]) => String(v).includes("SENTINEL"));
log(`[diag] VERDICT sentinel_reappeared=${reappeared}`);

const report = {
  schema_version: 1,
  generated_at: new Date().toISOString(),
  phaseA,
  phaseB,
  sentinel: { stale_ws: STALE_WS, stale_pane: STALE_PANE, reappeared },
};
writeFileSync(join(OUT_DIR, "diag-reset-repersist.json"), JSON.stringify(report, null, 2));
log(`[diag] wrote ${join(OUT_DIR, "diag-reset-repersist.json")}`);

try { await context.close(); await browser.close(); } catch { /* ignore */ }
process.exit(0);
