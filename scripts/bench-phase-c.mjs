// scripts/bench-phase-c.mjs
// CHG-050 Phase C — Route B: kernel scrollback API as marker completion signal.
//
// Measurement scope (per goal):
//   browser input → host/kernel → PTY → shell → kernel output/scrollback
//
// Excludes: client canvas render. DOM scan, mock output, direct terminal text
// injection are forbidden. Marker detection is a real HTTP GET against the
// kernel's /v1/domain/ptys/{id}?max_bytes=N scrollback endpoint.
//
// Output metrics (5 buckets):
//   1. input_to_shell       — short echo rtMs (browser keypress → scrollback marker)
//   2. shell_to_output      — large output rtMs (browser keypress → last line in scrollback)
//   3. reconnect            — page close → reopen → auth → first scrollback marker
//   4. A→B→A                — workspace switch round-trip (NOT_RUN if UI path unavailable)
//   5. lines_N (N=100/500/1000/5000) — shell_to_output per size
//
// Frozen layers: RTP1 / Kernel PTY ownership / auth / wire / workspace identity.
// The script does not touch production code.

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
const LABEL = process.env.RIDGE_BENCH_LABEL ?? "phase-c";
const SAMPLES = Number(process.env.RIDGE_BENCH_SAMPLES ?? "30");
const RECONNECT_SAMPLES = Number(process.env.RIDGE_RECONNECT_SAMPLES ?? "10");
const OUTPUT_SIZES = (process.env.RIDGE_OUTPUT_SIZES ?? "100,500,1000,5000")
  .split(",").map((s) => parseInt(s, 10));
const ART_DIR = join(ROOT, "artifacts/release/latency");
mkdirSync(ART_DIR, { recursive: true });

function log(s) { console.log(s); }
function percentile(arr, p) {
  if (!arr.length) return null;
  const i = Math.min(arr.length - 1, Math.floor((p / 100) * arr.length));
  return arr[i];
}
function stats(rtArr, timeouts = 0) {
  const sorted = rtArr.filter((x) => x !== null).sort((a, b) => a - b);
  return {
    count: sorted.length,
    min: sorted[0] ?? null,
    p50: percentile(sorted, 50),
    p95: percentile(sorted, 95),
    p99: percentile(sorted, 99),
    max: sorted[sorted.length - 1] ?? null,
    avg: sorted.length ? Math.round(sorted.reduce((a, b) => a + b, 0) / sorted.length) : null,
    timeouts,
  };
}

// ── host boot ───────────────────────────────────────────────────────────
async function bootHost() {
  const dataDir = mkdtempSync(join(tmpdir(), "ridge-pc-"));
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
  // RIDGE_KERNEL_DATA_DIR points at the kernel data dir; kernel.json sits at its root.
  const p = join(dataDir, "kernel.json");
  if (!existsSync(p)) return null;
  try { return JSON.parse(readFileSync(p, "utf8")); } catch { return null; }
}

function readFirstPaneId(dataDir) {
  const p = join(dataDir, "workspace-graph.json");
  if (!existsSync(p)) return null;
  try {
    const g = JSON.parse(readFileSync(p, "utf8"));
    for (const ws of Object.values(g.workspaces ?? {})) {
      const leaf = ws?.pane_tree?.root?.Leaf;
      if (typeof leaf === "string") return leaf;
    }
  } catch { /* */ }
  return null;
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
  const caPath = join(ART_DIR, `pc-ca-${Date.now()}.pem`);
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
  } catch {
    return false;
  }
}

async function waitForScrollbackMarker(kernelPort, token, paneId, marker, timeoutMs = 6000, stepMs = 40) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const hit = await scrollbackContains(kernelPort, token, paneId, marker, 65536);
    if (hit) return Date.now();
    await sleep(stepMs);
  }
  return null;
}

// ── focus + shell-ready helpers ─────────────────────────────────────────
async function ensureFocus(page) {
  // Click the terminal container first to guarantee DOM focus is in the
  // terminal region, then focus the hidden IME input. Re-assert after a
  // frame — the SPA may re-render and steal focus.
  await page.click(".term-stage").catch(() => {});
  await page.focus("textarea.hidden-input").catch(() => {});
  await page.evaluate(() => {
    const el = document.querySelector("textarea.hidden-input");
    if (el && document.activeElement !== el) el.focus();
  }).catch(() => {});
}

async function waitForShellReady(page, timeoutMs = 5000) {
  // The hidden input is disabled when `bindingUncertain` (anchor not yet
  // resolved). Poll until it is enabled before typing.
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

// ── scenarios ───────────────────────────────────────────────────────────
async function measureInputToShell({ page, kernelPort, token, paneId, samples }) {
  const out = [];
  for (let i = 0; i < samples; i += 1) {
    const tag = `RIDGE_PC_IS_${i}_${Date.now().toString(36)}`;
    const t0 = Date.now();
    await typeAndSubmit(page, `echo ${tag}`);
    const seen = await waitForScrollbackMarker(kernelPort, token, paneId, tag, 6000, 30);
    out.push({ i, tag, rtMs: seen ? seen - t0 : null });
    if (!seen) log(`  [is] sample ${i} TIMEOUT`);
    await sleep(150);
  }
  return out;
}

async function measureShellToOutput({ page, kernelPort, token, paneId, size, samples }) {
  const out = [];
  for (let i = 0; i < samples; i += 1) {
    // pwsh.exe: iterate 1..size and emit `line-<n>` per row.
    // Marker is the LAST line so detection covers the full output sweep.
    const lastTag = `RIDGE_PC_SO_S${size}_${i}_${Date.now().toString(36)}`;
    const psCmd = `1..${size} | ForEach-Object { if ($_ -eq ${size}) { '${lastTag}' } else { 'line-' + $_ } }`;
    const t0 = Date.now();
    await typeAndSubmit(page, psCmd, 0);
    const seen = await waitForScrollbackMarker(kernelPort, token, paneId, lastTag, 30000, 30);
    out.push({ i, size, tag: lastTag, rtMs: seen ? seen - t0 : null });
    if (!seen) log(`  [so s=${size}] sample ${i} TIMEOUT`);
    await sleep(250);
  }
  return out;
}

async function measureReconnect({ browser, contextOpts, pageOpts, totp, kernelPort, token, paneId, samples }) {
  const out = [];
  for (let i = 0; i < samples; i += 1) {
    const t0 = Date.now();
    const ctx = await browser.newContext(contextOpts);
    const page = await ctx.newPage();
    await page.goto(`https://localhost:${HOST_PORT}/_app/`, { waitUntil: "domcontentloaded" });
    // Auth gate
    try {
      const sel = 'input[maxlength="6"], input[autocomplete="one-time-code"], input[name="totp"]';
      await page.locator(sel).first().waitFor({ timeout: 8000 });
      await page.locator(sel).first().fill(totp);
      await page.locator('button').first().click();
      await sleep(500);
    } catch { /* may be already auth */ }
    // Wait for shell + pane
    const pane = await waitFor(() => readFirstPaneId(process.env.RIDGE_PC_DATADIR ?? ""), 12000, 300);
    if (!pane) {
      out.push({ i, rtMs: null, err: "no pane after auth" });
      await ctx.close();
      continue;
    }
    // Type a probe marker and wait for scrollback marker
    const tag = `RIDGE_PC_RC_${i}_${Date.now().toString(36)}`;
    await typeAndSubmit(page, `echo ${tag}`);
    const seen = await waitForScrollbackMarker(kernelPort, token, pane, tag, 8000, 40);
    out.push({ i, rtMs: seen ? seen - t0 : null, err: seen ? null : "marker timeout" });
    await ctx.close();
    await sleep(200);
  }
  return out;
}

async function measureWorkspaceSwitch({ page, kernelPort, token, paneId, samples }) {
  // Workspace switching UI is controlled by the SPA; expose the same keyboard
  // shortcuts the app binds (Ctrl+Alt+ArrowRight / Ctrl+Alt+ArrowLeft) and
  // measure round-trip A→B→A via scrollback marker on the SAME pane (the
  // marker proves the pane re-attached after switch).
  const out = [];
  for (let i = 0; i < samples; i += 1) {
    const tag = `RIDGE_PC_AB_${i}_${Date.now().toString(36)}`;
    const t0 = Date.now();
    try {
      await page.keyboard.press("Control+Alt+ArrowRight");
      await sleep(250);
      await page.keyboard.press("Control+Alt+ArrowLeft");
      await sleep(250);
      await typeAndSubmit(page, `echo ${tag}`);
      const seen = await waitForScrollbackMarker(kernelPort, token, paneId, tag, 6000, 30);
      out.push({ i, rtMs: seen ? seen - t0 : null, err: seen ? null : "marker timeout" });
    } catch (e) {
      out.push({ i, rtMs: null, err: String(e).slice(0, 120) });
    }
    await sleep(250);
  }
  return out;
}

// ── main ────────────────────────────────────────────────────────────────
log(`[pc] label=${LABEL} samples=${SAMPLES} reconnect=${RECONNECT_SAMPLES} sizes=${OUTPUT_SIZES.join("|")} port=${HOST_PORT}`);

const { child: host, totp, dataDir } = await bootHost();
log(`[pc] host booted TOTP=${totp} dataDir=${dataDir}`);
process.env.RIDGE_PC_DATADIR = dataDir;

const kernel = await waitFor(() => readKernel(dataDir), 20000, 300);
if (!kernel) throw new Error("kernel.json not found within 20s");
const kernelPort = kernel.port;
const kernelToken = kernel.token;
log(`[pc] kernel port=${kernelPort} token=${kernelToken}`);

let caPem = null;
for (let i = 0; i < 30; i += 1) {
  try { caPem = loadHostCa(); if (caPem) break; } catch { /* */ }
  await sleep(500);
}
if (!caPem) throw new Error("host CA pem not found");
const uninstallCa = installCa(caPem);
process.on("exit", uninstallCa);
process.on("SIGINT", () => { uninstallCa(); process.exit(130); });

const browser = await chromium.launch({ headless: false, slowMo: 10 });
const contextOpts = { viewport: { width: 1440, height: 900 }, locale: "en-US" };
const ctx = await browser.newContext(contextOpts);
const page = await ctx.newPage();

await page.goto(`https://localhost:${HOST_PORT}/_app/`, { waitUntil: "domcontentloaded" });
// AuthScreen.svelte's TOTP field is a <input type="text" inputmode="numeric" maxlength={6}>.
// match by maxlength to be robust to name/attr refactors.
const verifySelector = 'input[maxlength="6"], input[autocomplete="one-time-code"], input[name="totp"]';
try {
  await page.locator(verifySelector).first().waitFor({ timeout: 8000 });
  await page.locator(verifySelector).first().fill(totp);
  await page.locator('button').first().click();
  await sleep(500);
} catch (e) { log(`[pc] auth fill/click err: ${String(e).slice(0, 200)}`); }

// Wait for pane to be registered by the SPA
const paneId = await waitFor(() => readFirstPaneId(dataDir), 30000, 500);
if (!paneId) {
  log(`[pc] ERR no pane registered. URL=${page.url()}`);
  // Capture SPA-visible state for diagnosis (avoid screenshot — playwright's
  // fullPage capture can surface an unrelated browser tab; the page HTML is
  // what actually tells us which screen the SPA is showing).
  const bodyText = await page.evaluate(() => document.body?.innerText ?? "").catch(() => "");
  log(`[pc] bodyText head: ${JSON.stringify(bodyText.slice(0, 600))}`);
  const hasTerm = await page.evaluate(() => !!document.querySelector(".term-stage")).catch(() => false);
  const hasAuth = await page.evaluate(() => !!document.querySelector('input[name="totp"]')).catch(() => false);
  log(`[pc] term-stage=${hasTerm} auth-input=${hasAuth}`);
  throw new Error("no pane registered by SPA within 30s");
}
log(`[pc] pane=${paneId}`);

// Sanity: trigger a probe and ensure the marker appears in kernel scrollback
const probe = `RIDGE_PC_PROBE_${Date.now().toString(36)}`;
const probeT0 = Date.now();
await typeAndSubmit(page, `echo ${probe}`);
const probeSeen = await waitForScrollbackMarker(kernelPort, kernelToken, paneId, probe, 8000, 30);
log(`[pc] sanity probe ${probeSeen ? "OK" : "FAIL"} rtMs=${probeSeen ? probeSeen - probeT0 : "n/a"}`);
if (!probeSeen) {
  log("[pc] sanity failed — aborting; marker never arrived in kernel scrollback");
  await ctx.close(); await browser.close(); uninstallCa();
  host.kill?.("SIGINT");
  process.exit(2);
}

const summary = {
  label: LABEL,
  hostPort: HOST_PORT,
  kernelPort,
  paneId,
  samples: SAMPLES,
  metrics: {},
  startedAt: new Date().toISOString(),
};

log(`[pc] metric=input_to_shell (${SAMPLES} samples)`);
summary.metrics.input_to_shell = { samples: await measureInputToShell({ page, kernelPort, kernelToken, paneId, samples: SAMPLES }) };
summary.metrics.input_to_shell.stats = stats(summary.metrics.input_to_shell.samples.map((s) => s.rtMs), summary.metrics.input_to_shell.samples.filter((s) => s.rtMs === null).length);

log(`[pc] metric=shell_to_output per size (${SAMPLES} samples each)`);
summary.metrics.shell_to_output = { bySize: {} };
for (const size of OUTPUT_SIZES) {
  const s = await measureShellToOutput({ page, kernelPort, kernelToken, paneId, size, samples: SAMPLES });
  summary.metrics.shell_to_output.bySize[size] = {
    samples: s,
    stats: stats(s.map((x) => x.rtMs), s.filter((x) => x.rtMs === null).length),
  };
  log(`  [so s=${size}] p50=${summary.metrics.shell_to_output.bySize[size].stats.p50}ms`);
}

log(`[pc] metric=reconnect (${RECONNECT_SAMPLES} samples)`);
summary.metrics.reconnect = { samples: await measureReconnect({ browser, contextOpts, pageOpts: {}, totp, kernelPort, kernelToken, paneId, samples: RECONNECT_SAMPLES }) };
summary.metrics.reconnect.stats = stats(summary.metrics.reconnect.samples.map((s) => s.rtMs), summary.metrics.reconnect.samples.filter((s) => s.rtMs === null).length);

log(`[pc] metric=A→B→A (${SAMPLES} samples)`);
summary.metrics.workspace_switch = { samples: await measureWorkspaceSwitch({ page, kernelPort, kernelToken, paneId, samples: SAMPLES }) };
summary.metrics.workspace_switch.stats = stats(summary.metrics.workspace_switch.samples.map((s) => s.rtMs), summary.metrics.workspace_switch.samples.filter((s) => s.rtMs === null).length);

summary.endedAt = new Date().toISOString();
const outPath = join(ART_DIR, `${LABEL}-phase-c.json`);
writeFileSync(outPath, JSON.stringify(summary, null, 2));
log(`[pc] wrote ${outPath}`);

await ctx.close(); await browser.close();
uninstallCa();
try { host.kill?.("SIGINT"); } catch { /* */ }
setTimeout(() => host.kill?.("SIGKILL"), 2000).unref?.();
process.exit(0);
