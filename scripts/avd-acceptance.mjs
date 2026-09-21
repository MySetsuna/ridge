// scripts/avd-acceptance.mjs
// Headless driver for AVD-based acceptance run: boots a ridge test host,
// drives a connected Android emulator (Pixel_9_Pro_XL AVD) through the
// real Web Remote flow, captures per-category screenshots and trace.
//
// AVD acceptance is automatable with adb input tap/swipe/text + screencap,
// which is normally disallowed by the no-UI-injection rule. The user has
// explicitly lifted that rule for this run. Other constraints remain:
//     - No mock output (PTY bytes come from real host)
//     - No transport bypass (still uses real L1/L2 channels)
//     - No killing the installed ridge at C:\Program Files\ridge
//     - Real approval flow (stc lock/build/verify, not hand-rolled)
//
// BETA_READY gate in this run is the AVD-OBSERVED gate, not the
// real-device gate (per user authorization "不要硬约束只有真机通过才能
// 发布了").

import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync, mkdirSync, readFileSync, readdirSync } from "node:fs";
import { tmpdir as osTmpHome } from "node:os";
import { join } from "node:path";

const REPO = "C:/code/wind";
const BIN = `${REPO}/target/test-rdg/release/ridge.exe`;
const HOST_PORT = 5120;
const AVD_HOST_ALIAS = "10.0.2.2"; // Android emulator's loopback to host
const ADB = `${process.env.LOCALAPPDATA}/Android/Sdk/platform-tools/adb.exe`;
const ARTIFACTS = `${REPO}/artifacts/release/avd-acceptance`;
mkdirSync(ARTIFACTS, { recursive: true });

const dataDir = mkdtempSync(join(osTmpHome(), "ridge-avd-"));
console.log(`[avd] data dir: ${dataDir}`);
console.log(`[avd] candidate: ${BIN}`);
console.log(`[avd] host port: ${HOST_PORT}`);
console.log(`[avd] adb path: ${ADB}`);
console.log(`[avd] artifacts: ${ARTIFACTS}`);

// ── boot ridge host ─────────────────────────────────────────────────────
let stderrBuf = "";
let child = null;

function readTotp(buf) {
  const re = /TOTP:\s*(\d{6})/g;
  let m, last = null;
  while ((m = re.exec(buf)) !== null) last = m[1];
  return last;
}

async function sleep(ms) { return new Promise((r) => setTimeout(r, ms)); }

async function bootHost() {
  if (child) {
    try { child.kill("SIGKILL"); } catch { /* gone */ }
    await sleep(500);
  }
  stderrBuf = "";
  child = spawn(BIN, ["host", "--port", String(HOST_PORT)], {
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
  let totp = null;
  for (let i = 0; i < 60; i += 1) {
    await sleep(500);
    totp = readTotp(stderrBuf);
    if (totp) break;
  }
  if (!totp) {
    console.error("[avd] FAIL host never printed TOTP");
    process.exit(1);
  }
  return totp;
}

function adb(args, opts = {}) {
  const r = spawnSync(ADB, args, { encoding: "utf8", ...opts });
  if (r.status !== 0 && !opts.allowFail) {
    console.warn(`[avd] adb ${args.join(" ")} -> ${r.status}\nstdout:${r.stdout}\nstderr:${r.stderr}`);
  }
  return r;
}

function shell(cmd, allowFail = false) {
  return adb(["shell", cmd], { allowFail });
}

async function screencap(name) {
  const remote = `/sdcard/avd-${name}.png`;
  const local = join(ARTIFACTS, `${Date.now()}-${name}.png`);
  shell(`screencap -p ${remote}`);
  const r = adb(["pull", remote, local], { allowFail: true });
  if (r.status === 0) {
    console.log(`[avd] screencap saved: ${local}`);
    return local;
  }
  console.warn(`[avd] screencap failed: ${r.stderr}`);
  return null;
}

async function waitFor(condition, timeoutMs, label) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      if (await condition()) return true;
    } catch (e) { /* try again */ }
    await sleep(500);
  }
  console.warn(`[avd] timeout: ${label}`);
  return false;
}

// ── main flow ───────────────────────────────────────────────────────────
const totp = await bootHost();
console.log(`[avd] TOTP: ${totp}`);

// Verify host is reachable from AVD
const reach = shell(`curl -sk https://${AVD_HOST_ALIAS}:${HOST_PORT} -o /dev/null -w '%{http_code}'`);
console.log(`[avd] AVD → host reach check: ${reach.stdout?.trim()}`);

// Launch Chrome with cert-error bypass for our SPKI
const spkiHash = (() => {
  // Reuse the e2e helper logic — for now grab it from cert dir
  try {
    const caDir = join(dataDir, "certs");
    const files = readdirSync(caDir);
    return files;
  } catch { return []; }
})();
console.log(`[avd] cert dir contents: ${JSON.stringify(spkiHash)}`);

// Launch Chrome on the SPA URL
const url = `https://${AVD_HOST_ALIAS}:${HOST_PORT}/`;
console.log(`[avd] opening Chrome at ${url}`);
shell(`am start -a android.intent.action.VIEW -d '${url}' -n com.android.chrome/com.google.android.apps.chrome.Main`);

await sleep(3000);
await screencap("01-chrome-open");

// Type TOTP via adb input text — lifted constraint for this run
console.log(`[avd] entering TOTP: ${totp}`);
shell(`input text '${totp}'`);
await sleep(500);
shell(`input keyevent 66`); // ENTER
await sleep(3000);
await screencap("02-after-totp");

console.log(`[avd] waiting for app shell render (uiautomator dump)`);
const shellUp = await waitFor(async () => {
  const r = shell(`uiautomator dump /sdcard/ui.xml`);
  if (r.status !== 0) return false;
  shell(`cat /sdcard/ui.xml`);
  return /AppShell|terminal|RidgePane/i.test(r.stdout || "") || shell("uiautomator dump /sdcard/ui2.xml && cat /sdcard/ui2.xml", true).stdout?.includes("terminal");
}, 30000, "app shell render");

if (!shellUp) console.warn(`[avd] app shell not detected — continuing anyway`);

// Save TOTP + host info as artifacts
writeFileSync(join(ARTIFACTS, "host-info.json"), JSON.stringify({
  totp,
  hostPort: HOST_PORT,
  avdHostAlias: AVD_HOST_ALIAS,
  dataDir,
  bootTime: new Date().toISOString(),
}, null, 2));

console.log(`[avd] ready — run interactive categories via separate driver or test scripts`);
console.log(`[avd] artifacts: ${ARTIFACTS}`);
console.log(`[avd] TOTP: ${totp}`);

// Keep host alive for follow-up adb input commands
process.on("SIGINT", () => {
  console.log("[avd] SIGINT — cleaning up");
  if (child) try { child.kill("SIGINT"); } catch { /* */ }
  process.exit(0);
});

// Idle so follow-up tool calls can issue adb commands through this script
await sleep(300000);
console.log("[avd] idle timeout — exiting");
if (child) try { child.kill("SIGINT"); } catch { /* */ }
process.exit(0);