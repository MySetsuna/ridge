// scripts/bench-release-gate.mjs
// CHG-050 Remote Release Gate — 4 hard gates.
//
// Gate 1 PANE_BINDING: stale pane ref must not cause REMOTE_RESIZE_FAILED.
// Gate 2 VISIBLE_MARKER: input → shell → output → client model → canvas visible.
// Gate 3 RECONNECT_RESIZE_SWITCH: reconnect / resize / rapid A→B→A.
// Gate 4 SCROLLBACK_SMOKE: 100/500/1000/5000 lines, no hang/wrong pane/lost content.
//
// Uses kernel scrollback API for PTY_EXECUTION and __windE2E.visibleText for
// client-model visibility. No DOM overlay, no mock output, no text injection.

import { chromium } from "@playwright/test";
import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { loadHostCa } from "./tls-host.mjs";

const ROOT = resolve(".");
const BIN = process.env.RIDGE_BIN ?? "target/debug/ridge.exe";
const HOST_PORT = Number(process.env.RIDGE_BENCH_HOST_PORT ?? "5121");
const ART_DIR = join(ROOT, "artifacts/release/latency");
mkdirSync(ART_DIR, { recursive: true });

function log(s) { console.log(s); }

// ── host boot ───────────────────────────────────────────────────────────
async function bootHost() {
  const dataDir = mkdtempSync(join(tmpdir(), "ridge-rg-"));
  const child = spawn(BIN, ["host", "--port", String(HOST_PORT)], {
    env: {
      ...process.env,
      RIDGE_KERNEL_DATA_DIR: dataDir,
      RIDGE_PRINT_TOTP: "1",
      RIDGE_TEST_ALLOW_NON_BREAKAWAY: "1",
      RIDGE_REMOTE_HOST_REGISTRY: join(dataDir, "host-registry.json"),
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const errBuf = [];
  child.stderr.on("data", (d) => errBuf.push(d.toString()));
  let totp = null;
  for (let i = 0; i < 60; i += 1) {
    await sleep(500);
    const m = /TOTP:\s*(\d{6})/g.exec(errBuf.join(""));
    if (m) { totp = m[1]; break; }
  }
  if (!totp) throw new Error("TOTP not printed within 30s");
  return { child, totp, dataDir };
}

function readKernel(dataDir) {
  const p = join(dataDir, "kernel.json");
  if (!existsSync(p)) return null;
  try { return JSON.parse(readFileSync(p, "utf8")); } catch { return null; }
}

function readFirstPaneRef(dataDir) {
  const p = join(dataDir, "workspace-graph.json");
  if (!existsSync(p)) return null;
  try {
    const g = JSON.parse(readFileSync(p, "utf8"));
    for (const [wsId, ws] of Object.entries(g.workspaces ?? {})) {
      const leaf = ws?.pane_tree?.root?.Leaf;
      if (typeof leaf === "string") return { workspaceId: wsId, paneId: leaf, key: `${wsId}:${leaf}` };
    }
  } catch { /* */ }
  return null;
}

function readFirstPaneId(dataDir) {
  return readFirstPaneRef(dataDir)?.paneId ?? null;
}

async function waitFor(fn, timeoutMs, stepMs = 200) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const v = fn();
    if (v) return v;
    await sleep(stepMs);
  }
  return null;
}

// ── CA install ──────────────────────────────────────────────────────────
function installCa(caPem) {
  const caPath = join(ART_DIR, `rg-ca-${Date.now()}.pem`);
  writeFileSync(caPath, caPem);
  const r = spawnSync("certutil", ["-user", "-addstore", "Root", caPath], { encoding: "utf8" });
  if (r.status !== 0) throw new Error(`certutil addstore failed: ${r.stderr}`);
  return () => {
    try { spawnSync("certutil", ["-user", "-delstore", "Root", caPath], { encoding: "utf8" }); } catch { /* */ }
  };
}

// ── kernel scrollback marker detection ──────────────────────────────────
async function scrollbackContains(kernelPort, token, paneId, marker, maxBytes = 65536) {
  const url = `http://127.0.0.1:${kernelPort}/v1/domain/ptys/${paneId}?max_bytes=${maxBytes}`;
  try {
    const r = await fetch(url, { headers: { "x-ridge-kernel-token": token } });
    const text = await r.text();
    let j;
    try { j = JSON.parse(text); } catch { return false; }
    if (!j.ok || !j.data_b64) return false;
    const bytes = Buffer.from(j.data_b64, "base64");
    return bytes.toString("utf8").includes(marker);
  } catch { return false; }
}

async function waitForScrollbackMarker(kernelPort, token, paneId, marker, timeoutMs = 6000, stepMs = 40) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await scrollbackContains(kernelPort, token, paneId, marker, 65536)) return Date.now();
    await sleep(stepMs);
  }
  return null;
}

// ── client-model visibility via __windE2E.visibleText ───────────────────
async function visibleTextContains(page, paneId, marker) {
  try {
    const result = await page.evaluate((pid) => {
      const w = window;
      const api = w.__windE2E;
      if (!api) return { available: false, lines: [] };
      const lines = api.visibleText(pid) ?? [];
      return { available: true, lines };
    }, paneId);
    if (!result.available) return { found: false, reason: "no __windE2E", lines: [] };
    const found = result.lines.some((l) => l.includes(marker));
    return { found, lines: result.lines.slice(-5) };
  } catch (e) { return { found: false, reason: String(e).slice(0, 100), lines: [] }; }
}

async function waitForVisibleMarker(page, paneId, marker, timeoutMs = 6000, stepMs = 50) {
  const deadline = Date.now() + timeoutMs;
  let lastResult = null;
  while (Date.now() < deadline) {
    lastResult = await visibleTextContains(page, paneId, marker);
    if (lastResult.found) return Date.now();
    await sleep(stepMs);
  }
  log(`[g2] visibleText diagnostic: ${JSON.stringify(lastResult).slice(0, 500)}`);
  return null;
}

// ── focus + shell-ready helpers ─────────────────────────────────────────
async function ensureFocus(page) {
  await page.click(".term-stage").catch(() => {});
  await page.focus("textarea.hidden-input").catch(() => {});
  await page.evaluate(() => {
    const el = document.querySelector("textarea.hidden-input");
    if (el && document.activeElement !== el) el.focus();
  }).catch(() => {});
}

async function waitForShellReady(page, timeoutMs = 5000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const ready = await page.evaluate(() => {
      const el = document.querySelector("textarea.hidden-input");
      return !!el && !el.disabled;
    }).catch(() => false);
    if (ready) return true;
    await sleep(100);
  }
  return false;
}

async function typeAndSubmit(page, text, delay = 1) {
  await ensureFocus(page);
  await waitForShellReady(page, 3000);
  await page.keyboard.type(text, { delay });
  await page.keyboard.press("Enter");
}

// ── Gate 1: PANE_BINDING ────────────────────────────────────────────────
async function gatePaneBinding({ page, dataDir }) {
  log("[g1] PANE_BINDING: verify no stale pane ref causes REMOTE_RESIZE_FAILED");
  // Check debug overlay for attachError / hostError containing REMOTE_RESIZE_FAILED
  const state = await page.evaluate(() => {
    const overlay = document.querySelector("[data-pane-debug-model]");
    const attachErr = document.querySelector("[data-attach-error]");
    const hostErr = document.querySelector("[data-host-error]");
    return {
      overlay: overlay?.textContent ?? "",
      attachError: attachErr?.textContent ?? "",
      hostError: hostErr?.textContent ?? "",
    };
  }).catch(() => ({ overlay: "", attachError: "", hostError: "" }));

  const hasResizeFailed = JSON.stringify(state).includes("REMOTE_RESIZE_FAILED");
  const hasStalePane = JSON.stringify(state).includes("does not belong");
  log(`[g1] REMOTE_RESIZE_FAILED=${hasResizeFailed} stale_pane=${hasStalePane}`);
  log(`[g1] state: ${JSON.stringify(state).slice(0, 300)}`);
  return {
    pass: !hasResizeFailed && !hasStalePane,
    detail: { hasResizeFailed, hasStalePane, state },
  };
}

// ── Gate 2: VISIBLE_MARKER ──────────────────────────────────────────────
async function gateVisibleMarker({ page, kernelPort, kernelToken, paneId, paneKey }) {
  log("[g2] VISIBLE_MARKER: input → shell → output → client model → canvas visible");
  const tag = `RIDGE_RG_VIS_${Date.now().toString(36)}`;
  const t0 = Date.now();
  await typeAndSubmit(page, `echo ${tag}`);

  // Wait for kernel scrollback (PTY_EXECUTION)
  const scrollbackSeen = await waitForScrollbackMarker(kernelPort, kernelToken, paneId, tag, 8000, 30);
  // Wait for client-model visibility (VISIBLE_MARKER) using composite pane key
  const visibleSeen = await waitForVisibleMarker(page, paneKey, tag, 8000, 50);

  log(`[g2] PTY_EXECUTION=${scrollbackSeen ? "PASS" : "FAIL"} rtMs=${scrollbackSeen ? scrollbackSeen - t0 : "n/a"}`);
  log(`[g2] VISIBLE_MARKER=${visibleSeen ? "PASS" : "FAIL"} rtMs=${visibleSeen ? visibleSeen - t0 : "n/a"}`);

  return {
    pass: !!scrollbackSeen && !!visibleSeen,
    detail: {
      tag,
      scrollbackRtMs: scrollbackSeen ? scrollbackSeen - t0 : null,
      visibleRtMs: visibleSeen ? visibleSeen - t0 : null,
    },
  };
}

// ── Gate 3: RECONNECT_RESIZE_SWITCH ─────────────────────────────────────
async function gateReconnectResizeSwitch({ browser, contextOpts, totp, kernelPort, kernelToken, paneId, dataDir }) {
  log("[g3] RECONNECT_RESIZE_SWITCH: reconnect / resize / rapid A→B→A");
  const results = { reconnect: [], resize: [], workspaceSwitch: [] };

  // Reconnect: close page, reopen, auth, type marker
  for (let i = 0; i < 3; i += 1) {
    const t0 = Date.now();
    const ctx = await browser.newContext(contextOpts);
    const page = await ctx.newPage();
    await page.goto(`https://localhost:${HOST_PORT}/_app/`, { waitUntil: "domcontentloaded" });
    try {
      const sel = 'input[maxlength="6"], input[autocomplete="one-time-code"]';
      await page.locator(sel).first().waitFor({ timeout: 8000 });
      await page.locator(sel).first().fill(totp);
      await page.locator('button').first().click();
      await sleep(500);
    } catch { /* */ }
    const pane = await waitFor(() => readFirstPaneId(dataDir), 12000, 300);
    if (pane) {
      const tag = `RIDGE_RG_RC_${i}_${Date.now().toString(36)}`;
      await typeAndSubmit(page, `echo ${tag}`);
      const seen = await waitForScrollbackMarker(kernelPort, kernelToken, pane, tag, 8000, 40);
      results.reconnect.push({ i, rtMs: seen ? seen - t0 : null, ok: !!seen });
    } else {
      results.reconnect.push({ i, rtMs: null, ok: false, err: "no pane" });
    }
    await ctx.close();
    await sleep(200);
  }

  // Resize: use the main page
  const mainPage = await browser.newContext(contextOpts).then(c => c.newPage());
  await mainPage.goto(`https://localhost:${HOST_PORT}/_app/`, { waitUntil: "domcontentloaded" });
  try {
    const sel = 'input[maxlength="6"], input[autocomplete="one-time-code"]';
    await mainPage.locator(sel).first().waitFor({ timeout: 8000 });
    await mainPage.locator(sel).first().fill(totp);
    await mainPage.locator('button').first().click();
    await sleep(500);
  } catch { /* */ }
  await mainPage.setViewportSize({ width: 1200, height: 800 });
  await sleep(500);
  await mainPage.setViewportSize({ width: 1440, height: 900 });
  await sleep(500);
  const resizeTag = `RIDGE_RG_RS_${Date.now().toString(36)}`;
  await typeAndSubmit(mainPage, `echo ${resizeTag}`);
  const resizeSeen = await waitForScrollbackMarker(kernelPort, kernelToken, paneId, resizeTag, 8000, 30);
  results.resize.push({ ok: !!resizeSeen, rtMs: resizeSeen ? "fast" : null });

  // Workspace switch: rapid A→B→A
  for (let i = 0; i < 3; i += 1) {
    const tag = `RIDGE_RG_AB_${i}_${Date.now().toString(36)}`;
    await mainPage.keyboard.press("Control+Alt+ArrowRight");
    await sleep(250);
    await mainPage.keyboard.press("Control+Alt+ArrowLeft");
    await sleep(250);
    await typeAndSubmit(mainPage, `echo ${tag}`);
    const seen = await waitForScrollbackMarker(kernelPort, kernelToken, paneId, tag, 6000, 30);
    results.workspaceSwitch.push({ i, ok: !!seen });
  }

  const reconnectOk = results.reconnect.every(r => r.ok);
  const resizeOk = results.resize.every(r => r.ok);
  const switchOk = results.workspaceSwitch.every(r => r.ok);
  log(`[g3] reconnect=${reconnectOk ? "PASS" : "FAIL"} resize=${resizeOk ? "PASS" : "FAIL"} switch=${switchOk ? "PASS" : "FAIL"}`);

  return {
    pass: reconnectOk && resizeOk && switchOk,
    detail: results,
  };
}

// ── Gate 4: SCROLLBACK_SMOKE ────────────────────────────────────────────
async function gateScrollbackSmoke({ page, kernelPort, kernelToken, paneId }) {
  log("[g4] SCROLLBACK_SMOKE: 100/500/1000/5000 lines");
  const results = {};
  for (const size of [100, 500, 1000, 5000]) {
    const tag = `RIDGE_RG_SB_S${size}_${Date.now().toString(36)}`;
    const psCmd = `1..${size} | ForEach-Object { if ($_ -eq ${size}) { '${tag}' } else { 'line-' + $_ } }`;
    const t0 = Date.now();
    await typeAndSubmit(page, psCmd, 0);
    const seen = await waitForScrollbackMarker(kernelPort, kernelToken, paneId, tag, 30000, 30);
    results[size] = { ok: !!seen, rtMs: seen ? seen - t0 : null };
    log(`[g4] size=${size} ${seen ? "PASS" : "FAIL"} rtMs=${seen ? seen - t0 : "n/a"}`);
    await sleep(250);
  }
  const allOk = Object.values(results).every(r => r.ok);
  return { pass: allOk, detail: results };
}

// ── main ────────────────────────────────────────────────────────────────
log(`[rg] Remote Release Gate starting`);

const { child: host, totp, dataDir } = await bootHost();
log(`[rg] host booted TOTP=${totp} dataDir=${dataDir}`);

const kernel = await waitFor(() => readKernel(dataDir), 20000, 300);
if (!kernel) throw new Error("kernel.json not found");
const kernelPort = kernel.port;
const kernelToken = kernel.token;
log(`[rg] kernel port=${kernelPort}`);

let caPem = null;
for (let i = 0; i < 30; i += 1) {
  try { caPem = loadHostCa(); if (caPem) break; } catch { /* */ }
  await sleep(500);
}
if (!caPem) throw new Error("host CA pem not found");
const uninstallCa = installCa(caPem);

const browser = await chromium.launch({ headless: false, slowMo: 10 });
const contextOpts = { viewport: { width: 1440, height: 900 }, locale: "en-US" };
const ctx = await browser.newContext(contextOpts);
const page = await ctx.newPage();

// Set __RIDGE_E2E__ before page load so the manager creates __windE2E
await page.addInitScript(() => {
  window.__RIDGE_E2E__ = true;
});

await page.goto(`https://localhost:${HOST_PORT}/_app/`, { waitUntil: "domcontentloaded" });
const verifySelector = 'input[maxlength="6"], input[autocomplete="one-time-code"]';
try {
  await page.locator(verifySelector).first().waitFor({ timeout: 8000 });
  await page.locator(verifySelector).first().fill(totp);
  await page.locator('button').first().click();
  await sleep(500);
} catch (e) { log(`[rg] auth err: ${String(e).slice(0, 200)}`); }

const paneRef = await waitFor(() => readFirstPaneRef(dataDir), 30000, 500);
if (!paneRef) throw new Error("no pane registered");
const paneId = paneRef.paneId;
const paneKey = paneRef.key;
log(`[rg] pane=${paneId} key=${paneKey}`);

const summary = { gates: {}, startedAt: new Date().toISOString() };

// Gate 1
summary.gates.PANE_BINDING = await gatePaneBinding({ page, dataDir });

// Gate 2
summary.gates.VISIBLE_MARKER = await gateVisibleMarker({ page, kernelPort, kernelToken, paneId, paneKey });

// Gate 3
summary.gates.RECONNECT_RESIZE_SWITCH = await gateReconnectResizeSwitch({
  browser, contextOpts, totp, kernelPort, kernelToken, paneId, dataDir,
});

// Gate 4
summary.gates.SCROLLBACK_SMOKE = await gateScrollbackSmoke({ page, kernelPort, kernelToken, paneId });

summary.endedAt = new Date().toISOString();
summary.allPass = Object.values(summary.gates).every(g => g.pass);

const outPath = join(ART_DIR, "release-gate.json");
writeFileSync(outPath, JSON.stringify(summary, null, 2));
log(`[rg] wrote ${outPath}`);
log(`[rg] ALL_PASS=${summary.allPass}`);

await ctx.close(); await browser.close();
uninstallCa();
try { host.kill?.("SIGINT"); } catch { /* */ }
setTimeout(() => host.kill?.("SIGKILL"), 2000).unref?.();
process.exit(summary.allPass ? 0 : 1);
