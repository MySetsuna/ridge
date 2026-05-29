/**
 * Frame-time + attribution wdio config (P3.14.r4 / §P4, 2026-05-20).
 *
 * Picks up every spec in `tests/e2e-perf/frame-*.spec.ts` so the
 * `perf:frame` orchestrator runs both the headline `frame-time`
 * spec (rAF interval distribution) AND the §P4 `frame-time-
 * attribution` spec (per-source `performance.measure` breakdown)
 * in one round. The stress workload + 25 s sample window is
 * identical, so the two specs' numbers align frame-for-frame.
 *
 * Excludes `stress.spec.ts` — that's driven by `perf:compare` with
 * an external CPU/RSS sampler.
 *
 * Inherits NO_PROXY / tauri-driver lifecycle / capabilities from
 * wdio.conf.ts.
 */
// @ts-nocheck
import { config as baseConfig } from './wdio.conf';

export const config: WebdriverIO.Config = {
  ...baseConfig,
  specs: ['./tests/e2e-perf/frame-*.spec.ts'],
  mochaOpts: {
    ui: 'bdd',
    timeout: 180_000,
  },
};
