/**
 * Frame-time only wdio config (P3.14.r4, 2026-05-20).
 *
 * Targets exactly one spec — frame-time.spec.ts — so the `perf:frame`
 * orchestrator can run it twice (once per backend) without sweeping
 * up stress.spec.ts. Inherits NO_PROXY / tauri-driver lifecycle /
 * capabilities from wdio.conf.ts.
 */
// @ts-nocheck
import { config as baseConfig } from './wdio.conf';

export const config: WebdriverIO.Config = {
  ...baseConfig,
  specs: ['./tests/e2e-perf/frame-time.spec.ts'],
  mochaOpts: {
    ui: 'bdd',
    timeout: 180_000,
  },
};
