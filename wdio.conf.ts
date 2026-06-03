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
 *   pnpm tauri build  # produces <repo>/target/release/ridge.exe (workspace root)
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
import { spawn, spawnSync, ChildProcess } from 'node:child_process';
import { openSync, existsSync } from 'node:fs';
import path from 'node:path';
import os from 'node:os';

// Mirrors playwright.config.ts: a developer's HTTP_PROXY (Clash /
// v2ray / corporate gateway) routed loopback through localhost:1080
// and made every WebDriver session POST fail with the generic
// "please make sure you have a WebDriver compatible server running"
// error. Adding loopback to NO_PROXY fixes that without touching
// the user's shell config. Idempotent — appends to any existing value.
process.env.NO_PROXY = [process.env.NO_PROXY, 'localhost,127.0.0.1,::1']
  .filter(Boolean)
  .join(',');

// §1.35 (2026-05-24) — isolate the test ridge's WebView2 user-data-dir
// from the installed `C:\Program Files\ridge\ridge.exe` host. Both
// binaries share `identifier: "com.tauri-app.ridge"` (see
// `src-tauri/tauri.conf.json`), so by default both resolve the
// SAME WebView2 user-data folder
// (`%LOCALAPPDATA%\com.tauri-app.ridge\EBWebView`). WebView2 enforces
// an exclusive lock per data dir — when the host ridge is already
// running, the test ridge spawned by tauri-driver hangs at boot
// with HRESULT 0x8007139F (ERROR_INVALID_STATE), surfaced to wdio
// as "app never reached pane-attached state" after the 30 s
// `waitForAppReady` timeout.
//
// Same pattern as `scripts/tauri-dev-cdp.mjs` for the dev launcher.
// The dir is project-local + .gitignored via the catch-all
// `.webview2-*` entry that ships with the repo. Set BEFORE
// `tauri-driver` spawns so the var lands in the inherited env of
// every msedgedriver / ridge.exe child.
// §1.35 fix RE-ENABLED (2026-06-04): isolate the test ridge's WebView2
// user-data dir from the installed `C:\Program Files\ridge\ridge.exe` host.
// They share `identifier: "com.tauri-app.ridge"`, so without this they
// resolve the SAME WebView2 user-data folder; WebView2's exclusive lock
// then hangs the test ridge at boot (about:blank). Same pattern as
// `scripts/tauri-dev-cdp.mjs`. `.webview2-*` is gitignored.
process.env.WEBVIEW2_USER_DATA_FOLDER = path.resolve('.webview2-e2e');

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
          // Workspace root target dir (ridge-core extraction moved it here from
          // src-tauri/target). `pnpm tauri build` → <repo>/target/release/ridge.exe.
          'target/release/ridge.exe',
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
  async onPrepare() {
    // Absolute path to tauri-driver — `spawn` without `shell:true` on
    // Windows does NOT resolve PATH for bare executable names, so a
    // literal "tauri-driver" silently fails with ENOENT swallowed by
    // the inherited stdio. We need a concrete, existing path.
    //
    // Resolution order (most → least specific):
    //   1. `TAURI_DRIVER_BIN` — explicit override.
    //   2. `CARGO_HOME/bin/tauri-driver(.exe)` — honours dev boxes whose
    //      cargo is relocated (`C:\DevKit\Rust\.cargo`, `/opt/cargo`,
    //      etc.). cargo itself follows this env var, so trusting it
    //      here matches the install contract.
    //   3. PATH lookup via `where` / `which` — recovers when the driver
    //      lives somewhere unusual but is reachable on PATH.
    //   4. `~/.cargo/bin/tauri-driver(.exe)` — stock cargo layout.
    //
    // (2026-05-22) — the prior USERPROFILE-only lookup left this dev
    // box at ENOENT every run because CARGO_HOME is overridden to
    // `C:\DevKit\Rust\.cargo`. Surfaces as wdio sessions silently
    // hitting an already-leaked tauri-driver on :4444 from a previous
    // run — when that gets cleaned up, every spec times out 30 s in
    // `waitForAppReady` with a misleading "never reached pane-attached
    // state" message. Keep this resolution chain intact.
    const driverExeName = process.platform === 'win32' ? 'tauri-driver.exe' : 'tauri-driver';
    const pathLookup = (() => {
      const cmd = process.platform === 'win32' ? 'where' : 'which';
      const r = spawnSync(cmd, [driverExeName], { encoding: 'utf8', shell: false });
      if (r.status !== 0 || !r.stdout) return null;
      const first = r.stdout.split(/\r?\n/).map((s) => s.trim()).find(Boolean);
      return first && existsSync(first) ? first : null;
    })();
    const candidates = [
      process.env.TAURI_DRIVER_BIN,
      process.env.CARGO_HOME && path.join(process.env.CARGO_HOME, 'bin', driverExeName),
      pathLookup,
      path.join(
        process.env.USERPROFILE || process.env.HOME || '',
        '.cargo',
        'bin',
        driverExeName,
      ),
    ].filter(Boolean) as string[];
    const driverBin = candidates.find((p) => existsSync(p)) ?? candidates[0];
    const logFile = path.join(os.tmpdir(), 'tauri-driver.log');
    const out = openSync(logFile, 'a');
    const err = openSync(logFile, 'a');
    driverProc = spawn(driverBin, ['--port', String(DRIVER_PORT)], {
      stdio: ['ignore', out, err],
      shell: false,
      windowsHide: true,
      detached: true,
    });
    // Detach so the driver outlives any accidental SIGTERM cascading
    // from a worker fork's exit. onComplete still kills it explicitly.
    driverProc.unref();
    // eslint-disable-next-line no-console
    console.log(`tauri-driver pid=${driverProc.pid}, log=${logFile}`);
    driverProc.on('error', (e) => {
      // eslint-disable-next-line no-console
      console.error(`tauri-driver spawn error (${driverBin}):`, e);
    });
    driverProc.on('exit', (code) => {
      if (code !== null && code !== 0) {
        // eslint-disable-next-line no-console
        console.error(`tauri-driver exited unexpectedly with code ${code}`);
      }
    });
    // Poll /status until the driver reports msedgedriver is ready —
    // a fixed setTimeout produced races where workers tried to POST
    // /session before the underlying msedgedriver had spawned. 15 s
    // ceiling is overkill on dev boxes (real wait is <1 s) but covers
    // CI cold-start.
    const deadline = Date.now() + 15_000;
    while (Date.now() < deadline) {
      try {
        const res = await fetch(`http://127.0.0.1:${DRIVER_PORT}/status`);
        if (res.ok) {
          const body = (await res.json()) as { value?: { ready?: boolean } };
          if (body?.value?.ready) return;
        }
      } catch {
        /* not listening yet */
      }
      await new Promise((r) => setTimeout(r, 300));
    }
    throw new Error('tauri-driver /status never returned ready=true');
  },
  onComplete() {
    if (driverProc && driverProc.pid && !driverProc.killed) {
      // Best-effort. On Windows + detached the tree-kill is needed to
      // also reap the child msedgedriver process — `taskkill /T` does
      // both. Falls back to plain kill on non-Windows.
      try {
        if (process.platform === 'win32') {
          spawn('taskkill', ['/PID', String(driverProc.pid), '/T', '/F'], {
            shell: false,
            stdio: 'ignore',
          });
        } else {
          driverProc.kill();
        }
      } catch { /* already gone */ }
    }
  },
};
