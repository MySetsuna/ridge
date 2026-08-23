import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright harness for Ridge's frontend smoke tests.
 *
 * Scope is deliberately narrow: we run the SvelteKit dev server (not a Tauri
 * build), visit `/`, and verify the SPA chrome + critical UI contracts that
 * do not require a real Tauri backend. Anything FS-touching is covered by
 * `cargo test`, and component-internal logic is covered by vitest.
 *
 * Not headless? — Set `PWDEBUG=1` or pass `--headed` to inspect visually.
 */

// P1.4 (2026-05-19): merge loopback into NO_PROXY so the webServer URL probe
// + per-test fetches don't get hijacked by a developer's local HTTP_PROXY
// (Clash / v2ray / corporate gateways). Without this, the proxy returns
// 502 for 127.0.0.1:5173 because it can't reach an upstream there, and
// `pnpm e2e` deadlocks for 120 s waiting for the dev server URL. Idempotent
// — appends to any existing NO_PROXY value.
process.env.NO_PROXY = [process.env.NO_PROXY, 'localhost,127.0.0.1,::1']
  .filter(Boolean)
  .join(',');

const e2ePort = process.env.RIDGE_E2E_PORT || '5173';
const e2eOrigin = `http://127.0.0.1:${e2ePort}`;

export default defineConfig({
  testDir: './tests/e2e',
  timeout: 60_000,
  expect: { timeout: 5_000 },
  // Smoke tier is small and shares a single webServer. Parallel workers race
  // each other on hydration timing of the dev server; serial is deterministic
  // and total runtime is still < 1 min.
  fullyParallel: false,
  workers: 1,
  // Only chromium for the smoke tier; matches the Tauri WebView2 lineage on
  // Windows. Cross-browser matrix is out of scope here.
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
  ],
  use: {
    baseURL: e2eOrigin,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
  },
  webServer: {
    command: `pnpm exec vite dev --host 127.0.0.1 --port ${e2ePort} --strictPort`,
    // Probe only server readiness. The tests own full-page boot assertions;
    // probing `/` here duplicates the entire cold SSR compile before a page
    // even exists and misreports app failures as webServer timeouts.
    url: `${e2eOrigin}/@vite/client`,
    // Opt in only when the caller owns that server. Blind reuse can test an
    // unrelated Vite app already listening on the common development port.
    reuseExistingServer: process.env.RIDGE_E2E_REUSE_SERVER === '1',
    timeout: 120_000,
    stdout: 'pipe',
    stderr: 'pipe',
  },
});
