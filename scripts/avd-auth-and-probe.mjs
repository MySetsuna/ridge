// scripts/avd-auth-and-probe.mjs
// Integrated driver:
//    - Spawn fresh test-rdg host (own data dir, port 5120)
//    - Capture TOTP from stderr
//    - Force-stop chrome + restart with URL (loads SPA fresh with SPKI pin)
//    - Wait for SPA hydration (page renders "Ridge Remote" gate)
//    - Use input swipe with 100ms dwell for clicks (input tap is too fast for
//      Svelte 5 delegated event handlers — page doesn't register click)
//    - Type TOTP + swipe Verify button + screencap to verify transition
//
// All within the 30s TOTP window.

import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync, mkdirSync } from "node:fs";
import { tmpdir as osTmpHome } from "node:os";
import { join } from "node:path";

const REPO = "C:/code/wind";
const BIN = `${REPO}/target/test-rdg/release/ridge.exe`;
const HOST_PORT = 5120;
const AVD_HOST_ALIAS = "10.0.2.2";
const ADB = `${process.env.LOCALAPPDATA}/Android/Sdk/platform-tools/adb.exe`;
const ARTIFACTS = `${REPO}/artifacts/release/avd-acceptance`;
mkdirSync(ARTIFACTS, { recursive: true });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function adb(args, opts = {}) {
  return spawnSync(ADB, args, { encoding: "utf8", ...opts });
}

async function swipeTap(x, y, dwell = 120) {
  // `input swipe X Y X Y MS` — Chrome registers dwell-time clicks;
  // plain `input tap` doesn't trigger Svelte 5 button onclick on Android.
  adb(["shell", `input swipe ${x} ${y} ${x} ${y} ${dwell}`]);
  await sleep(150);
}

async function typeText(text) {
  adb(["shell", `input text ${text}`]);
  await sleep(100);
}

async function screencap(name) {
  const remote = `//sdcard/avd-${name}.png`;
  const local = join(ARTIFACTS, `${Date.now()}-${name}.png`);
  adb(["shell", `screencap -p ${remote}`], { allowFail: true });
  const r = adb(["pull", remote, local], { allowFail: true });
  if (r.status === 0) console.log(`[avd] screencap: ${local}`);
  return local;
}

// ── boot host ────────────────────────────────────────────────────────────
const dataDir = mkdtempSync(join(osTmpHome(), "ridge-avd-"));
console.log(`[avd] data dir: ${dataDir}`);

let stderrBuf = "";
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
child.stdout.on("data", (d) => process.stdout.write(d));
child.stderr.on("data", (d) => {
  const s = d.toString();
  stderrBuf += s;
  process.stderr.write(d);
});

function readTotp(buf) {
  const re = /TOTP:\s*(\d{6})/g;
  let m, last = null;
  while ((m = re.exec(buf)) !== null) last = m[1];
  return last;
}

let totp = null;
for (let i = 0; i < 60; i += 1) {
  await sleep(250);
  totp = readTotp(stderrBuf);
  if (totp) break;
}
if (!totp) {
  console.error("[avd] FAIL host never printed TOTP");
  try { child.kill("SIGKILL"); } catch { /* */ }
  process.exit(1);
}
const totpTime = Date.now();
console.log(`[avd] TOTP captured: ${totp}`);

// ── drive auth sequence within 30s window ───────────────────────────────
console.log(`[avd] force-stop chrome + navigate fresh...`);
adb(["shell", "am force-stop com.android.chrome"]);
await sleep(1500);
adb(["shell", `am start -a android.intent.action.VIEW -n com.android.chrome/com.google.android.apps.chrome.Main -d 'https://${AVD_HOST_ALIAS}:${HOST_PORT}/'`]);
await sleep(7000); // SPA render (cold-start needs extra)

await screencap("01-fresh-load");

console.log(`[avd] tap input field with 120ms dwell...`);
// Re-tap to be sure focus sticks; first tap may land on chrome while
// the SPA is still loading.
await swipeTap(674, 1610); // input field
await sleep(300);
await swipeTap(674, 1610);
await sleep(300);

console.log(`[avd] type TOTP ${totp}...`);
await typeText(totp);
await sleep(500);
await screencap("02-totp-typed");

console.log(`[avd] dismiss keyboard with Back key...`);
// KEYCODE_BACK = 4 — dismisses soft keyboard without losing focus context.
adb(["shell", "input keyevent 4"]);
await sleep(500);

console.log(`[avd] tap submit button with 150ms dwell (uiautomator-verified coords)...`);
// uiautomator dump: Verify & Connect button bounds=[237,1821][1104,1968] →
// center (672, 1894). With keyboard up the form was compressed; back-key
// gives the canonical (post-render) layout. input swipe X Y X Y MS triggers
// Svelte 5 delegated click handlers (plain `input tap` doesn't on AVD).
await swipeTap(672, 1894, 150);
await sleep(6000); // wait for fetch + maybe transition
await screencap("03-after-submit");

const totpAgeAtEnter = Date.now() - totpTime;
console.log(`[avd] total time from TOTP capture: ${totpAgeAtEnter}ms (window 30s)`);

writeFileSync(join(ARTIFACTS, "auth-log.json"), JSON.stringify({
  totp,
  totpAgeMsAtEnter: Date.now() - totpTime,
}, null, 2));

console.log(`[avd] done — host kept alive for follow-up categories`);

// Hold host alive for follow-up adb commands
process.on("SIGINT", () => {
  try { child.kill("SIGINT"); } catch { /* */ }
  process.exit(0);
});

await sleep(300000);
try { child.kill("SIGINT"); } catch { /* */ }
process.exit(0);