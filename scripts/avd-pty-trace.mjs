// scripts/avd-pty-trace.mjs
// Spawn fresh test-rdg host with RIDGE_TRACE=1, drive AVD auth, send marker via shell,
// capture all [ridge-trace] lines from host log.
import { spawn, spawnSync } from "node:child_process";
import { writeFileSync, mkdirSync, openSync, readFileSync } from "node:fs";
import { join } from "node:path";

const REPO = "C:/code/wind";
const ART = `${REPO}/artifacts/release/avd-pty`;
const ADB = `${process.env.LOCALAPPDATA}/Android/Sdk/platform-tools/adb.exe`;
const HOST = `${REPO}/target/test-rdg/release/ridge.exe`;
mkdirSync(ART, { recursive: true });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const adb = (args) => spawnSync(ADB, args, { encoding: "utf8", timeout: 15_000 });
const screencap = (name) => {
  const remote = `//sdcard/avd-pty-${name}.png`;
  const local = join(ART, `${name}.png`);
  adb(["shell", `screencap -p ${remote}`]);
  adb(["pull", remote, local]);
  return local;
};
const tap = (x, y, dwell = 250) => {
  adb(["shell", `input swipe ${x} ${y} ${x} ${y} ${dwell}`]);
};

// 1. Spawn host with RIDGE_TRACE=1.
const dataDir = `C:\\Users\\12867\\AppData\\Local\\Temp\\ridge-pty-trace-${Date.now()}`;
const env = {
  ...process.env,
  RIDGE_KERNEL_DATA_DIR: dataDir,
  RIDGE_PRINT_TOTP: "1",
  RIDGE_TRACE: "1",
};
const logPath = join(ART, "host-trace.log");
const out = openSync(logPath, "w");
const proc = spawn(HOST, ["host", "--port", "5120"], {
  env, stdio: ["ignore", out, out], detached: true,
});
proc.unref();
console.log(`[pty] host pid=${proc.pid} log=${logPath}`);

let totp = null;
for (let i = 0; i < 50; i++) {
  await sleep(100);
  const buf = readFileSync(logPath, "utf8");
  const m = buf.match(/TOTP:\s*(\d{6})/);
  if (m) { totp = m[1]; break; }
}
if (!totp) { console.error("[pty] no TOTP"); process.exit(1); }
console.log(`[pty] TOTP=${totp}`);

writeFileSync(join(ART, "trace-totp.json"), JSON.stringify({ totp, ts: Math.floor(Date.now()/1000), hostPid: proc.pid }, null, 2));

// 2. Clear Chrome completely (caches + service worker + localStorage) then start URL.
adb(["shell", "pm", "clear", "com.android.chrome"]);
await sleep(800);
adb(["shell", "am", "start", "-a", "android.intent.action.VIEW",
     "-n", "com.android.chrome/com.google.android.apps.chrome.Main",
     "-d", "https://10.0.2.2:5120/"]);
await sleep(6000); // SPA gate renders ~2s; let notification dialog settle
screencap("01-loaded");

// Dismiss Chrome "Notifications make things easier" dialog with BACK.
// (pm clear puts us on a clean chrome session; the notif dialog appears once.)
adb(["shell", "input", "keyevent", "4"]);
await sleep(600);
screencap("02-after-back");

// After BACK, focus the input field. pm clear session: SPA input is already
// auto-focused at mount. Tap once to be safe.
tap(674, 1610, 150);
await sleep(300);

// 4. Type TOTP. (No BACK here — pm clear means no soft kbd to dismiss,
//    and BACK on a focused web input might navigate Chrome out of the SPA.)
adb(["shell", "input", "text", totp]);
await sleep(400);
screencap("03-typed");

// 6. Tap Verify & Connect at canonical coords (672, 1894 native).
tap(672, 1894, 150);
await sleep(4000);
screencap("04-after-verify");

// Now shell is ready. Type a unique marker.
// The marker: ridge-pty-trace-MARK-<epoch> via `echo`.
const marker = `RIDGE_AVD_PTY_${Math.floor(Date.now()/1000)}`;
writeFileSync(join(ART, "marker.json"), JSON.stringify({ marker, ts: Math.floor(Date.now()/1000) }, null, 2));

// Show SPA kbd via kbd-toggle (top right of toolbar at native 1200,110).
tap(1200, 110, 200);
await sleep(1500);
screencap("06-kbd-up");

// Use SPA virtual kbd — for simplicity, use `adb shell input text` which feeds
// the SPA shell input via the SPA's text-mode keypress handler.
// Actually input text goes to focused native field. SPA shell uses virtual
// kbd. Try: tap SPA kbd letter area. The SPA has a kbd with qwerty.
// Simpler: type via `adb shell input text` after re-opening soft kbd by tapping
// input area in shell.
tap(672, 1610, 120); // tap shell area to focus
await sleep(300);
adb(["shell", "input", "text", `echo%20${marker}`]);
await sleep(300);
screencap("07-echo-typed");
// Enter
adb(["shell", "input", "keyevent", "66"]);
await sleep(1500);
screencap("08-echo-sent");

// Wait then dump host log.
await sleep(2000);
const buf = readFileSync(logPath, "utf8");
writeFileSync(join(ART, "host-trace-final.log"), buf);

const traceLines = buf.split("\n").filter(l => l.includes("[ridge-trace]"));
console.log(`[pty] trace lines captured: ${traceLines.length}`);
writeFileSync(join(ART, "trace-lines.txt"), traceLines.join("\n"));

// Also dump reverse path: type another marker
const marker2 = `RIDGE_AVD_REV_${Math.floor(Date.now()/1000)}`;
writeFileSync(join(ART, "marker2.json"), JSON.stringify({ marker2, ts: Math.floor(Date.now()/1000) }, null, 2));

adb(["shell", "input", "text", `echo%20${marker2}`]);
await sleep(300);
screencap("09-rev-typed");
adb(["shell", "input", "keyevent", "66"]);
await sleep(2000);
screencap("10-rev-sent");

const buf2 = readFileSync(logPath, "utf8");
writeFileSync(join(ART, "host-trace-final2.log"), buf2);
const traceLines2 = buf2.split("\n").filter(l => l.includes("[ridge-trace]"));
writeFileSync(join(ART, "trace-lines-final.txt"), traceLines2.join("\n"));
console.log(`[pty] total trace lines: ${traceLines2.length}`);
console.log(`[pty] marker1=${marker} marker2=${marker2} hostPid=${proc.pid}`);
