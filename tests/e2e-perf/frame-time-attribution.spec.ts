/**
 * §P4 frame-time attribution (2026-05-24).
 *
 * Companion to `frame-time.spec.ts`. That spec asks "how janky is
 * the rAF interval distribution under PTY stress?" — useful as a
 * headline number and regression gate, but it can't answer:
 *
 *   "Within a long frame, where did the time actually go?"
 *
 * P4 plan target: drive p95 from ~50 ms → 20–25 ms by reducing the
 * two known main-thread bottlenecks:
 *
 *   1. Tauri event marshaling — every `pty-output-*` event runs
 *      through base64 + JSON-wrap + event-name routing before our JS
 *      handler sees the bytes. Bypassed for postcard deltas via the
 *      P4.3 `Channel<DeltaPayload>` (`rg.ptyDelta.apply`); still in
 *      play for the text path (`rg.ptyText.feed`).
 *   2. Main-thread canvas paint — rAF tick body in
 *      `manager.ts::startRafLoop` (`rg.frame.tick`).
 *
 * Methodology:
 *   - Flip `window.__RIDGE_PERF_TRACE = true` BEFORE the stress
 *     workload starts. `src/lib/terminal/perfTrace.ts::perfMark`
 *     gates `performance.mark`/`measure` on that flag — defaults off
 *     in production for sub-microsecond branch cost on hot paths.
 *   - Attach a PerformanceObserver inside the webview before the
 *     stress fires, accumulating every `entryType: 'measure'` into a
 *     per-label bucket of durations.
 *   - Pump the SAME 50k-line PowerShell loop as `frame-time.spec.ts`
 *     (so the two specs' numbers align — paint cost + feed cost +
 *     idle = rAF interval).
 *   - At end of window: aggregate per-label sum / count /
 *     p50 / p95 / p99, log + persist, optionally assert per-label
 *     budgets.
 *
 * Reading the output:
 *   - `rg.frame.tick.p99` high but `rg.ptyText.feed.sum` low → paint
 *     is the bottleneck.
 *   - `rg.ptyText.feed.sum` is a large fraction of total stress time
 *     → the JSON event path is still hot; the postcard channel
 *     isn't winning the bytes it should be.
 *   - `rg.ptyDelta.apply.p99` high → the binary path itself got
 *     slower (decode bug? grid mutation regression?).
 *
 * Thresholds are env-overridable (same pattern as `frame-time.spec.ts`):
 *   RIDGE_PERF_ASSERT=0 disables the gate (pure data collection).
 *   RIDGE_PERF_TICK_P95_MAX_MS  (default 40)
 *   RIDGE_PERF_DELTA_P95_MAX_MS (default 5)
 *   RIDGE_PERF_TEXT_P95_MAX_MS  (default 10)
 *
 * Run via:  pnpm perf:frame   (orchestrator) — picked up by the
 * `tests/e2e-perf/**\/*.spec.ts` glob in wdio.conf.ts (this spec
 * lives next to `frame-time.spec.ts`).
 */
// @ts-nocheck
import { browser, expect } from '@wdio/globals';
import { writeFileSync, mkdirSync, existsSync } from 'node:fs';
import path from 'node:path';
import { waitForAppReady, firstPaneId } from '../e2e-shell/helpers';

const STRESS_SEC = parseInt(process.env.RIDGE_PERF_STRESS_SEC || '25', 10);
const ASSERT_ENABLED = process.env.RIDGE_PERF_ASSERT !== '0';
const TICK_P95_MAX_MS = Number(process.env.RIDGE_PERF_TICK_P95_MAX_MS ?? 40);
const DELTA_P95_MAX_MS = Number(process.env.RIDGE_PERF_DELTA_P95_MAX_MS ?? 5);
const TEXT_P95_MAX_MS = Number(process.env.RIDGE_PERF_TEXT_P95_MAX_MS ?? 10);

interface LabelStats {
  count: number;
  sumMs: number;
  p50Ms: number;
  p95Ms: number;
  p99Ms: number;
  maxMs: number;
}

describe('frame-time attribution (rg.frame.tick / rg.ptyText.feed / rg.ptyDelta.apply)', () => {
  before(async () => {
    await waitForAppReady();
    await browser.setTimeout({ script: (STRESS_SEC + 30) * 1000 });
  });

  after(async () => {
    // Best-effort: clear the trace flag so a later spec running in
    // the same session doesn't accidentally accumulate measures.
    await browser.execute(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (window as any).__RIDGE_PERF_TRACE = false;
    });
  });

  it(`samples mark+measure spans during ${STRESS_SEC}s of PTY stress`, async () => {
    const paneId = await firstPaneId();

    // Same workload as frame-time.spec.ts so the two specs' numbers
    // align frame-for-frame.
    const cmd =
      '1..50000 | ForEach-Object { [char]27 + "[31;1m" + ("line " + $_).PadRight(80, "X") + [char]27 + "[0m" }\r';

    // Enable tracing BEFORE writing the workload so the very first
    // event is captured.
    await browser.execute(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (window as any).__RIDGE_PERF_TRACE = true;
    });

    // eslint-disable-next-line no-console
    console.log('[attribution] writing stress command');
    await browser.execute(
      (id, data) => (window as any).__windE2E.writePty(id, data),
      paneId,
      cmd,
    );
    await browser.pause(500);

    // eslint-disable-next-line no-console
    console.log(`[attribution] observing measures for ${STRESS_SEC}s`);
    const result = await browser.executeAsync((seconds, doneCb) => {
      const buckets = new Map<string, number[]>();
      const observer = new PerformanceObserver((entries) => {
        for (const e of entries.getEntries()) {
          if (e.entryType !== 'measure') continue;
          const arr = buckets.get(e.name) ?? [];
          arr.push(e.duration);
          buckets.set(e.name, arr);
        }
        // Drain processed entries so the next batch starts fresh.
        performance.clearMeasures();
      });
      observer.observe({ entryTypes: ['measure'], buffered: false });
      const t0 = performance.now();
      setTimeout(() => {
        observer.disconnect();
        const durationMs = performance.now() - t0;
        const summary: Record<
          string,
          {
            count: number;
            sumMs: number;
            p50Ms: number;
            p95Ms: number;
            p99Ms: number;
            maxMs: number;
          }
        > = {};
        for (const [label, samples] of buckets.entries()) {
          if (samples.length === 0) continue;
          const sorted = samples.slice().sort((a, b) => a - b);
          const n = sorted.length;
          const pct = (q: number) => sorted[Math.min(n - 1, Math.floor(n * q))];
          const sum = samples.reduce((s, x) => s + x, 0);
          summary[label] = {
            count: n,
            sumMs: Number(sum.toFixed(2)),
            p50Ms: Number(pct(0.5).toFixed(3)),
            p95Ms: Number(pct(0.95).toFixed(3)),
            p99Ms: Number(pct(0.99).toFixed(3)),
            maxMs: Number(sorted[n - 1].toFixed(3)),
          };
        }
        doneCb({ durationMs, summary });
      }, seconds * 1000);
    }, STRESS_SEC);

    // eslint-disable-next-line no-console
    console.log('[attribution]', JSON.stringify(result, null, 2));

    const outDir = path.resolve(process.cwd(), 'scripts', 'perf-runs');
    if (!existsSync(outDir)) mkdirSync(outDir, { recursive: true });
    const ts = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
    const file = path.join(outDir, `p4-attribution-${ts}.json`);
    writeFileSync(file, JSON.stringify(result, null, 2));
    // eslint-disable-next-line no-console
    console.log(`[attribution] wrote ${file}`);

    const tick = result.summary['rg.frame.tick'] as LabelStats | undefined;
    const text = result.summary['rg.ptyText.feed'] as LabelStats | undefined;
    const delta = result.summary['rg.ptyDelta.apply'] as LabelStats | undefined;

    // Sanity: tick must have fired (the rAF loop is always running).
    expect(tick).toBeTruthy();
    expect(tick!.count).toBeGreaterThan(0);

    if (ASSERT_ENABLED) {
      // eslint-disable-next-line no-console
      console.log(
        `[attribution] gate: tick.p95<=${TICK_P95_MAX_MS} delta.p95<=${DELTA_P95_MAX_MS} text.p95<=${TEXT_P95_MAX_MS}`,
      );
      expect(tick!.p95Ms).toBeLessThanOrEqual(TICK_P95_MAX_MS);
      // text + delta only assert when they actually fired (a stress
      // run that happened to route everything through one path
      // shouldn't fail the other path's threshold).
      if (delta && delta.count > 0) {
        expect(delta.p95Ms).toBeLessThanOrEqual(DELTA_P95_MAX_MS);
      }
      if (text && text.count > 0) {
        expect(text.p95Ms).toBeLessThanOrEqual(TEXT_P95_MAX_MS);
      }
    }
  });
});
