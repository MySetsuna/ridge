// scripts/avd-auth-once.mjs
// One-shot: spawn fresh host, capture TOTP, drive AVD to Verify & Connect.
// All within the 30s TOTP window. Coordinate mapping: 1.50x displayed→native.
import { spawn, spawnSync } from "node:child_process";
import { writeFileSync, mkdirSync, openSync, readFileSync } from "node:fs";
import { join } from "node:path";

const REPO = "C:/code/wind";
const ART = `${REPO}/artifacts/release/avd-c3-c6`;
const ADB = `${process.env.LOCALAPPDATA}/Android/Sdk/platform-tools/adb.exe`;
const HOST = `${REPO}/target/test-rdg/release/ridge.exe`;
mkdirSync(ART, { recursive: true });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const adb = (args) => spawnSync(ADB, args, { encoding: "utf8" });
const screencap = (name) => {
  const remote = `//sdcard/avd-${name}.png`;
  const local = join(ART, `${name}.png`);
  adb(["shell", `screencap -p ${remote}`]);
  adb(["pull", remote, local]);
  return local;
};
const tap = (x, y, dwell = 250) => {
  adb(["shell", `input swipe ${x} ${y} ${x} ${y} ${dwell}`]);
};

// 1. Spawn host, capture TOTP from stderr.
const dataDir = `C:\\Users\\12867\\AppData\\Local\\Temp\\ridge-avd-c3c6-once-${Date.now()}`;
const env = { ...process.env, RIDGE_KERNEL_DATA_DIR: dataDir, RIDGE_PRINT_TOTP: "1" };
const logPath = join(ART, "host-once.log");
const out = openSync(logPath, "w");
const proc = spawn(HOST, ["host", "--port", "5120"], { env, stdio: ["ignore", out, out], detached: true });
proc.unref();
console.log(`[once] host pid=${proc.pid} dataDir=${dataDir}`);

// Wait for TOTP.
let totp = null;
for (let i = 0; i < 50; i++) {
  await sleep(100);
  const buf = readFileSync(logPath, "utf8");
  const m = buf.match(/TOTP:\s*(\d{6})/);
  if (m) { totp = m[1]; break; }
}
if (!totp) { console.error("[once] no TOTP"); process.exit(1); }
console.log(`[once] TOTP=${totp}`);

// 2. Force-stop Chrome and start fresh against host.
adb(["shell", "am", "force-stop", "com.android.chrome"]);
await sleep(800);
adb(["shell", "am", "start", "-a", "android.intent.action.VIEW",
     "-n", "com.android.chrome/com.google.android.apps.chrome.Main",
     "-d", "https://10.0.2.2:5120/"]);
await sleep(5000); // SPA gate renders ~2s; let notification dialog settle
screencap("a02-reload");

// 2b. Dismiss Chrome "Notifications make things easier" dialog with BACK.
adb(["shell", "input", "keyevent", "4"]);
await sleep(600);
screencap("a02b-after-back");

// 3. Tap input field twice (120ms dwell each) — first tap may land while SPA
//    still loading; second confirms focus.
console.log("[once] tap input field twice...");
tap(674, 1610, 120);
await sleep(200);
tap(674, 1610, 120);
await sleep(200);

// 4. Type TOTP.
adb(["shell", "input", "text", totp]);
await sleep(300);
screencap("a04-typed");

// 5. BACK to dismiss keyboard (with focus in input field, BACK only hides
//    soft keyboard without navigating Chrome).
adb(["shell", "input", "keyevent", "4"]);
await sleep(300);
screencap("a05a-after-back");

// 6. Tap Verify & Connect at canonical coords (672, 1894 native).
tap(672, 1894, 150);
await sleep(3500);
screencap("a05-after-verify");

// 6. Save totp file.
writeFileSync(join(ART, "auth-totp.json"), JSON.stringify({ totp, ts: Math.floor(Date.now()/1000), hostPid: proc.pid }, null, 2));
console.log(`[once] done — totp=${totp} hostPid=${proc.pid}`);
