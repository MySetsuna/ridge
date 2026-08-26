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
import { openSync, existsSync, readFileSync, rmSync } from 'node:fs';
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
// The profile is unique per WDIO process and removed after driver shutdown.
// Set it before tauri-driver spawns so every child inherits the same path.
// §1.35 fix RE-ENABLED (2026-06-04): isolate the test ridge's WebView2
// user-data dir from the installed `C:\Program Files\ridge\ridge.exe` host.
// They share `identifier: "com.tauri-app.ridge"`, so without this they
// resolve the SAME WebView2 user-data folder; WebView2's exclusive lock
// then hangs the test ridge at boot (about:blank). Same pattern as
// `scripts/tauri-dev-cdp.mjs`.
const E2E_WEBVIEW_DIR = path.join(os.tmpdir(), `ridge-e2e-webview-${process.pid}`);
process.env.WEBVIEW2_USER_DATA_FOLDER = E2E_WEBVIEW_DIR;
// The installed Ridge instance may still hold the bundle-level
// single-instance mutex while shell tests run.  Keep the test process
// independent; production builds leave this unset and retain single-instance
// behavior.
process.env.RIDGE_DISABLE_SINGLE_INSTANCE = '1';
// Enables the in-page test API through a Rust-injected, process-local flag.
// Normal production launches leave it unset, so mutating hooks stay absent.
process.env.RIDGE_E2E = '1';
const E2E_KERNEL_DIR = path.join(os.tmpdir(), `ridge-e2e-kernel-${process.pid}`);
process.env.RIDGE_KERNEL_DATA_DIR = E2E_KERNEL_DIR;

const DRIVER_PORT = 4444;
let driverProc: ChildProcess | null = null;
let kernelProc: ChildProcess | null = null;

function processAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

function killVerifiedE2eKernel(pid: number): void {
  if (!processAlive(pid)) return;
  if (process.platform === 'win32') {
    const probe = spawnSync('powershell.exe', [
      '-NoProfile',
      '-Command',
      `(Get-CimInstance Win32_Process -Filter "ProcessId = ${pid}").CommandLine`,
    ], { encoding: 'utf8', shell: false, windowsHide: true, timeout: 5_000 });
    const commandLine = probe.stdout?.trim() ?? '';
    const expected = path.resolve('target/release/ridge.exe').toLowerCase();
    if (!commandLine.toLowerCase().includes(expected) || !commandLine.includes('--ridge-kernel-host')) {
      throw new Error(`refusing to kill unverified E2E kernel PID ${pid}`);
    }
    spawnSync('taskkill', ['/PID', String(pid), '/T', '/F'], {
      shell: false, stdio: 'ignore', windowsHide: true, timeout: 10_000,
    });
  } else {
    try { process.kill(-pid, 'SIGKILL'); }
    catch { process.kill(pid, 'SIGKILL'); }
  }
}

async function stopE2eKernel(): Promise<void> {
  const registry = path.join(E2E_KERNEL_DIR, 'kernel.json');
  let endpoint: { pid: number; port: number; token: string } | null = null;
  try {
    endpoint = JSON.parse(readFileSync(registry, 'utf8'));
  } catch { /* tracked-process fallback below */ }

  if (endpoint && Number.isSafeInteger(endpoint.pid) && endpoint.pid > 0) {
    try {
      await fetch(`http://127.0.0.1:${endpoint.port}/v1/shutdown`, {
        method: 'POST',
        headers: {
          'x-ridge-kernel-token': endpoint.token,
          'x-ridge-token': endpoint.token,
        },
        signal: AbortSignal.timeout(2_000),
      });
    } catch { /* bounded force fallback below */ }

    const deadline = Date.now() + 2_000;
    while (processAlive(endpoint.pid) && Date.now() < deadline) {
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
    killVerifiedE2eKernel(endpoint.pid);
  }

  if (kernelProc?.pid) killVerifiedE2eKernel(kernelProc.pid);
}

async function startE2eKernel(): Promise<void> {
  const application = path.resolve('target/release/ridge.exe');
  if (!existsSync(application)) {
    throw new Error(`release application missing: ${application}`);
  }
  rmSync(E2E_KERNEL_DIR, { recursive: true, force: true });
  kernelProc = spawn(application, ['--ridge-kernel-host'], {
    env: process.env,
    stdio: 'ignore',
    shell: false,
    windowsHide: true,
    detached: process.platform !== 'win32',
  });
  let spawnError: unknown;
  kernelProc.once('error', (error) => {
    spawnError = error;
  });

  try {
    const registry = path.join(E2E_KERNEL_DIR, 'kernel.json');
    const deadline = Date.now() + 15_000;
    while (Date.now() < deadline) {
      if (spawnError) throw spawnError;
      if (kernelProc.exitCode !== null) {
        throw new Error(`E2E kernel exited before ready with code ${kernelProc.exitCode}`);
      }
      try {
        const endpoint = JSON.parse(readFileSync(registry, 'utf8')) as { pid: number; port: number };
        if (endpoint.pid !== kernelProc.pid) {
          throw new Error(`isolated kernel PID mismatch: spawned ${kernelProc.pid}, registry ${endpoint.pid}`);
        }
        const response = await fetch(`http://127.0.0.1:${endpoint.port}/v1/health`, {
          signal: AbortSignal.timeout(1_000),
        });
        const health = response.ok ? await response.json() as { ok?: boolean; role?: string } : null;
        if (health?.ok === true && health.role === 'ridge-kernel') return;
      } catch (error) {
        if (error instanceof Error && error.message.startsWith('isolated kernel PID mismatch')) {
          throw error;
        }
      }
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    throw new Error('isolated E2E kernel did not become healthy within 15s');
  } catch (error) {
    await stopE2eKernel();
    throw error;
  }
}

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
      'wdio:enforceWebDriverClassic': true,
      'tauri:options': {
        // Workspace root target dir (ridge-core extraction moved it here from
        // src-tauri/target). `pnpm tauri build` → <repo>/target/release/ridge.exe.
        application: path.resolve('target/release/ridge.exe'),
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
    // tauri-driver places the application in a Windows Job. Ridge deliberately
    // refuses to spawn its detached kernel from that Job when BREAKAWAY is
    // denied, so start the isolated kernel from this parent first.
    await startE2eKernel();
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
    const nativeDriver = process.env.TAURI_NATIVE_DRIVER?.trim();
    const driverArgs = ['--port', String(DRIVER_PORT)];
    // tauri-driver otherwise resolves msedgedriver from PATH. An explicit
    // path lets CI/local runs pin it to the installed WebView2 runtime.
    if (nativeDriver && existsSync(nativeDriver)) {
      driverArgs.push('--native-driver', nativeDriver);
      console.log(`tauri-driver native driver=${nativeDriver}`);
    }
    driverProc = spawn(driverBin, driverArgs, {
      stdio: ['ignore', out, err],
      shell: false,
      windowsHide: true,
    });
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
  async onComplete() {
    let kernelError: unknown;
    try {
      await stopE2eKernel();
    } catch (error) {
      kernelError = error;
    } finally {
      if (driverProc && driverProc.pid && !driverProc.killed) {
        // Wait for tree cleanup before WDIO exits; an async taskkill can be
        // abandoned with msedgedriver and the application still alive.
        try {
          if (process.platform === 'win32') {
            spawnSync('taskkill', ['/PID', String(driverProc.pid), '/T', '/F'], {
              shell: false,
              stdio: 'ignore',
              windowsHide: true,
              timeout: 10_000,
            });
          } else {
            driverProc.kill();
          }
        } catch { /* already gone */ }
      }
      driverProc = null;
      kernelProc = null;
      rmSync(E2E_KERNEL_DIR, { recursive: true, force: true });
      rmSync(E2E_WEBVIEW_DIR, { recursive: true, force: true });
    }
    if (kernelError) throw kernelError;
  },
};
