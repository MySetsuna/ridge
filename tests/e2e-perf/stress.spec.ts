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

// P4.4 (2026-05-21) — `RIDGE_PERF_BACKEND` was an env knob the perf
// orchestrator used to A/B compare 'rust' vs 'wasm'. Rust is now the
// only path; the env var is still read so old `perf-compare.ps1`
// invocations don't crash, but it no longer triggers a localStorage
// flip + refresh.
const BACKEND = (process.env.RIDGE_PERF_BACKEND || 'rust') as 'rust' | 'wasm';
const STRESS_SEC = Math.max(1, parseInt(process.env.RIDGE_PERF_STRESS_SEC || '35', 10) || 35);
// Browser heap is exposed by Chromium/WebView2 only when the runtime allows
// it. Keep the probe real (no mock fallback) and make the growth gate opt-in:
// CI/device baselines differ, but a soak run must still report availability
// and samples so an unavailable probe cannot be mistaken for a clean result.
const HEAP_GROWTH_MAX_MB = Number(process.env.RIDGE_PERF_HEAP_GROWTH_MAX_MB ?? 0);
const WORKER_PENDING_MAX = Number(process.env.RIDGE_PERF_WORKER_PENDING_MAX ?? 0);

describe(`perf stress (${BACKEND})`, () => {
  before(async () => {
    await waitForAppReady();
    // The in-page memory sampler runs for the whole stress window. A bounded
    // WebDriver script timeout prevents a detached WebView from hanging the
    // run forever while still allowing the configured soak duration.
    await browser.setTimeout({ script: (STRESS_SEC + 30) * 1000 });
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
    // throughput, then sample real browser heap/resource counters during the
    // stress window. `performance.memory` is unavailable on some WebView2
    // builds; those runs are reported as unavailable, never as zero usage.
    await browser.pause(500);
    // eslint-disable-next-line no-console
    console.log(`[perf-stress] entering ${STRESS_SEC}s sample window`);
    const memory = await browser.executeAsync((seconds, doneCb) => {
      const perf = performance as Performance & {
        memory?: {
          usedJSHeapSize?: number;
          totalJSHeapSize?: number;
          jsHeapSizeLimit?: number;
        };
      };
      const samples: Array<{
        atMs: number;
        usedHeapBytes: number | null;
        totalHeapBytes: number | null;
        heapLimitBytes: number | null;
        resourceEntries: number;
        workerPending: number | null;
      }> = [];
      const read = () => {
        const heap = perf.memory;
        const finite = (value: unknown) => typeof value === 'number' && Number.isFinite(value) ? value : null;
        let workerPending: number | null = null;
        try {
          const bridge = (window as Window & {
            __windE2E?: { workerBridge?: () => { active: boolean; pending: number } };
          }).__windE2E?.workerBridge?.();
          workerPending = finite(bridge?.pending);
        } catch {
          // Diagnostic hook is dev-only; absence is reported as null.
        }
        samples.push({
          atMs: Math.round(performance.now()),
          usedHeapBytes: finite(heap?.usedJSHeapSize),
          totalHeapBytes: finite(heap?.totalJSHeapSize),
          heapLimitBytes: finite(heap?.jsHeapSizeLimit),
          resourceEntries: performance.getEntriesByType('resource').length,
          workerPending,
        });
      };
      const startedAt = performance.now();
      read();
      const interval = window.setInterval(read, 1000);
      window.setTimeout(() => {
        window.clearInterval(interval);
        read();
        doneCb({ completed: true, elapsedMs: Math.round(performance.now() - startedAt), samples });
      }, Math.max(1, Number(seconds)) * 1000);
    }, STRESS_SEC);
    // eslint-disable-next-line no-console
    console.log(`[perf-stress] sample window done, exiting`);

    const heapSamples = (memory?.samples ?? []).filter((sample) => sample.usedHeapBytes !== null);
    const initialHeap = heapSamples[0]?.usedHeapBytes ?? null;
    const maxHeap = heapSamples.reduce<number | null>(
      (max, sample) => max === null ? sample.usedHeapBytes : Math.max(max, sample.usedHeapBytes ?? max),
      null,
    );
    const resourceEntriesStart = memory?.samples?.[0]?.resourceEntries ?? 0;
    const resourceEntriesEnd = memory?.samples?.at(-1)?.resourceEntries ?? resourceEntriesStart;
    const workerSamples = (memory?.samples ?? []).filter((sample) => sample.workerPending !== null);
    const workerPendingMax = workerSamples.reduce<number | null>(
      (max, sample) => max === null ? sample.workerPending : Math.max(max, sample.workerPending ?? max),
      null,
    );
    const workerPendingEnd = workerSamples.at(-1)?.workerPending ?? null;
    const memoryReport = {
      completed: memory?.completed === true,
      elapsedMs: memory?.elapsedMs ?? 0,
      samples: memory?.samples?.length ?? 0,
      heapAvailable: heapSamples.length > 0,
      initialHeapMb: initialHeap === null ? null : Number((initialHeap / 1048576).toFixed(1)),
      maxHeapMb: maxHeap === null ? null : Number((maxHeap / 1048576).toFixed(1)),
      maxHeapGrowthMb: initialHeap === null || maxHeap === null
        ? null
        : Number(((maxHeap - initialHeap) / 1048576).toFixed(1)),
      resourceEntriesStart,
      resourceEntriesEnd,
      resourceEntryGrowth: resourceEntriesEnd - resourceEntriesStart,
      workerPendingMax,
      workerPendingEnd,
    };
    // eslint-disable-next-line no-console
    console.log('[perf-stress] browser resource/heap soak:', JSON.stringify(memoryReport));
    expect(memoryReport.completed).toBe(true);
    expect(memoryReport.samples).toBeGreaterThanOrEqual(2);
    if (HEAP_GROWTH_MAX_MB > 0 && memoryReport.maxHeapGrowthMb !== null) {
      expect(memoryReport.maxHeapGrowthMb).toBeLessThanOrEqual(HEAP_GROWTH_MAX_MB);
    }
    if (WORKER_PENDING_MAX > 0 && memoryReport.workerPendingMax !== null) {
      expect(memoryReport.workerPendingMax).toBeLessThanOrEqual(WORKER_PENDING_MAX);
    }

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
