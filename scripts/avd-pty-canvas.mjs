// scripts/avd-pty-canvas.mjs
// Phase 4: AVD PTY marker on screen via dev host 9529 (already serving).
// TOTP computed locally from DPAPI-decrypted seed (no need to spawn new host).
// One retry allowed on identical error mode; otherwise halt.

import { spawn, spawnSync } from 'node:child_process';
import { execFileSync } from 'node:child_process';
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  openSync,
  readFileSync,
  writeFileSync,
} from 'node:fs';
import { join } from 'node:path';
import { createHash, createHmac } from 'node:crypto';

const REPO = 'C:/code/wind';
const ART = `${REPO}/artifacts/release/avd-emulator`;
const ADB = `${process.env.LOCALAPPDATA}/Android/Sdk/platform-tools/adb.exe`;
const DEV_HOST_PORT = 9529; // live dev host (PID 33100), already serving
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const RUN = String(Date.now());
const RUN_DIR = String.raw`C:\code\wind\artifacts\release\avd-emulator\${RUN}`.replace(/\$\{RUN\}/g, RUN);
mkdirSync(RUN_DIR, { recursive: true });
const log = (...a) => {
  const line = a.map((x) => (typeof x === 'string' ? x : JSON.stringify(x))).join(' ');
  console.log(line);
  writeFileSync(join(RUN_DIR, 'log.txt'), `${line}\n`, { flag: 'a' });
};

const adb = (args, opts = {}) =>
  spawnSync(ADB, args, { encoding: 'utf8', timeout: 30_000, ...opts });
const tap = (x, y, dwell = 200) => adb(['shell', `input swipe ${x} ${y} ${x} ${y} ${dwell}`]);
const screencap = (name) => {
  const remote = `//sdcard/avd-pty-${name}.png`;
  const local = join(RUN_DIR, `${name}.png`);
  adb(['shell', `screencap -p ${remote}`]);
  adb(['pull', remote, local]);
  return local;
};

const sha256 = (p) => {
  const h = createHash('sha256');
  h.update(readFileSync(p));
  return h.digest('hex');
};

// Pre-flight: confirm dev host is alive
const probe = spawnSync(
  'curl',
  ['-sk', '-o', '/dev/null', '-w', '%{http_code}', `https://localhost:${DEV_HOST_PORT}/_app/`],
  { encoding: 'utf8' },
);
const httpCode = String(probe.stdout).trim();
log(`dev host probe https://localhost:${DEV_HOST_PORT}/_app/ = ${httpCode}`);
if (httpCode !== '200') {
  log('FATAL dev host not alive on 9529; aborting');
  process.exit(10);
}

// Phase 2 (no-op): reuse dev host TLS / TOTP secret. No rotation needed since
// we're hitting the live host with the same CA + same seed.
const TRUSTED_CA_DER = `${REPO}/artifacts/release/avd-acceptance/ridge-ca.der`;
let pre = null, post = null;
if (existsSync(TRUSTED_CA_DER)) {
  pre = sha256(TRUSTED_CA_DER);
  post = pre; // no rotation; same host
  log(`TLS pre=${pre.slice(0, 16)} post=${post.slice(0, 16)} match=true rotation=none (reuse dev host)`);
}

const MARKER = `RIDGE-AVD-MARKER-${RUN}`;

// §security-totp-runtime (2026-09-25): compute TOTP from the DPAPI-decrypted
// seed for identity "default" at run time. The seed value is NEVER hardcoded —
// see scripts/security-totp-rotate.mjs for the rotation tool and
// scripts/security-totp-seed-lint.mjs for the tracked-source regression gate.
//   identity "default" -> hex(sha256("default")[:16]) = 37a8eec1ce19687d
//   seed file = %APPDATA%\ridge\config\totp\37a8eec1ce19687d.seed (DPAPI CurrentUser)
function readSeedHex() {
  const seedFile = `${process.env.APPDATA}/ridge/config/totp/37a8eec1ce19687d.seed`;
  if (!existsSync(seedFile)) throw new Error(`seed file not found: ${seedFile}`);
  const ps = `
    Add-Type -AssemblyName System.Security
    $bytes = [System.IO.File]::ReadAllBytes('${seedFile.replace(/'/g, "''")}')
    $dec = [System.Security.Cryptography.ProtectedData]::Unprotect($bytes, $null, [System.Security.Cryptography.DataProtectionScope]::CurrentUser)
    [System.BitConverter]::ToString($dec).Replace('-','').ToLower()
  `;
  const out = spawnSync('powershell', ['-NoProfile', '-NonInteractive', '-Command', ps], { encoding: 'utf8', timeout: 30_000, windowsHide: true });
  if (out.status !== 0) throw new Error(`DPAPI unprotect failed: ${String(out.stderr || '').slice(0, 120)}`);
  return out.stdout.trim();
}
function computeTotp() {
  const secretHex = readSeedHex();
  const secret = Buffer.from(secretHex, 'hex');
  const nowSec = Math.floor(Date.now() / 1000);
  const counter = Math.floor(nowSec / 30);
  const cb = Buffer.alloc(8);
  cb.writeBigUInt64BE(BigInt(counter));
  const mac = createHmac('sha256', secret).update(cb).digest();
  const offset = mac[mac.length - 1] & 0x0f;
  const code =
    ((mac[offset] & 0x7f) << 24) |
    (mac[offset + 1] << 16) |
    (mac[offset + 2] << 8) |
    mac[offset + 3];
  return String(code % 1_000_000).padStart(6, '0');
}

const PORT = DEV_HOST_PORT;
log(`run=${RUN} marker=${MARKER} port=${PORT} (dev host reused)`);

// Reverse tunnel + Chrome (force-stop then launch URL)
adb(['reverse', `tcp:${PORT}`, `tcp:${PORT}`]);
await sleep(500);
adb(['shell', 'am', 'force-stop', 'com.android.chrome']);
await sleep(1500);
adb([
  'shell',
  'am',
  'start',
  '-a',
  'android.intent.action.VIEW',
  '-n',
  'com.android.chrome/com.google.android.apps.chrome.Main',
  '-d',
  `https://localhost:${PORT}/`,
]);
await sleep(6000);
// Detect Chrome onboarding popup via dump and dismiss if present
const dumpCheck = spawnSync(ADB, ['shell', 'uiautomator', 'dump', '/sdcard/d.xml'], { encoding: 'utf8' });
adb(['pull', '/sdcard/d.xml', join(RUN_DIR, 'd-init.xml')]);
let needsOnboard = false;
try {
  const xml = readFileSync(join(RUN_DIR, 'd-init.xml'), 'utf8');
  needsOnboard = xml.includes('Make Chrome your own') || xml.includes('Use without an account');
} catch {}
if (needsOnboard) {
  log('Chrome onboarding detected, tapping Use without an account (y=2670)');
  tap(672, 2670, 200);
  await sleep(5000);
  // Re-trigger URL after onboarding is dismissed
  adb([
    'shell',
    'am',
    'start',
    '-a',
    'android.intent.action.VIEW',
    '-n',
    'com.android.chrome/com.google.android.apps.chrome.Main',
    '-d',
    `https://localhost:${PORT}/`,
  ]);
  await sleep(6000);
}
screencap('01-loaded');

// Phase 4b: bounds-parsed TOTP gate. Dump UI before + after typing to extract
// real input + verify button centers (kbd push-up shifts the button).
screencap('01-loaded');

// Dump UI, extract EditText (input) bounds
function dumpAndParse(label) {
  const out = join(RUN_DIR, `${label}.xml`);
  adb(['shell', 'uiautomator', 'dump', '/sdcard/u.xml']);
  adb(['pull', '/sdcard/u.xml', out]);
  const xml = readFileSync(out, 'utf8');
  // bounds format: [x1,y1][x2,y2] → center ((x1+x2)/2, (y1+y2)/2)
  const re = /class="([^"]+)"[^/]*?bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"/g;
  const out2 = {};
  for (const m of xml.matchAll(re)) {
    const cls = m[1];
    const x = (Number(m[2]) + Number(m[4])) >> 1;
    const y = (Number(m[3]) + Number(m[5])) >> 1;
    out2[cls] = out2[cls] || [];
    out2[cls].push({ x, y, x1: +m[2], y1: +m[3], x2: +m[4], y2: +m[5] });
  }
  return out2;
}

let ui = dumpAndParse('ui-pre-input');
let inputEl = (ui['android.widget.EditText'] || []).find((e) => e.y > 1300) || (ui['android.widget.EditText'] || [])[0];
if (!inputEl) { log('FATAL: no EditText found in UI dump; abort'); process.exit(11); }
log(`input bounds=(${inputEl.x1},${inputEl.y1})-(${inputEl.x2},${inputEl.y2}) center=(${inputEl.x},${inputEl.y})`);

let totp = computeTotp();
const totpExpiry = Math.floor(Date.now() / 1000) + 30 - (Math.floor(Date.now() / 1000) % 30);
log(`TOTP=${totp} expires_at_unix=${totpExpiry}`);

tap(inputEl.x, inputEl.y, 200);
await sleep(600);
// Clear stale text from prior runs (KEYCODE_MOVE_END=123 then 30x DEL=67)
adb(['shell', 'input', 'keyevent', '123']);
for (let i = 0; i < 30; i++) adb(['shell', 'input', 'keyevent', '67']);
await sleep(400);
adb(['shell', 'input', 'text', totp]);
await sleep(800);
screencap('03-typed');

// Re-dump (kbd pushed up — verify button may have shifted)
ui = dumpAndParse('ui-post-type');
let verifyEl = (ui['android.widget.Button'] || []).find((b) => b.x > 100 && b.y > 1300 && b.y < 2500) || (ui['android.widget.Button'] || [])[0];
if (!verifyEl) { log('FATAL: no Button found after typing; abort'); process.exit(12); }
log(`verify bounds=(${verifyEl.x1},${verifyEl.y1})-(${verifyEl.x2},${verifyEl.y2}) center=(${verifyEl.x},${verifyEl.y})`);
// Recompute TOTP right before submit (avoid window drift across tap+type delay).
totp = computeTotp();
log(`TOTP=${totp} (pre-submit recompute)`);
// Dismiss kbd with BACK so verify button isn't covered
adb(['shell', 'input', 'keyevent', '4']);
await sleep(400);
ui = dumpAndParse('ui-kbd-hidden');
verifyEl = (ui['android.widget.Button'] || []).find((b) => b.x > 100 && b.y > 1300 && b.y < 2500 && (b.y2 - b.y1) > 50);
if (!verifyEl) {
  // Re-dismiss kbd (BACK) and try once more
  adb(['shell', 'input', 'keyevent', '4']);
  await sleep(600);
  ui = dumpAndParse('ui-kbd-hidden-2');
  verifyEl = (ui['android.widget.Button'] || []).find((b) => b.x > 100 && b.y > 1300 && b.y < 2500 && (b.y2 - b.y1) > 50);
}
if (!verifyEl) { log('FATAL: no real Button (y2-y1>50) after kbd-hide; abort'); process.exit(12); }
log(`verify bounds=(${verifyEl.x1},${verifyEl.y1})-(${verifyEl.x2},${verifyEl.y2}) center=(${verifyEl.x},${verifyEl.y})`);
tap(verifyEl.x, verifyEl.y, 200);
await sleep(2000);
const remaining = totpExpiry - Math.floor(Date.now() / 1000);
if (remaining < 5) {
  totp = computeTotp();
  log(`TOTP=${totp} (recomputed for next window)`);
}
await sleep(8000);
screencap('04-after-verify');

// Wait for terminal pane to mount (canvas initializes + WS attaches)
await sleep(6000);
screencap('05-shell-ready');

// Echo marker into terminal pane (canvas is roughly center of screen)
//   - Canvas focus area: (672, 1610)
tap(672, 1610, 120);
await sleep(600);
adb(['shell', 'input', 'text', `echo%20${MARKER}`]);
await sleep(500);
screencap('06-echo-typed');
adb(['shell', 'input', 'keyevent', '66']);
await sleep(4000);
screencap('07-echo-sent');

await sleep(2000);
screencap('08-final');

// Mark found via uiautomator dump
adb(['shell', 'uiautomator', 'dump', '/sdcard/ui.xml']);
adb(['pull', '/sdcard/ui.xml', join(RUN_DIR, 'ui.xml')]);
let markerFound = false;
try {
  const xml = readFileSync(join(RUN_DIR, 'ui.xml'), 'utf8');
  markerFound = xml.includes(MARKER);
} catch {}
log(`markerFound=${markerFound} marker=${MARKER}`);

const summary = {
  run: RUN,
  marker: MARKER,
  markerFound,
  retried: false,
  stop_reason: markerFound ? null : 'marker_not_in_ui_xml',
  duration_ms: Date.now() - Number(RUN),
  totp_used: totp,
  tls: {
    pre_sha256: pre,
    post_sha256: post,
    match: pre === post,
    rotation_reason: 'none (reuse dev host)',
  },
};
writeFileSync(join(RUN_DIR, 'summary.json'), JSON.stringify(summary, null, 2));

const diag = {
  schema_version: 1,
  generated_at: new Date().toISOString(),
  run: RUN,
  marker: MARKER,
  surface: 'avd-emulator',
  tls: summary.tls,
  artifacts: {
    fingerprint: join(RUN_DIR, 'pre-post-cert-fingerprint.txt'),
    ui_xml: join(RUN_DIR, 'ui.xml'),
    screencaps: [
      '01-loaded.png', '03-typed.png', '04-after-verify.png',
      '05-shell-ready.png', '06-echo-typed.png', '07-echo-sent.png',
      '08-final.png',
    ],
  },
};
writeFileSync(join(RUN_DIR, 'diagnostics.json'), JSON.stringify(diag, null, 2));

log(`done markerFound=${markerFound}`);
process.exit(markerFound ? 0 : 5);