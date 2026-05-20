/**
 * One-shot wdio config for restoring shared localStorage state
 * (P3.14.r3, 2026-05-20). Inherits driver lifecycle / proxy /
 * capabilities from wdio.conf.ts, narrows specs to tests/e2e-utils/.
 *
 * Trigger: `pnpm e2e:reset` after running the e2e-shell or perf
 * suite — those leave parserBackend in a non-default state on the
 * shared WebView2 user-data-dir.
 */
// @ts-nocheck
import { config as baseConfig } from './wdio.conf';

export const config: WebdriverIO.Config = {
  ...baseConfig,
  specs: ['./tests/e2e-utils/**/*.spec.ts'],
};
