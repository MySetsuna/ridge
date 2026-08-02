/**
 * Perf-only WebdriverIO config (P3.14, 2026-05-20).
 *
 * Inherits everything from wdio.conf.ts (including the NO_PROXY fix
 * and the tauri-driver spawn/teardown lifecycle) but narrows specs to
 * tests/e2e-perf/ and stretches the mocha timeout so a 35 s stress
 * window has room to breathe.
 *
 * Driven by scripts/perf-compare.ps1 — it sets RIDGE_PERF_BACKEND +
 * RIDGE_PERF_STRESS_SEC, runs this config, and samples in parallel via
 * scripts/perf-bench.ps1.
 */
// @ts-nocheck
import { config as baseConfig } from './wdio.conf';

const stressSeconds = Math.max(1, Number.parseInt(process.env.RIDGE_PERF_STRESS_SEC ?? '35', 10) || 35);
// Keep Mocha's outer timeout ahead of the in-page sampler. This permits real
// long-run WebView2 soak windows (for example 10–30 minutes) while retaining a
// hard upper bound so a detached driver cannot leave a CI job hanging forever.
const perfTimeoutMs = Math.min(24 * 60 * 60 * 1000, (stressSeconds + 120) * 1000);

export const config: WebdriverIO.Config = {
  ...baseConfig,
  specs: ['./tests/e2e-perf/**/*.spec.ts'],
  mochaOpts: {
    ui: 'bdd',
    timeout: perfTimeoutMs,
  },
};
