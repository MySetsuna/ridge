/**
 * P3.14 perf harness — drive real PTY traffic under a known backend.
 *
 * Reads `RIDGE_PERF_BACKEND` ('rust' | 'wasm') and `RIDGE_PERF_STRESS_SEC`
 * (default 35) from env. The spec:
 *   1. Waits for the app to reach pane-ready.
 *   2. Reads current `Settings.parserBackend` from localStorage; if it
 *      differs from the requested backend, flips it + refreshes + waits
 *      again (so the pane attaches under the right producer).
 *   3. Writes a heavy PowerShell loop into the pane's PTY via the
 *      `__windE2E.writePty` helper (which calls the same `write_to_pty`
 *      Tauri command the pane's key encoder uses, so shell output flows
 *      back through whichever backend is configured — DO NOT use
 *      feedPty here, that short-circuits to kernel.feed and bypasses
 *      the Rust producer entirely).
 *   4. Sleeps for `RIDGE_PERF_STRESS_SEC` seconds. The orchestrator
 *      (scripts/perf-compare.ps1) runs perf-bench.ps1 in parallel
 *      during this window, sampling CPU + RSS of the test ridge.exe
 *      process tree.
 *
 * Two runs (rust + wasm) by the orchestrator produce comparable summary
 * files in scripts/perf-runs/.
 */
// @ts-nocheck
import { browser, expect } from '@wdio/globals';
import { waitForAppReady, firstPaneId } from '../e2e-shell/helpers';

const BACKEND = (process.env.RIDGE_PERF_BACKEND || 'rust') as 'rust' | 'wasm';
const STRESS_SEC = parseInt(process.env.RIDGE_PERF_STRESS_SEC || '35', 10);

describe(`perf stress (${BACKEND})`, () => {
  before(async () => {
    await waitForAppReady();
    // Flip backend if needed. Reading localStorage is safe here — the
    // appReady gate guarantees we're past about:blank.
    const current: string = await browser.execute(() => {
      const raw = localStorage.getItem('ridge-settings');
      if (!raw) return 'rust';
      try {
        const obj = JSON.parse(raw);
        return obj.parserBackend || 'rust';
      } catch {
        return 'rust';
      }
    });
    if (current !== BACKEND) {
      // eslint-disable-next-line no-console
      console.log(`[perf-stress] flipping backend from ${current} to ${BACKEND}`);
      await browser.execute((b) => {
        const raw = localStorage.getItem('ridge-settings');
        const obj = raw ? JSON.parse(raw) : {};
        obj.parserBackend = b;
        localStorage.setItem('ridge-settings', JSON.stringify(obj));
      }, BACKEND);
      await browser.refresh();
      await waitForAppReady();
    } else {
      // eslint-disable-next-line no-console
      console.log(`[perf-stress] backend already ${current}, no reload`);
    }
  });

  it(`writes a ${STRESS_SEC}s PowerShell stress stream to PTY`, async () => {
    const paneId = await firstPaneId();
    expect(paneId).toBeTruthy();

    // Loop a large numeric sequence — PowerShell echoes one number per
    // line, which exercises:
    //   - VTE parsing of LF + cursor-down
    //   - row push into scrollback (ScrollbackAppend producer + apply)
    //   - per-row delta encoding (col-range diff producer + apply)
    // 500k iterations at PowerShell's default echo rate floods the
    // pipeline for well over a minute. We stop sampling at STRESS_SEC,
    // so the spec doesn't have to wait for it to drain.
    const cmd = '1..500000 | ForEach-Object { $_ }\r';
    // eslint-disable-next-line no-console
    console.log(`[perf-stress] writing stress command to pane ${paneId}`);
    await browser.execute(
      (id, data) => {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        return (window as any).__windE2E.writePty(id, data);
      },
      paneId,
      cmd,
    );

    // Sentinel: bytes are written to PTY; PowerShell parses the line
    // and starts echoing. Give it ~500 ms to settle into a steady-state
    // throughput, then sleep through the sampling window.
    await browser.pause(500);
    // eslint-disable-next-line no-console
    console.log(`[perf-stress] entering ${STRESS_SEC}s sample window`);
    await browser.pause(STRESS_SEC * 1000);
    // eslint-disable-next-line no-console
    console.log(`[perf-stress] sample window done, exiting`);

    // Smoke: confirm the mirror actually advanced (more than 0 lines of
    // scrollback). This is the only assertion — perf data comes from
    // the external sampler, not from this spec.
    const sb: number = await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      return (window as any).__windE2E.scrollbackLen(id);
    }, paneId);
    // eslint-disable-next-line no-console
    console.log(`[perf-stress] scrollback length at exit: ${sb}`);
    expect(sb).toBeGreaterThan(0);
  });
});
