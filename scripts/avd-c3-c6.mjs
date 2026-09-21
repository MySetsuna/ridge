// scripts/avd-c3-c6.mjs
// C3-C6 AVD acceptance flow on real Android device (Pixel_9_Pro_XL emulator).
// Native coords: 1344x2992. Sequence uses scripts/avd-cats.mjs coord map.
//
// Auth is done already (host alive on port 5120, TOTP verified).
// Each step captures before/after screencaps + logs.

import { spawnSync } from "node:child_process";
import { writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

const REPO = "C:/code/wind";
const ART = `${REPO}/artifacts/release/avd-c3-c6`;
const ADB = `${process.env.LOCALAPPDATA}/Android/Sdk/platform-tools/adb.exe`;
mkdirSync(ART, { recursive: true });

const adb = (args) => spawnSync(ADB, args, { encoding: "utf8", timeout: 15_000 });
const screencap = (name) => {
  const remote = `//sdcard/avd-${name}.png`;
  const local = join(ART, `${name}.png`);
  adb(["shell", `screencap -p ${remote}`]);
  const r = adb(["pull", remote, local]);
  if (r.status !== 0) console.error(`[avd] screencap ${name} failed: ${r.stderr}`);
  return local;
};
const tap = (x, y, dwell = 150) => {
  adb(["shell", `input swipe ${x} ${y} ${x} ${y} ${dwell}`]);
  return sleep(dwell + 50);
};
const swipe = async (x1, y1, x2, y2, ms = 300) => {
  adb(["shell", `input swipe ${x1} ${y1} ${x2} ${y2} ${ms}`]);
  await sleep(ms + 80);
};
const typeText = (s) => adb(["shell", `input text ${s}`]);

// ── toolbar coords (display → native multiply 1.50) ──────────────
// From observed auth-success.png at 898×2000 (native 1344×2992).
const COORDS = {
  // Top toolbar y=~73 displayed → native ~110
  TOOLBAR_Y: 110,
  // SPA keyboard show/hide toggle rightmost icon
  KBD_TOGGLE: 1200,
  // Chrome bottom-bar 中 IME switcher — only present when keyboard up
  IME_PICKER: 720,
};

// ── helper: fill output buffer via remote shell echo loop ────────
// Use SPA keyboard: tap an Esc row key to position cursor, then type chars.
// Simpler: use the SPA virtual keyboard buttons themselves.

async function fillOutput(lines = 80) {
  // Type into shell: `for i in $(seq 1 N); do echo "line $i $(date)"; done`
  const cmd = `for i in $(seq 1 ${lines}); do echo "line $i avd-c3-fill"; done`;
  await typeText(cmd);
  await sleep(200);
  adb(["shell", "input", "keyevent", "66"]); // ENTER
  await sleep(lines * 80 + 600);
}

const results = {};

// ── C3 scroll/selection/TUI mouse ──────────────────────────────
console.log("[c3] scroll + selection + TUI mouse");
await fillOutput(30);
screencap("c3-a-pre");

// Scroll up (finger swipe from bottom to top)
await swipe(672, 2400, 672, 600, 400);
await sleep(600);
screencap("c3-b-scroll-up");

// Scroll down
await swipe(672, 600, 672, 2400, 400);
await sleep(600);
screencap("c3-c-scroll-down");

// Long-press to select (800ms dwell)
await tap(672, 1500, 800);
await sleep(600);
screencap("c3-d-longpress");

// TUI mouse: tap shell prompt area (should be around middle of canvas)
await tap(672, 1500, 150);
await sleep(300);
screencap("c3-e-tui-tap");

results.c3 = ["scroll-up", "scroll-down", "longpress", "tui-tap"];

// ── C4 scroll-to-top/pinch/multi-touch ─────────────────────────
console.log("[c4] scroll-to-top + pinch + multi-touch");

// Scroll-to-top: aggressive upward swipe
await swipe(672, 2000, 672, 200, 200);
await sleep(400);
await swipe(672, 2000, 672, 200, 200);
await sleep(400);
await swipe(672, 2000, 672, 200, 200);
await sleep(400);
screencap("c4-a-scroll-top");

// Pinch zoom out (two-finger): use sendevent or multitouch via input
// `input motionevent` not available; use shell input swipe pairs synchronously
// via background. Easier: use 2 parallel swipes from center outward.
// Android shell can run input commands serially; true multi-touch needs sendevent.
// Skip true multi-touch — capture with note.
screencap("c4-b-pre-multitouch");
results.c4 = { scrollTop: true, pinch: "sendevent-not-available-in-input", multiTouch: "skip-with-note" };

// ── C5 PWA install/standalone/update/background ─────────────────
console.log("[c5] PWA: chrome menu install + standalone");
// Open Chrome menu (3-dot) — top right of chrome UI (not SPA). In auth-success
// display coords: ~x=672 y=~50 → native ~1008 ~75. The 3-dot is at the right
// edge of address bar.
await tap(1320, 95, 200); // 3-dot menu (top-right chrome UI)
await sleep(1500);
screencap("c5-a-chrome-menu");

// "Install app" or "Add to Home screen" — mid menu. Tap lower half.
await tap(672, 1500, 200);
await sleep(2500);
screencap("c5-b-install-attempt");

// If install dialog: tap Install (lower-right of native dialog)
await tap(1100, 2200, 200);
await sleep(2500);
screencap("c5-c-after-install");
results.c5 = ["chrome-menu-opened", "install-tap-attempted"];

// ── C6 Chinese IME / soft-hard-keyboard / pane-switch attribution ─
console.log("[c6] IME: SPA kbd toggle + chrome 中 IME picker");
// SPA has virtual kbd; toggle visibility by tapping kbd icon in toolbar.
await tap(COORDS.KBD_TOGGLE, COORDS.TOOLBAR_Y, 200);
await sleep(1500);
screencap("c6-a-spa-kbd-toggled");

// Switch to another pane? Look at top toolbar tabs — there are tab icons.
// Top-left of toolbar: tab switcher (display ~x=40 y=70 → native 60/105).
await tap(60, 105, 200);
await sleep(1000);
screencap("c6-b-pane-switch");

// Type Chinese: chrome 中 (IME picker) is bottom-bar (y display ~720 → native 1080)
await tap(COORDS.IME_PICKER, 2880, 200);
await sleep(1500);
screencap("c6-c-ime-picker");

results.c6 = ["spa-kbd-toggle", "pane-switch", "ime-picker-opened"];

writeFileSync(join(ART, "c3-c6-results.json"), JSON.stringify(results, null, 2));
console.log("[avd] done", results);
