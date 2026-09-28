// scripts/avd-marker-adb.mjs
//
// §A.8 AVD visible-marker proof via pure ADB input + screencap (no CDP — the
// emulator's Chrome DevTools HTTP endpoint returns 0 bytes on this build).
// Reuses the ALREADY-RUNNING host on 9620 (current remote-dist) + adb reverse.
//
// Types a real `echo <marker>` into the terminal (space typed via KEYCODE_SPACE
// so the shell sees a genuine command), then screencaps the AVD screen. The
// marker must appear on the canvas as shell output.
//
// Read-only on SPA source; no transport/wire/auth-protocol changes.
// Does not push/tag/release/deploy. Prints the RUN_DIR so the caller can Read
// the screencaps.

import { spawnSync } from "node:child_process";
import { writeFileSync, mkdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { createHmac } from "node:crypto";

const ROOT = resolve(".");
const ADB = `${process.env.LOCALAPPDATA}/Android/Sdk/platform-tools/adb.exe`;
const HOST_PORT = Number(process.env.AVD_HOST_PORT ?? "9620");
const OUT_DIR = join(ROOT, "artifacts/release/avd-visual", `adb-${Date.now()}`);
mkdirSync(OUT_DIR, { recursive: true });
const log = (...a) => console.log(a.join(" "));

const adb = (args) => spawnSync(ADB, args, { encoding: "utf8", timeout: 30000 });
const tap = (x, y, dwell = 150) => adb(["shell", `input swipe ${x} ${y} ${x} ${y} ${dwell}`]);
const key = (code) => adb(["shell", "input", "keyevent", String(code)]);
const text = (t) => {
  // Quote the whole token so adb's shell doesn't split on spaces.
  adb(["shell", "input", "text", `'${String(t).replace(/'/g, "'\\''")}'`]);
};
const screencap = (name) => {
  const remote = `/data/local/tmp/avd-adb-${name}.png`;
  const local = join(OUT_DIR, `${name}.png`);
  adb(["shell", `screencap -p ${remote}`]);
  adb(["pull", remote, local]);
  return local;
};
const dump = (name) => {
  adb(["shell", "uiautomator", "dump", "/sdcard/u.xml"]);
  const local = join(OUT_DIR, `${name}.xml`);
  adb(["pull", "/sdcard/u.xml", local]);
  try { return readFileSync(local, "utf8"); } catch { return ""; }
};

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

// Pre-flight.
const dev = adb(["devices"]);
if (!String(dev.stdout).includes("emulator-5554")) { log("FATAL emulator-5554 not running"); process.exit(10); }
const probe = spawnSync("curl", ["-sk", "-o", "/dev/null", "-w", "%{http_code}", `https://localhost:${HOST_PORT}/`], { encoding: "utf8" });
if (String(probe.stdout).trim() !== "200") { log("FATAL host not alive on", String(HOST_PORT)); process.exit(10); }
adb(["reverse", `tcp:${HOST_PORT}`, `tcp:${HOST_PORT}`]);
await sleep(500);
log(`run → ${OUT_DIR}`);
log(`host ${HOST_PORT} alive; reverse set`);

// Launch Chrome fresh at the SPA. No ?debug (its overlay covers the terminal's
// top-left where prompt/echo output renders) and no ?reset (we reuse the active
// pane that already has shell content).
const APP_URL = `https://localhost:${HOST_PORT}/`;
adb(["shell", "am", "force-stop", "com.android.chrome"]);
await sleep(1500);
adb(["shell", "am", "start", "-a", "android.intent.action.VIEW", "-n", "com.android.chrome/com.google.android.apps.chrome.Main", "-d", APP_URL]);
await sleep(7000);

// Dismiss onboarding dialogs (up to 6 guard passes).
for (let g = 0; g < 6; g++) {
  const xml = dump(`d-${g}`);
  if (/Make Chrome your own|Use without an account/.test(xml)) { log(`guard ${g}: no-account`); tap(672, 2638); await sleep(2500); continue; }
  if (/Chrome notifications|No thanks/.test(xml)) { log(`guard ${g}: no-thanks`); tap(757, 2133); await sleep(2500); continue; }
  log(`guard ${g}: ready`);
  break;
}
adb(["shell", "am", "start", "-a", "android.intent.action.VIEW", "-n", "com.android.chrome/com.google.android.apps.chrome.Main", "-d", APP_URL]);
await sleep(7000);
screencap("01-loaded");

// ── TOTP gate (only when actually gated) ─────────────────────────────────
// The always-on `prop paneId=…` overlay is a text node → uiautomator sees it.
// If it's already present the session auto-authed (Chrome profile kept the
// pairing) and any `input text` would land in the TERMINAL, not a gate field
// (this leaked a TOTP code into the shell once). Skip the gate fill then.
let xml = dump("ui-pre-input");
// Impoverished a11y dumps (Chrome UI only, <35 nodes) happen while the web
// content is still loading — retry until web nodes appear so we don't mistake
// "no overlay text" for "gate is up" and type a TOTP into the shell/launcher.
for (let i = 0; i < 5 && (xml.match(/<node/g) || []).length < 35 && !/prop paneId=/.test(xml); i++) {
  await sleep(2000);
  xml = dump(`ui-pre-input-retry-${i}`);
}
const terminalUp = /prop paneId=|attached=/.test(xml);
if (!terminalUp) {
  // Pick the CONTENT EditText (cy > 400) — the first EditText is Chrome's URL
  // omnibox at the top of the screen.
  const eds = [...xml.matchAll(/class="android\.widget\.EditText"[^>]*bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"/g)]
    .map((m) => ({ cx: (Number(m[1]) + Number(m[3])) >> 1, cy: (Number(m[2]) + Number(m[4])) >> 1 }))
    .filter((e) => e.cy > 400);
  let inputCx = 672, inputCy = 1702;
  if (eds.length) { inputCx = eds[eds.length - 1].cx; inputCy = eds[eds.length - 1].cy; }
  log(`gate: input center=(${inputCx},${inputCy})`);
  const totp = computeTotp();
  log(`totp computed`);
  tap(inputCx, inputCy);
  await sleep(500);
  key(123); // MOVE_END
  for (let i = 0; i < 30; i++) key(67); // DEL clear
  await sleep(300);
  text(totp);
  await sleep(700);
  screencap("02-typed");

  // Verify by uiautomator bounds: find the actual Button whose text looks like
  // Verify/Connect (hardcoded coords drift with IME open/closed and with the
  // error banner). Fall back through the observed centers. Never BACK — it can
  // close the Chrome tab. ESC is used to dismiss the IME instead.
  const fallbacks = [[672, 2086], [672, 1894], [672, 1515]];
  let verified = false;
  const findVerifyBtn = (g) => [...g.matchAll(/<node[^>]*>/g)]
    .map((m) => {
      const tag = m[0];
      return {
        cls: (tag.match(/class="([^"]*)"/) || [])[1] || "",
        text: ((tag.match(/text="([^"]*)"/) || [])[1] || "").replace(/&amp;/g, "&"),
        bounds: (tag.match(/bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"/) || []).slice(1),
      };
    })
    .find((n) => /Button|TextView/.test(n.cls) && /verify\s*&?\s*connect|验证|连接|继续/i.test(n.text) && n.bounds.length === 4);
  for (let attempt = 0; attempt < 5; attempt++) {
    let gxml = dump(`ui-verify-${attempt}`);
    if (/prop paneId=/.test(gxml)) { verified = true; break; }
    // Chrome's autofill popup ("Show saved passwords…") covers the Verify
    // button after typing a code — dismiss it by tapping a neutral header area
    // before hunting for the button again.
    if (/Show saved passwords|Show saved payment|autofill/i.test(gxml)) {
      log("autofill popup detected — dismissing");
      tap(672, 600);
      await sleep(800);
      gxml = dump(`ui-verify-${attempt}-undropped`);
    }
    const bm = findVerifyBtn(gxml);
    if (bm) {
      const cx = (Number(bm.bounds[0]) + Number(bm.bounds[2])) >> 1, cy = (Number(bm.bounds[1]) + Number(bm.bounds[3])) >> 1;
      log(`verify button ("${bm.text}") at (${cx},${cy})`);
      tap(cx, cy);
    } else {
      const [fx, fy] = fallbacks[Math.min(attempt, fallbacks.length - 1)];
      log(`verify fallback tap (${fx},${fy})`);
      tap(fx, fy);
    }
    await sleep(2500);
    if (/prop paneId=/.test(dump(`ui-verify-post-${attempt}`))) { verified = true; break; }
  }
  log(`gate verify result: ${verified}`);
} else {
  log("gate skipped: terminal already mounted (session auto-auth)");
  screencap("02-typed");
}
// Safety: never type into the launcher. If the terminal isn't up, relaunch
// Chrome once; abort hard if it still isn't. Impoverished a11y dumps (<35
// nodes) are re-tried — they can't prove "not mounted".
const dumpSeesTerminal = async (name) => {
  for (let i = 0; i < 5; i++) {
    const x = dump(i ? `${name}-retry-${i}` : name);
    if (/prop paneId=|attached=/.test(x)) return true;
    if ((x.match(/<node/g) || []).length >= 35) return false; // rich dump, genuinely gated/other
    await sleep(2000);
  }
  return false;
};
if (!(await dumpSeesTerminal("ui-pre-type"))) {
  log("terminal not mounted after gate — relaunching Chrome");
  adb(["shell", "am", "start", "-a", "android.intent.action.VIEW", "-n", "com.android.chrome/com.google.android.apps.chrome.Main", "-d", APP_URL]);
  await sleep(7000);
  if (!(await dumpSeesTerminal("ui-pre-type-2"))) {
    log("FATAL terminal still not mounted — aborting (would type into launcher)");
    screencap("07-final");
    process.exit(11);
  }
}
await sleep(6000);
screencap("03-after-verify");
await sleep(6000);
screencap("04-shell-ready");

// ── echo marker into the terminal ────────────────────────────────────────
const MARKER = `RIDGE-AVD-${Date.now()}`;
log(`MARKER=${MARKER}`);
// Focus the canvas (center-ish of the screen below the title bar).
tap(672, 1400);
await sleep(800);
// Wipe any leftover prompt-line garbage (harmless on an empty prompt).
for (let i = 0; i < 20; i++) key(67);
await sleep(200);
// Type: echo <space> MARKER
// input text uses %s for space (bypasses IME). Then send Enter via shell
// variable trick: printf builds "\nX", ${TXT%X} strips X leaving a real
// newline — input text injects KEYCODE_ENTER through InputManager, bypassing
// IME key event interception.
text(`echo%s${MARKER}`);
await sleep(400);
screencap("05-echo-typed");
// Send Enter via real newline byte embedded in input text argument.
adb(["shell", "sh", "-c", `TXT=$(printf '\\nX'); input text "\${TXT%X}"`]);
await sleep(4000);
screencap("06-after-enter");

// Capture the clean shell output. Re-check Chrome is foreground before the
// money shot.
{
  const focus = adb(["shell", "dumpsys", "window", "displays"]).stdout || "";
  if (!/mCurrentFocus=.*com\.android\.chrome/.test(focus)) {
    log("Chrome lost foreground before final shot — relaunching");
    adb(["shell", "am", "start", "-a", "android.intent.action.VIEW", "-n", "com.android.chrome/com.google.android.apps.chrome.Main", "-d", APP_URL]);
    await sleep(7000);
  }
}
const finalShot = screencap("07-final");
log(`final screencap ${finalShot}`);

// The terminal is canvas-rendered, so uiautomator can't see the text. Provide
// the marker + path for the caller to Read the PNG and confirm visibility.
writeFileSync(join(OUT_DIR, "marker.txt"), MARKER);
log(`done. Read ${join(OUT_DIR, "07-final.png")} to confirm the marker is visible.`);
process.exit(0);
