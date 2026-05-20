/**
 * WebdriverIO + tauri-driver shell-level e2e harness (P3.14, 2026-05-20).
 *
 * Drives the real Tauri application binary (release build) through the
 * WebDriver protocol so spec files in `tests/e2e-shell/` can exercise
 * the rust parser path end-to-end: real PTY, real Tauri commands, real
 * webview render. Distinct from `tests/e2e/` (Playwright + Vite dev
 * server, no Tauri runtime).
 *
 * Prerequisites — install before first run:
 *   pnpm add -D @wdio/cli @wdio/local-runner @wdio/mocha-framework \
 *               @wdio/spec-reporter webdriverio @types/chai chai
 *   cargo install tauri-driver
 *   pnpm tauri build  # produces src-tauri/target/release/ridge.exe
 *
 * Tauri 2 compatibility caveat: tauri-driver's main branch targets
 * Tauri 2; if you hit a `unable to connect` error, fall back to
 * Microsoft's WinAppDriver — same WebDriver protocol, drop-in. macOS
 * is not supported (Apple does not expose WKWebView WebDriver hooks);
 * the harness is Windows + Linux only.
 *
 * Run with:  pnpm e2e:shell
 */

// @ts-nocheck — depends on optional dev deps (see prerequisites above)
import { spawn, ChildProcess } from 'node:child_process';
import path from 'node:path';

const DRIVER_PORT = 4444;
let driverProc: ChildProcess | null = null;

export const config: WebdriverIO.Config = {
  runner: 'local',
  specs: ['./tests/e2e-shell/**/*.spec.ts'],
  maxInstances: 1,
  capabilities: [
    {
      // tauri-driver routes platform-native automation:
      //   - Windows: msedgedriver against WebView2
      //   - Linux:   WebKitWebDriver against webkit2gtk
      browserName: 'wry',
      'tauri:options': {
        application: path.resolve(
          'src-tauri/target/release/ridge.exe',
        ),
      },
    } as WebdriverIO.Capabilities,
  ],
  hostname: '127.0.0.1',
  port: DRIVER_PORT,
  logLevel: 'info',
  framework: 'mocha',
  reporters: ['spec'],
  mochaOpts: {
    ui: 'bdd',
    timeout: 60_000,
  },

  /** Spawn tauri-driver before the spec run, kill it after.
   *  Idempotent — re-running with a driver already on the port falls
   *  through to the connect attempt and surfaces a clearer error than
   *  the address-in-use one wdio would otherwise show. */
  onPrepare() {
    return new Promise<void>((resolve, reject) => {
      driverProc = spawn('tauri-driver', [`--port`, `${DRIVER_PORT}`], {
        stdio: 'inherit',
        shell: true,
      });
      driverProc.on('error', reject);
      // Driver needs ~1 s to bind the WebDriver port.
      setTimeout(resolve, 1_500);
    });
  },
  onComplete() {
    if (driverProc && !driverProc.killed) driverProc.kill();
  },
};
