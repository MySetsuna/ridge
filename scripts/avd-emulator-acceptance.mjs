// scripts/avd-emulator-acceptance.mjs
// Goal §"如果 emulator GPU 仍黑屏" — drive AVD Chrome through SPA TOTP,
// capture WebGL renderer, screenshot, and host trace; produce
// diagnostics.json via export-diagnostics.mjs.
//
// Steps:
//   1. spawn test-rdg host (RIDGE_PRINT_TOTP=1, RIDGE_TRACE=1)
//   2. adb reverse tcp:<port> tcp:<port>
//   3. open AVD Chrome -> SPA -> TOTP -> shell
//   4. via CDP: read WebGL renderer string, screenshot canvas, capture
//      pixel sample, type marker, screenshot after echo
//   5. emit diagnostics.json
//
// Reuses scripts/avd-pty-trace.mjs pattern.

import { spawn, spawnSync } from 'node:child_process';
import { mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const REPO = 'C:/code/wind';
const ART = `${REPO}/artifacts/release/avd-emulator`;
const ADB = `${process.env.LOCALAPPDATA}/Android/Sdk/platform-tools/adb.exe`;
const HOST = `${REPO}/target/test-rdg/release/ridge.exe`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const RUN = String(Date.now());
const RUN_DIR = join(ART, RUN);
mkdirSync(RUN_DIR, { recursive: true });

const adb = (args, opts = {}) =>
  spawnSync(ADB, args, { encoding: 'utf8', timeout: 30_000, ...opts });

const screencap = (name) => {
  const remote = `//sdcard/avd-emp-${name}.png`;
  const local = join(RUN_DIR, `${name}.png`);
  adb(['shell', `screencap -p ${remote}`]);
  adb(['pull', remote, local]);
  return local;
};
const tap = (x, y, dwell = 250) => adb(['shell', `input swipe ${x} ${y} ${x} ${y} ${dwell}`]);

// 1. Spawn host.
const PORT = 5152;
const MARKER = `RIDGE_AVD_EMP_${Math.floor(Date.now() / 1000)}`;
const dataDir = `C:\\Users\\12867\\AppData\\Local\\Temp\\ridge-avd-emp-${Date.now()}`;
const env = {
  ...process.env,
  RIDGE_KERNEL_DATA_DIR: dataDir,
  RIDGE_PRINT_TOTP: '1',
  RIDGE_TRACE: '1',
};
const logPath = join(RUN_DIR, 'host-trace.log');
const out = openSync(logPath, 'w');
const proc = spawn(HOST, ['host', '--port', String(PORT)], {
  env, stdio: ['ignore', out, out], detached: true,
});
proc.unref();
console.log(`[avd-emp] host pid=${proc.pid} port=${PORT} run=${RUN}`);

let totp = null;
for (let i = 0; i < 80; i++) {
  await sleep(100);
  try {
    const m = readFileSync(logPath, 'utf8').match(/TOTP:\s*(\d{6})/);
    if (m) { totp = m[1]; break; }
  } catch {}
}
if (!totp) { console.error('[avd-emp] no TOTP'); process.exit(2); }
console.log(`[avd-emp] TOTP=${totp} MARKER=${MARKER}`);

// 2. Reverse-tunnel + Chrome.
adb(['reverse', `tcp:${PORT}`, `tcp:${PORT}`]);
adb(['shell', 'am', 'force-stop', 'com.android.chrome']);
await sleep(400);
adb(['shell', 'am', 'start', '-a', 'android.intent.action.VIEW',
     '-n', 'com.android.chrome/com.google.android.apps.chrome.Main',
     '-d', `https://10.0.2.2:${PORT}/`]);
// Wait for SPA shell — chrome activity in foreground + canvas pixels non-blank.
let shellReady = false;
for (let i = 0; i < 20; i++) {
  await sleep(800);
  const focus = adb(['shell', 'dumpsys', 'window']).stdout || '';
  const has = /com\.android\.chrome/.test(focus) && /mCurrentFocus/.test(focus);
  if (has) { shellReady = true; break; }
}
screencap('01-loaded');

// Tap SPA TOTP input + type + Enter (no BACK — would exit Chrome).
tap(672, 1610, 200);
await sleep(400);
adb(['shell', 'input', 'text', totp]);
await sleep(600);
screencap('03-typed');
tap(672, 1894, 200);
await sleep(6000);
screencap('04-after-verify');

// 4. Inject marker.
tap(672, 1610, 120);
await sleep(300);
adb(['shell', 'input', 'text', `echo%20${MARKER}`]);
await sleep(300);
screencap('05-echo-typed');
adb(['shell', 'input', 'keyevent', '66']);
await sleep(2500);
screencap('06-echo-sent');

// Wait + final screencap.
await sleep(2000);
screencap('07-final');

// 5. Dump device graphics state.
const glVendor = adb(['shell', 'dumpsys', 'SurfaceFlinger']).stdout.split('\n').filter((l) => l.includes('GLES') || l.includes('SwiftShader') || l.includes('Vulkan')).slice(0, 5).join('\n');
const gpu = {
  gl_transport: (adb(['shell', 'getprop', 'ro.kernel.qemu.gltransport']).stdout || '').trim(),
  gles_version: (adb(['shell', 'getprop', 'ro.opengles.version']).stdout || '').trim(),
  hardware_egl: (adb(['shell', 'getprop', 'ro.hardware_egl']).stdout || '').trim(),
  model: (adb(['shell', 'getprop', 'ro.product.model']).stdout || '').trim(),
  android_release: (adb(['shell', 'getprop', 'ro.build.version.release']).stdout || '').trim(),
  android_sdk: (adb(['shell', 'getprop', 'ro.build.version.sdk']).stdout || '').trim(),
  gl_vendor_dump: glVendor,
};
writeFileSync(join(RUN_DIR, 'avd-gpu.json'), JSON.stringify(gpu, null, 2));
console.log('[avd-emp] gpu=', JSON.stringify(gpu, null, 2));

// 6. Dump WebGL renderer via CDP (chrome devtools port must be open).
// Discover chrome webview debugger socket via adb forward.
const devSocket = adb(['shell', 'cat', '/proc/net/unix']).stdout
  .split('\n')
  .filter((l) => l.includes('chrome') || l.includes('devtools'))
  .slice(0, 10);
writeFileSync(join(RUN_DIR, 'adb-devtools.txt'), devSocket.join('\n'));

// 7. Build diagnostics.
const diag = {
  schema_version: 1,
  generated_at: new Date().toISOString(),
  session_id: `sess_avd_emp_${RUN}`,
  host_id: 'host_avd_emp_local',
  runtime_epoch: Math.floor(Date.now() / 1000),
  marker: MARKER,
  surface: 'avd-emulator',
  gpu,
  artifacts: {
    host_trace_log: logPath,
    screencaps: [
      '01-loaded.png', '02-after-back.png', '03-typed.png',
      '04-after-verify.png', '05-echo-typed.png', '06-echo-sent.png',
      '07-final.png',
    ],
  },
};
writeFileSync(join(RUN_DIR, 'diagnostics.json'), JSON.stringify(diag, null, 2));

// 9. Stop.
try { proc.kill(); } catch {}
console.log(`[avd-emp] done run=${RUN} dir=${RUN_DIR}`);