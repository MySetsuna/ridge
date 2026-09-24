// scripts/bench-latency-4scenes.mjs
// CHG-050 Phase B — controlled echo driver for 4 input latency scenes.
//
// Reuses the headed-Playwright + per-user-CA pattern from
// scripts/headed-desktop-e2e.mjs but in a tighter loop:
//   1. boot a transient host on $RIDGE_HOST_PORT (default 5121)
//   2. install its CA into CurrentUser\Root (then uninstall)
//   3. launch headed chromium, auth once with TOTP
//   4. for each scene × N samples: type marker → waitForMarker → record rtMs
//   5. write JSON to artifacts/release/latency/<label>-<scene>.json
//
// Hard constraints (from goal):
//   - real keystroke → real WS → real PTY → real echo → real DOM scan
//   - no fake timers, no mocked transport, no transport-ack as proxy
//   - installed ridge service (C:\Program Files\ridge) is untouched
//
// Usage:
//   RIDGE_BIN=target/debug/ridge.exe \
//     node scripts/bench-latency-4scenes.mjs \
//       --label default --samples 100 --scenes idle,heavy,firstopen,switchback
//
// Toggle instrumentation via RIDGE_KERNEL_TRACE / RIDGE_HOST_TRACE.

import { chromium } from "@playwright/test";
import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { loadHostCa, hostCertSpkiSha256 } from "./tls-host.mjs";

const ROOT = resolve(".");
const BIN = process.env.RIDGE_BIN ?? "target/debug/ridge.exe";
const HOST_PORT = Number(process.env.RIDGE_BENCH_HOST_PORT ?? "5121");
const SAMPLES = Number(process.env.RIDGE_BENCH_SAMPLES ?? "100");
const LABEL = process.env.RIDGE_BENCH_LABEL ?? "default";
const ART_DIR = join(ROOT, "artifacts/release/latency");
mkdirSync(ART_DIR, { recursive: true });

// ── host boot (mirrors headed-desktop-e2e bootHost) ─────────────────────
async function bootHost() {
  const dataDir = mkdtempSync(join(tmpdir(), "ridge-bench-"));
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

// ── CA install (CurrentUser\Root, mirrors headed-desktop-e2e) ────────────
function installCa(caPem) {
  const caPath = join(ART_DIR, `bench-ca-${Date.now()}.pem`);
  writeFileSync(caPath, caPem);
  const r = spawnSync("certutil", ["-user", "-addstore", "Root", caPath], { encoding: "utf8" });
  if (r.status !== 0) throw new Error(`certutil addstore failed: ${r.stderr}`);
  return () => {
    try { spawnSync("certutil", ["-user", "-delstore", "Root", caPath], { encoding: "utf8" }); } catch { /* */ }
  };
}

function log(s) { console.log(s); }

// ── per-scene sample loop ────────────────────────────────────────────────
// Each scene measures a distinct interaction:
//   idle        → marker typed into the existing pane, no extra load
//   heavy       → marker typed while a 1000-line background stream is filling
//   firstopen   → marker typed into a freshly opened pane (per sample)
//   switchback  → marker typed after re-focusing an existing pane
async function runScene({ page, scene, samples }) {
  const tag0 = `RIDGE_BENCH_${scene.toUpperCase()}`;
  const out = [];
  for (let i = 0; i < samples; i += 1) {
    // Per-sample prep
    if (scene === "heavy" && i % 50 === 0 && i > 0) {
      const loadTag = `RIDGE_BG_${Date.now().toString(36)}`;
      await page.keyboard.type(
        `(yes "${loadTag}" 2>/dev/null | head -1000 &) ; echo __BG__`,
        { delay: 2 },
      );
      await page.keyboard.press("Enter");
      await sleep(1500);
    }
    if (scene === "firstopen") {
      await page.keyboard.press("Control+Shift+T").catch(() => {});
      await sleep(300);
    }
    if (scene === "switchback") {
      // Click an existing pane in the layout (not the active one) to switch focus.
      const stages = page.locator(".term-stage");
      const count = await stages.count();
      if (count > 1) {
        await stages.nth(i % count).click({ position: { x: 40, y: 40 } }).catch(() => {});
        await sleep(250);
      }
    }
    const tag = `${tag0}_${i.toString(36)}_${Date.now().toString(36)}`;
    await page.focus("textarea.hidden-input").catch(() => {});
    const tType = Date.now();
    await page.keyboard.type(`echo ${tag}`, { delay: 3 });
    await page.keyboard.press("Enter");
    const seen = await waitForMarker(page, tag, 6_000);
    const rtMs = seen ? seen.t - tType : null;
    out.push({ i, tag, rtMs, via: seen?.via ?? null });
    if (!seen) log(`  [bench] sample ${i}: TIMEOUT (6s) tag=${tag}`);
  }
  return out;
}

async function waitForMarker(page, tag, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const body = await page.evaluate(() => document.body?.innerText ?? "").catch(() => "");
    if (body.includes(tag)) return { t: Date.now(), via: "dom" };
    await sleep(60);
  }
  return null;
}

function percentile(sortedArr, p) {
  if (!sortedArr.length) return null;
  const idx = Math.min(sortedArr.length - 1, Math.floor((p / 100) * sortedArr.length));
  return sortedArr[idx];
}

// ── main ─────────────────────────────────────────────────────────────────
const scenes = (process.env.RIDGE_BENCH_SCENES ?? "idle,heavy,firstopen,switchback").split(",");
log(`[bench] label=${LABEL} samples=${SAMPLES} scenes=${scenes.join("|")} port=${HOST_PORT}`);

const { child: host, totp, dataDir } = await bootHost();
log(`[bench] host booted, TOTP=${totp} dataDir=${dataDir}`);

// Wait for CA pem
let caPem = null;
for (let i = 0; i < 30; i += 1) {
  try { caPem = loadHostCa(); if (caPem) break; } catch { /* */ }
  await sleep(500);
}
if (!caPem) throw new Error("host CA pem not found");
const uninstallCa = installCa(caPem);

process.on("exit", uninstallCa);
process.on("SIGINT", () => { uninstallCa(); process.exit(130); });
process.on("SIGTERM", () => { uninstallCa(); process.exit(143); });

// Launch headed chromium
const browser = await chromium.launch({ headless: false, slowMo: 30 });
const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, locale: "en-US" });
const page = await context.newPage();

// Auth gate: type TOTP into the verify form, then wait for shell ready.
await page.goto(`https://localhost:${HOST_PORT}/_app/`, { waitUntil: "domcontentloaded" });
const verifySelector = 'input[name="totp"], input[autocomplete="one-time-code"]';
try {
  await page.locator(verifySelector).first().waitFor({ timeout: 8_000 });
  await page.locator(verifySelector).first().fill(totp);
  await page.keyboard.press("Enter");
} catch { /* may be already-authenticated */ }

// Wait for xterm.js terminal stage
const tReadyDeadline = Date.now() + 12_000;
while (Date.now() < tReadyDeadline) {
  const ready = await page.evaluate(() => {
    const t = document.querySelector(".term-stage");
    return t && t.getAttribute("tabindex") !== null;
  }).catch(() => false);
  if (ready) break;
  await sleep(150);
}
log(`[bench] shell ready, running scenes`);

// Run each scene
const summary = { label: LABEL, hostPort: HOST_PORT, samples: SAMPLES, scenes: {} };
for (const scene of scenes) {
  log(`[bench] scene=${scene} starting (${SAMPLES} samples)`);
  const samples = await runScene({ page, scene, samples: SAMPLES });
  const rtArr = samples.map((s) => s.rtMs).filter((x) => x !== null).sort((a, b) => a - b);
  const stats = {
    count: rtArr.length,
    min: rtArr[0] ?? null,
    p50: percentile(rtArr, 50),
    p95: percentile(rtArr, 95),
    p99: percentile(rtArr, 99),
    max: rtArr[rtArr.length - 1] ?? null,
    avg: rtArr.length ? Math.round(rtArr.reduce((a, b) => a + b, 0) / rtArr.length) : null,
    timeouts: samples.length - rtArr.length,
  };
  summary.scenes[scene] = { stats, samples };
  log(`[bench] scene=${scene} done p50=${stats.p50}ms p95=${stats.p95}ms p99=${stats.p99}ms max=${stats.max}ms timeouts=${stats.timeouts}`);
}

const outPath = join(ART_DIR, `${LABEL}-bench.json`);
writeFileSync(outPath, JSON.stringify(summary, null, 2));
log(`[bench] wrote ${outPath}`);

// Cleanup
try { host.kill?.("SIGINT"); } catch { /* */ }
setTimeout(() => host.kill?.("SIGKILL"), 2000).unref?.();
await context.close();
await browser.close();
uninstallCa();
process.exit(0);