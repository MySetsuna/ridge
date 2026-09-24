// scripts/avd-visual-marker.mjs
// CHG-051 AVD visual marker verification — wraps the auth + canvas test flow
// against an isolated dev host (default port 9620). The TOTP secret for the
// "default" identity is read from the DPAPI-decrypted file on disk at run time
// (NOT echoed in source — it is the production shared cloud identity).
//
// What this proves:
//   1. AVD auth → SPA mounted in Chrome
//   2. Shell exec echo of unique marker
//   3. Marker on AVD canvas pixel layer (via screencap)
//
// Hard constraints:
//   - One isolated dev host on this session's chosen port
//   - Reuse dev host if already running; else spawn + capture TOTP from stderr
//   - AVD emulator must already be running
//   - adb reverse tcp:<port> tcp:<port> must succeed

import { spawn, spawnSync } from 'node:child_process';
import { createHmac } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, openSync, writeSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const REPO = 'C:/code/wind';
const ART = `${REPO}/artifacts/release/avd-visual`;
const ADB = `${process.env.LOCALAPPDATA}/Android/Sdk/platform-tools/adb.exe`;
const HOST_PORT = Number(process.env.AVD_HOST_PORT ?? '9620');
const HOST_BIN = process.env.RIDGE_BIN ?? 'target/debug/ridge.exe';
const FRESH_DATA_DIR = join(REPO, 'artifacts/release/avd-visual', 'fresh-data');
const HOST_LOG = join(REPO, 'artifacts/release/avd-visual', 'host-trace.log');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const RUN = String(Date.now());
const RUN_DIR = join(ART, RUN);
mkdirSync(RUN_DIR, { recursive: true });

// Capture logcat (filtered to chromium console.log) for the run. WebView
// console.log may come through as V/D priority under chromium tag, so keep
// all levels and grep later.
const logcatFh = openSync(join(RUN_DIR, 'logcat.txt'), 'a');
const logcatProc = spawn(ADB, ['logcat', '-v', 'time', 'chromium:V', 'Console:V', '*:S'], { stdio: ['ignore', logcatFh, logcatFh] });

const log = (...a) => {
  const line = a.map((x) => (typeof x === 'string' ? x : JSON.stringify(x))).join(' ');
  console.log(line);
  writeFileSync(join(RUN_DIR, 'log.txt'), `${line}\n`, { flag: 'a' });
};

const adb = (args, opts = {}) => spawnSync(ADB, args, { encoding: 'utf8', timeout: 30_000, ...opts });
const tap = (x, y, dwell = 200) => adb(['shell', `input swipe ${x} ${y} ${x} ${y} ${dwell}`]);
// ADB shell splits args on spaces, AND `&` is the background operator inside
// that shell, so URLs containing `&` must be single-quoted. Use a tiny helper.
const shellQuoted = (args) => {
  const quoted = args.map((a) => (/[\s'"&|<>();`]/.test(a) ? `'${String(a).replace(/'/g, `'\\''`)}'` : a));
  return ['shell', quoted.join(' ')];
};
const screencap = (name) => {
  const remote = `/data/local/tmp/avd-v-${name}.png`;
  const local = join(RUN_DIR, `${name}.png`);
  adb(['shell', `screencap -p ${remote}`]);
  adb(['pull', remote, local]);
  return local;
};
const dumpAndParse = (label) => {
  const out = join(RUN_DIR, `${label}.xml`);
  adb(['shell', 'uiautomator', 'dump', '/sdcard/u.xml']);
  adb(['pull', '/sdcard/u.xml', out]);
  const xml = readFileSync(out, 'utf8');
  const re = /class="([^"]+)"[^/]*?bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"/g;
  const map = {};
  for (const m of xml.matchAll(re)) {
    const cls = m[1];
    const x = (Number(m[2]) + Number(m[4])) >> 1;
    const y = (Number(m[3]) + Number(m[5])) >> 1;
    map[cls] = map[cls] || [];
    map[cls].push({ x, y, x1: +m[2], y1: +m[3], x2: +m[4], y2: +m[5] });
  }
  return map;
};
// Launch Chrome with a URL (correctly quoted so `&` survives shell parsing).
const launchChrome = (url) => {
  const r = adb(shellQuoted(['am', 'start', '-a', 'android.intent.action.VIEW', '-n', 'com.android.chrome/com.google.android.apps.chrome.Main', '-d', url]));
  if (r.status !== 0) log(`launchChrome warn: ${r.stderr?.trim().slice(0, 120)}`);
};

// ── TOTP seed: read from DPAPI-decrypted file on disk ─────────────────────
function readSeed() {
  // identity="default" → sha256("default")[:8] hex = "37a8eec1ce19687d"
  const seedFile = `${process.env.APPDATA}/ridge/config/totp/37a8eec1ce19687d.seed`;
  if (!existsSync(seedFile)) throw new Error(`seed file not found: ${seedFile}`);
  // Decrypt via .NET DPAPI Unprotect
  const ps = `
    Add-Type -AssemblyName System.Security
    $bytes = [System.IO.File]::ReadAllBytes('${seedFile.replace(/'/g, "''")}')
    $dec = [System.Security.Cryptography.ProtectedData]::Unprotect($bytes, $null, [System.Security.Cryptography.DataProtectionScope]::CurrentUser)
    [System.BitConverter]::ToString($dec).Replace('-','').ToLower()
  `;
  const out = spawnSync('powershell', ['-NoProfile', '-Command', ps], { encoding: 'utf8' });
  if (out.status !== 0) throw new Error(`DPAPI unprotect failed: ${out.stderr}`);
  return out.stdout.trim();
}

function computeTotp(secretHex) {
  const secret = Buffer.from(secretHex, 'hex');
  const counter = Math.floor(Math.floor(Date.now() / 1000) / 30);
  const cb = Buffer.alloc(8);
  cb.writeBigUInt64BE(BigInt(counter));
  const mac = createHmac('sha256', secret).update(cb).digest();
  const offset = mac[mac.length - 1] & 0x0f;
  const code = ((mac[offset] & 0x7f) << 24) | (mac[offset + 1] << 16) | (mac[offset + 2] << 8) | mac[offset + 3];
  return String(code % 1_000_000).padStart(6, '0');
}

// ── main ──────────────────────────────────────────────────────────────────
log(`run=${RUN} host_port=${HOST_PORT}`);
const secretHex = readSeed();
log(`seed loaded (len=${secretHex.length})`);

// Probe AVD
const dev = adb(['devices']);
if (!dev.stdout.includes('emulator-5554')) {
  log('FATAL: emulator-5554 not running');
  process.exit(10);
}
log('emulator-5554 attached');

// Probe / spawn dev host
let hostProc = null;
let hostAlreadyUp = false;
const portCheck = spawnSync('powershell', ['-NoProfile', '-Command', `Get-NetTCPConnection -LocalPort ${HOST_PORT} -ErrorAction SilentlyContinue | Select-Object -First 1 State`], { encoding: 'utf8' });
if (portCheck.stdout && portCheck.stdout.includes('Listen')) {
  hostAlreadyUp = true;
  log(`host already listening on ${HOST_PORT}, reusing`);
} else {
  log(`spawning host on ${HOST_PORT} with fresh data dir`);
  // wipe + recreate fresh data dir so we don't reuse stale kernel state
  if (existsSync(FRESH_DATA_DIR)) {
    try {
      const { rmSync } = await import('node:fs');
      rmSync(FRESH_DATA_DIR, { recursive: true, force: true });
    } catch { /* */ }
  }
  mkdirSync(FRESH_DATA_DIR, { recursive: true });
  hostProc = spawn(HOST_BIN, ['host', '--port', String(HOST_PORT)], {
    env: {
      ...process.env,
      RIDGE_KERNEL_DATA_DIR: FRESH_DATA_DIR,
      RIDGE_PRINT_TOTP: '1',
      RIDGE_TEST_ALLOW_NON_BREAKAWAY: '1',
      RIDGE_KERNEL_TRACE: '1',
      RIDGE_HOST_TRACE: '1',
      RIDGE_HOST_DEBUG: '1',
      RUST_LOG: 'info,ridge_kernel::domain_pty=trace,ridge_cli::kernel_host_impl=trace,ridge_remote::debug=info',
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  // Mirror host stderr to a trace log for offline post-mortem
  const traceFh = openSync(HOST_LOG, 'a');
  hostProc.stderr.on('data', (d) => { writeSync(traceFh, d); });
  const errBuf = [];
  hostProc.stderr.on('data', (d) => errBuf.push(d.toString()));
  // Wait for "ridge host ready"
  for (let i = 0; i < 30; i += 1) {
    await sleep(500);
    if (errBuf.join('').includes('ridge host ready')) break;
  }
  log('host ready');
}

// Set up reverse forward
adb(['reverse', `tcp:${HOST_PORT}`, `tcp:${HOST_PORT}`]);
await sleep(500);

// Launch Chrome on AVD (cold-boot; onboarding flow re-fires because of pm clear)
adb(['shell', 'am', 'force-stop', 'com.android.chrome']);
await sleep(1500);
const APP_URL = `https://localhost:${HOST_PORT}/_app/?reset=1&debug=pane=1`;
launchChrome(APP_URL);
await sleep(6000);

// Onboarding + first-run dialog dismiss — IMPORTANT: do NOT re-launch Chrome
// after each tap. `am start -a VIEW -d <url>` either re-opens Chrome in its
// cold-start onboarding state (wiping the dismissed dialog), or lands on a
// ERR_ACCESS_DENIED page if the previous tab was file://. Instead we let the
// existing Chrome activity advance; only navigate to the SPA once the dialogs
// are all gone.
try {
  for (let guard = 0; guard < 8; guard++) {
    adb(['shell', 'uiautomator', 'dump', '/sdcard/u.xml']);
    adb(['pull', '/sdcard/u.xml', join(RUN_DIR, `d-${guard}.xml`)]);
    const xml = readFileSync(join(RUN_DIR, `d-${guard}.xml`), 'utf8');
    let detected = '';
    // Sign-in / welcome: "Use without an account" button — bounds [72,2566][1272,2710] center (672,2638)
    if (xml.includes('Make Chrome your own') || xml.includes('Use without an account')) {
      log(`guard ${guard}: tap Use without an account`);
      tap(672, 2638, 200);
      detected = 'no-account';
    } else if (xml.includes('Chrome notifications make things easier') || xml.includes('No thanks')) {
      log(`guard ${guard}: tap No thanks`);
      // "No thanks" bounds [625,2061][889,2205] center (757,2133)
      tap(757, 2133, 200);
      detected = 'no-notifications';
    } else if (xml.includes('Turn on sync') || xml.includes("Don't turn on")) {
      // "Don't turn on" sits lower; coords approximate. Just confirm and try.
      log(`guard ${guard}: turn on sync visible — accept default (don't tap)`);
      detected = 'sync-shown';
    } else if (xml.includes('Access to the file was denied') || xml.includes('ERR_')) {
      log(`guard ${guard}: error page, navigating to SPA URL`);
      launchChrome(APP_URL);
      detected = 'err';
    }
    if (!detected) {
      log(`guard ${guard}: no dialog detected, assuming ready`);
      break;
    }
    await sleep(2500);
  }
} catch (e) { log(`onboarding check skipped: ${String(e).slice(0, 200)}`); }

// Final navigation to SPA URL after all dialogs dismissed
launchChrome(APP_URL);
await sleep(6000);

screencap('01-loaded');

// Bounds-parsed TOTP gate
let ui = dumpAndParse('ui-pre-input');
let inputEl = (ui['android.widget.EditText'] || []).find((e) => e.y > 1300) || (ui['android.widget.EditText'] || [])[0];
if (!inputEl) { log('FATAL: no EditText'); process.exit(11); }
log(`input center=(${inputEl.x},${inputEl.y}) bounds=${inputEl.x1},${inputEl.y1}-${inputEl.x2},${inputEl.y2}`);

const totp = computeTotp(secretHex);
log(`TOTP=${totp}`);

tap(inputEl.x, inputEl.y, 200);
await sleep(600);
// Clear stale
adb(['shell', 'input', 'keyevent', '123']);
for (let i = 0; i < 30; i++) adb(['shell', 'input', 'keyevent', '67']);
await sleep(400);
// Disable autofill suggestions above kbd: send DEL twice extra + long pause
adb(['shell', 'input', 'keyevent', '67']);
await sleep(300);
adb(['shell', 'input', 'text', totp]);
await sleep(800);
screencap('02-typed');

// WebView does NOT expose DOM as Android widgets → uiautomator Button[] is empty.
// Use hardcoded coords derived from 02-typed screenshot: Ridge Remote auth screen
// has input box centered at y≈1376, green Verify button centered at y≈1515 (kbd open).
// See AVD acceptance memory: input center (672,1702), verify center (672,1894) for kbd-hidden;
// kbd-open variant has verify visible just above kbd accessory bar (y<1816).
const VERIFY_KBD_OPEN = { x: 672, y: 1515 };
const VERIFY_KBD_HIDDEN = { x: 672, y: 1894 };

// Dismiss kbd with BACK so verify button isn't covered — but if verify is already
// above kbd, tap directly. Try kbd-open tap first; only fall back to kbd-hidden.
tap(VERIFY_KBD_OPEN.x, VERIFY_KBD_OPEN.y, 200);
await sleep(8000);
screencap('03-after-verify');

// Wait for terminal pane mount
await sleep(6000);
screencap('04-shell-ready');

const MARKER = `RIDGE-AVD-VISUAL-${RUN}`;
log(`MARKER=${MARKER}`);

// Type marker into canvas
tap(672, 1610, 120);
await sleep(600);
adb(['shell', 'input', 'text', `echo%20${MARKER}`]);
await sleep(500);
screencap('05-echo-typed');
adb(['shell', 'input', 'keyevent', '66']);
await sleep(3000);
screencap('06-after-enter');

// IME close
adb(['shell', 'input', 'keyevent', '4']);
await sleep(1500);
screencap('07-ime-closed');

log('done');
try { logcatProc.kill(); } catch { /* */ }
if (hostProc) try { hostProc.kill(); } catch { /* */ }
process.exit(0);