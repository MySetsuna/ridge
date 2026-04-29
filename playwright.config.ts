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
export default defineConfig({
  testDir: './tests/e2e',
  timeout: 30_000,
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
    baseURL: 'http://localhost:5173',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
  },
  webServer: {
    command: 'pnpm dev',
    url: 'http://localhost:5173',
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
    stdout: 'pipe',
    stderr: 'pipe',
  },
});
