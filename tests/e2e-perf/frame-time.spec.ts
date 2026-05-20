/**
 * P3.14.r4 — frame-time benchmark: prove the architectural win of
 * rust mode that the CPU sampler can't see.
 *
 * Hypothesis: in wasm mode, `kernel.feed(rawBytes)` is a synchronous
 * wasm call that runs the entire VTE parser on the main thread; a
 * heavy burst (e.g. PowerShell `1..500000 | %{$_}`) blocks rAF and
 * produces "long frames" (jank). In rust mode, the same bytes flow
 * through `PaneParser` in a tokio task; the main thread only handles
 * `kernel.applyDeltaFrame(deltaBytes)` (postcard decode + targeted
 * grid mutation), which is much cheaper per frame.
 *
 * The CPU bench (p3.14.r3) shows rust costs ~3% MORE total CPU than
 * wasm because of the multi-hop pipeline — that's expected and not a
 * regression. The win shows up here: rAF interval p95/p99 + jank
 * counts should be DRAMATICALLY lower in rust mode under the same
 * stress workload.
 *
 * Methodology:
 *   1. Pump 500k-line PowerShell loop into PTY.
 *   2. From inside the webview, run a rAF tick that records every
 *      frame interval for STRESS_SEC seconds.
 *   3. Compute p50/p95/p99/max + count frames exceeding 33ms / 50ms
 *      / 100ms thresholds.
 *   4. Write JSON to scripts/perf-runs/p3-frame-{backend}-{ts}.json
 *      so the orchestrator can compare both rounds.
 *
 * Run via:  pnpm perf:frame   (orchestrates both backends)
 */
// @ts-nocheck
import { browser, expect } from '@wdio/globals';
import { writeFileSync, mkdirSync, existsSync } from 'node:fs';
import path from 'node:path';
import { waitForAppReady, firstPaneId } from '../e2e-shell/helpers';

const BACKEND = (process.env.RIDGE_PERF_BACKEND || 'rust') as 'rust' | 'wasm';
const STRESS_SEC = parseInt(process.env.RIDGE_PERF_STRESS_SEC || '25', 10);

describe(`frame-time (${BACKEND})`, () => {
  before(async () => {
    await waitForAppReady();
    // WebDriver default script timeout is 30 s — our executeAsync
    // runs for STRESS_SEC + a few ms of post-processing, so bump it
    // generously above 30 s for any STRESS_SEC ≥ ~25 s.
    await browser.setTimeout({ script: (STRESS_SEC + 30) * 1000 });
    const current: string = await browser.execute(() => {
      const raw = localStorage.getItem('ridge-settings');
      if (!raw) return 'rust';
      try {
        return JSON.parse(raw).parserBackend || 'rust';
      } catch {
        return 'rust';
      }
    });
    if (current !== BACKEND) {
      // eslint-disable-next-line no-console
      console.log(`[frame-time] flipping backend ${current} → ${BACKEND}`);
      await browser.execute((b) => {
        const raw = localStorage.getItem('ridge-settings');
        const obj = raw ? JSON.parse(raw) : {};
        obj.parserBackend = b;
        localStorage.setItem('ridge-settings', JSON.stringify(obj));
      }, BACKEND);
      await browser.refresh();
      await waitForAppReady();
      await browser.setTimeout({ script: (STRESS_SEC + 30) * 1000 });
    }
  });

  it(`samples rAF intervals during ${STRESS_SEC}s of PTY stress`, async () => {
    const paneId = await firstPaneId();

    // Workload tuning notes (P3.14.r4):
    //   - `1..500000 | %{$_}`: too light (~20 KB/s). Both backends
    //     held 60 fps with 0/1 jank events — wasm wasn't stressed.
    //   - `1..500000 | %{ SGR + "X"*200 + SGR + " line $_" }`: too
    //     heavy (~110 MB total). Renderer became the bottleneck;
    //     BOTH backends collapsed to ~9 fps with identical jank
    //     histograms — measured the canvas/WebGPU paint cost, not
    //     the parser differential.
    //   - Sweet spot: ~30 KB/line, ANSI SGR per line, fewer
    //     iterations so total ~3-5 MB stretched over 25 s. Parser
    //     stays busy; renderer keeps up; difference between
    //     kernel.feed (full VTE) and applyDeltaFrame (postcard
    //     decode + grid mutation) shows.
    const cmd =
      '1..50000 | ForEach-Object { [char]27 + "[31;1m" + ("line " + $_).PadRight(80, "X") + [char]27 + "[0m" }\r';
    // eslint-disable-next-line no-console
    console.log(`[frame-time ${BACKEND}] writing stress command`);
    await browser.execute(
      (id, data) => (window as any).__windE2E.writePty(id, data),
      paneId,
      cmd,
    );
    // 500 ms settle so PowerShell is in steady-state echo before we
    // start measuring.
    await browser.pause(500);

    // eslint-disable-next-line no-console
    console.log(`[frame-time ${BACKEND}] sampling rAF for ${STRESS_SEC}s`);
    const stats = await browser.executeAsync((seconds, doneCb) => {
      const intervals: number[] = [];
      let last = performance.now();
      let raf = 0;
      function tick(t: number) {
        intervals.push(t - last);
        last = t;
        raf = requestAnimationFrame(tick);
      }
      raf = requestAnimationFrame(tick);
      setTimeout(() => {
        cancelAnimationFrame(raf);
        // First sample is the gap between rAF schedule and first
        // callback — drop it so we measure steady-state only.
        if (intervals.length > 1) intervals.shift();
        const sorted = intervals.slice().sort((a, b) => a - b);
        const n = sorted.length;
        const pct = (q: number) =>
          n > 0 ? sorted[Math.min(n - 1, Math.floor(n * q))] : 0;
        const sum = intervals.reduce((s, x) => s + x, 0);
        doneCb({
          frames: n,
          durationMs: Math.round(sum),
          fps: n > 0 ? Number(((n / (sum / 1000)) || 0).toFixed(1)) : 0,
          meanMs: n > 0 ? Number((sum / n).toFixed(2)) : 0,
          p50Ms: Number(pct(0.5).toFixed(2)),
          p95Ms: Number(pct(0.95).toFixed(2)),
          p99Ms: Number(pct(0.99).toFixed(2)),
          maxMs: n > 0 ? Number(sorted[n - 1].toFixed(2)) : 0,
          // Jank thresholds (count of frames exceeding each):
          //   33 ms  ≈ 2 missed vsyncs at 60 Hz
          //   50 ms  → user-perceptible stutter
          //   100 ms → "the UI froze" for an observer
          jank33: intervals.filter((s) => s > 33).length,
          jank50: intervals.filter((s) => s > 50).length,
          jank100: intervals.filter((s) => s > 100).length,
        });
      }, seconds * 1000);
    }, STRESS_SEC);

    // eslint-disable-next-line no-console
    console.log(`[frame-time ${BACKEND}]`, JSON.stringify(stats, null, 2));

    // Persist for orchestrator comparison.
    const outDir = path.resolve(process.cwd(), 'scripts', 'perf-runs');
    if (!existsSync(outDir)) mkdirSync(outDir, { recursive: true });
    const ts = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
    const file = path.join(outDir, `p3-frame-${BACKEND}-${ts}.json`);
    writeFileSync(file, JSON.stringify({ backend: BACKEND, ...stats }, null, 2));
    // eslint-disable-next-line no-console
    console.log(`[frame-time ${BACKEND}] wrote ${file}`);

    expect(stats.frames).toBeGreaterThan(0);
  });
});
