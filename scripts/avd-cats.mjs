// scripts/avd-cats.mjs
// AVD gesture acceptance — uses hardcoded coords since uiautomator can't
// see WebView buttons. Coords in original device resolution (1344x2992).

import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const REPO = "C:/code/wind";
const ART = `${REPO}/artifacts/release/avd-acceptance`;
const ADB = `${process.env.LOCALAPPDATA}/Android/Sdk/platform-tools/adb.exe`;
mkdirSync(ART, { recursive: true });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function adb(args) {
  return spawnSync(ADB, args, { encoding: "utf8" });
}

function screencap(name) {
  // Use file+pull — exec-out produces truncated PNG on this AVD.
  // Use //sdcard/ to bypass MSYS path mangling in adb pull.
  const remote = `//sdcard/avd-${name}-${Date.now()}.png`;
  const local = join(ART, `${Date.now()}-${name}.png`);
  adb(["shell", `screencap -p ${remote}`]);
  const r = adb(["pull", remote, local]);
  if (r.status === 0) console.log(`[avd] ${name}: ${local}`);
  else console.error(`[avd] ${name} pull failed: ${r.stderr}`);
  return local;
}

async function swipe(x1, y1, x2, y2, ms = 200) {
  adb(["shell", `input swipe ${x1} ${y1} ${x2} ${y2} ${ms}`]);
  await sleep(ms + 50);
}

async function tap(x, y, dwell = 150) {
  await swipe(x, y, x, y, dwell);
}

// Toolbar coords (1344x2992 native, scale 1.50)
const COORDS = {
  TOOLBAR_Y: 410,
  FILE: 127,
  SEARCH: 345,
  SHELL_DOT: 1305,
  SWAP: 1620,
  KEYBOARD: 1830,
  PANE_TOP: 460,
  PANE_BOT: 2850,
};

const cmd = process.argv[2] || "all";

if (cmd === "sidebar" || cmd === "all") {
  console.log("\n[cat2] sidebar");
  screencap("c2-pre");
  await tap(COORDS.FILE, COORDS.TOOLBAR_Y, 200);
  await sleep(1500);
  screencap("c2-post-tap");
}

if (cmd === "scroll" || cmd === "all") {
  console.log("\n[cat3] scroll");
  await swipe(670, 2400, 670, 600, 300);
  await sleep(800);
  screencap("c3-scrolled-up");
  await swipe(670, 600, 670, 2400, 300);
  await sleep(800);
  screencap("c3-scrolled-down");
  // Long-press
  await swipe(670, 1500, 670, 1500, 800);
  await sleep(800);
  screencap("c3-long-press");
}

if (cmd === "pwa" || cmd === "all") {
  console.log("\n[cat5] pwa install via chrome menu");
  // Chrome menu (3 dots) at top-right of chrome UI, not SPA
  // In displayed image: ~ x=1090 y=124, original: x≈1635 y≈186
  await tap(1635, 186, 200);
  await sleep(1500);
  screencap("c5-chrome-menu");
  // Look for "Install app" or similar — usually mid-list
  // Tap roughly at "Install app" position (lower half of menu)
  await tap(670, 1500, 200);
  await sleep(2000);
  screencap("c5-install-attempt");
  // If install dialog appears, tap "Install" button (typically lower-right)
  await tap(1100, 2200, 200);
  await sleep(2000);
  screencap("c5-after-install");
}

if (cmd === "ime" || cmd === "all") {
  console.log("\n[cat6] ime");
  // Tap SPA keyboard icon (rightmost in top toolbar)
  await tap(COORDS.KEYBOARD, COORDS.TOOLBAR_Y, 200);
  await sleep(2000);
  screencap("c6-spa-kbd-tap");
  // Also tap chrome's bottom IME switcher (中) to open IME picker
  await tap(720, 2890, 200);
  await sleep(2000);
  screencap("c6-ime-picker");
}

console.log("\n[avd] cats done");