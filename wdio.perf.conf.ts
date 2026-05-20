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

export const config: WebdriverIO.Config = {
  ...baseConfig,
  specs: ['./tests/e2e-perf/**/*.spec.ts'],
  mochaOpts: {
    ui: 'bdd',
    timeout: 180_000,
  },
};
