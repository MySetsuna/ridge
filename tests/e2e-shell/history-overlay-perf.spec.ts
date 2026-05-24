/**
 * Performance lock for the wasm shell-history overlay (§1.34, 2026-05-22).
 *
 * Background: the popup migrated from a Svelte `<TerminalHistoryPopup>`
 * DOM element to a wasm canvas overlay (`HistoryOverlay` in
 * `packages/ridge-term/src/render/renderer.rs`). The migration's
 * expected wins:
 *
 *   1. Open / close is a single wasm call (setHistoryOverlay /
 *      clearHistoryOverlay) plus a JS-side mirror map update — no
 *      Svelte mount cycle, no `computePopupPosition` round-trip, no
 *      separate DOM paint pass on top of the canvas.
 *   2. While the overlay is visible, the only added render cost is the
 *      extra `CellInstance`s the wasm renderer pushes into the
 *      already-active webgpu pass — no new composite layer, no DOM
 *      layout dirtying.
 *   3. Arrow-key navigation (rapid setHistoryOverlay calls with a
 *      different selectedIndex per call, mimicking a user holding
 *      ArrowDown) stays sub-frame.
 *
 * Thresholds are intentionally generous so this spec catches a real
 * regression (e.g. an accidental N² walk through items, or a glyph
 * atlas eviction storm) without flaking on slow CI:
 *
 *   - p99 of a single `setHistoryOverlay` JS-side call < 5 ms
 *   - p99 of a single `clearHistoryOverlay` JS-side call < 5 ms
 *   - 30 consecutive setHistoryOverlay calls (arrow-down hold) total
 *     < 100 ms
 *
 * The JS-side measurement covers: wasm Handle.setHistoryOverlay
 * (postcard-free direct call), wasm `HistoryOverlay::layout`, the JS
 * mirror map update, and the manager's `wake()` queue push. It does
 * NOT include the next paint (we're not blocking on rAF) — that would
 * mix in unrelated frame jitter. Paint latency is implicitly bounded
 * by the rAF cadence the existing frame-time.spec.ts already locks.
 *
 * If this spec ever fails, candidate root causes:
 *   - `_lastHistoryOverlayCall` map grew expensive (e.g. spread copy
 *     of a huge items array on every call — currently capped at 10).
 *   - `HistoryOverlay::layout` started allocating per call.
 *   - `frame_pinned` / `GlyphAtlas` evicting + re-rasterizing the
 *     overlay's row glyphs on every selectedIndex change.
 *   - `manager.wake()` doing more than enqueueing a rAF.
 */
// @ts-nocheck
import { browser, expect } from '@wdio/globals';
import { waitForAppReady, firstPaneId } from './helpers';

describe('shell-history wasm overlay — performance lock (§1.34)', () => {
  let paneId: string;

  before(async () => {
    await waitForAppReady();
    paneId = await firstPaneId();
    // The cold wasm Handle.setHistoryOverlay sometimes pays a one-time
    // init tax for the GlyphAtlas (first non-cell-grid glyphs land at
    // call time). Warm it once outside the measured window so the
    // p99 we assert reflects steady-state cost, not cold start.
    await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const e2e = (window as any).__windE2E;
      e2e.setHistoryOverlay(id, ['warmup'], -1, 0, 0, true);
      e2e.clearHistoryOverlay(id);
    }, paneId);
  });

  it('setHistoryOverlay p99 < 5ms across 50 calls', async () => {
    const stats = (await browser.execute((id: string) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const e2e = (window as any).__windE2E;
      // Realistic payload: 10 items (the popup cap), mixed lengths so
      // the wasm-side cell-width walk does real work per call.
      const items = [
        'ls -la',
        'cd ..',
        'git status',
        'pnpm build',
        'pnpm test',
        'pnpm e2e:shell',
        'cargo build --release',
        'pwsh scripts/perf-frame-compare.ps1',
        'git log --oneline -20',
        'echo "hello world"',
      ];
      const samples: number[] = [];
      for (let i = 0; i < 50; i++) {
        // Rotate selectedIndex so the wasm side can't dedupe to a no-op.
        const sel = i % items.length;
        const t0 = performance.now();
        e2e.setHistoryOverlay(id, items, sel, 5, 0, true);
        samples.push(performance.now() - t0);
      }
      e2e.clearHistoryOverlay(id);
      samples.sort((a, b) => a - b);
      return {
        p50: samples[Math.floor(samples.length / 2)],
        p95: samples[Math.floor(samples.length * 0.95)],
        p99: samples[samples.length - 1],
        max: samples[samples.length - 1],
      };
    }, paneId)) as { p50: number; p95: number; p99: number; max: number };

    // eslint-disable-next-line no-console
    console.log(
      `[perf] setHistoryOverlay  p50=${stats.p50.toFixed(2)}ms  p95=${stats.p95.toFixed(2)}ms  p99=${stats.p99.toFixed(2)}ms`,
    );
    expect(stats.p99).toBeLessThan(5);
  });

  it('clearHistoryOverlay p99 < 5ms across 50 calls', async () => {
    const stats = (await browser.execute((id: string) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const e2e = (window as any).__windE2E;
      const items = ['a', 'b', 'c'];
      const samples: number[] = [];
      for (let i = 0; i < 50; i++) {
        e2e.setHistoryOverlay(id, items, 0, 5, 0, true);
        const t0 = performance.now();
        e2e.clearHistoryOverlay(id);
        samples.push(performance.now() - t0);
      }
      samples.sort((a, b) => a - b);
      return {
        p50: samples[Math.floor(samples.length / 2)],
        p95: samples[Math.floor(samples.length * 0.95)],
        p99: samples[samples.length - 1],
      };
    }, paneId)) as { p50: number; p95: number; p99: number };

    // eslint-disable-next-line no-console
    console.log(
      `[perf] clearHistoryOverlay  p50=${stats.p50.toFixed(2)}ms  p95=${stats.p95.toFixed(2)}ms  p99=${stats.p99.toFixed(2)}ms`,
    );
    expect(stats.p99).toBeLessThan(5);
  });

  it('arrow-down hold (30 rapid setHistoryOverlay calls) finishes in < 100ms', async () => {
    // Simulates a user holding ArrowDown to walk through 30 history
    // entries — the visible-cost path that motivated the migration.
    // We drive the JS bridge directly because going through the
    // pressKey() chain would mix in event-loop overhead the wasm
    // overlay isn't responsible for.
    const totalMs = (await browser.execute((id: string) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const e2e = (window as any).__windE2E;
      const items = [
        'cmd0', 'cmd1', 'cmd2', 'cmd3', 'cmd4',
        'cmd5', 'cmd6', 'cmd7', 'cmd8', 'cmd9',
      ];
      const t0 = performance.now();
      for (let i = 0; i < 30; i++) {
        e2e.setHistoryOverlay(id, items, i % items.length, 5, 0, true);
      }
      const elapsed = performance.now() - t0;
      e2e.clearHistoryOverlay(id);
      return elapsed;
    }, paneId)) as number;

    // eslint-disable-next-line no-console
    console.log(`[perf] arrow-down hold (30x setHistoryOverlay) = ${totalMs.toFixed(2)}ms`);
    expect(totalMs).toBeLessThan(100);
  });

  it('historyOverlayState mirror is consistent with last setHistoryOverlay call', async () => {
    // Sanity check that pairs with the timing specs above: we never
    // want a fast-but-broken `setHistoryOverlay` to slip past timing
    // assertions while silently corrupting the mirror map.
    const probe = (await browser.execute((id: string) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const e2e = (window as any).__windE2E;
      e2e.setHistoryOverlay(id, ['alpha', 'beta', 'gamma'], 1, 7, 12, false);
      const open = e2e.historyOverlayState(id);
      e2e.clearHistoryOverlay(id);
      const closed = e2e.historyOverlayState(id);
      return { open, closed };
    }, paneId)) as {
      open: { open: boolean; items: string[]; selectedIndex: number; anchorRow: number; anchorCol: number; placeAbove: boolean };
      closed: { open: boolean; items: string[]; selectedIndex: number };
    };

    expect(probe.open.open).toBe(true);
    expect(probe.open.items).toEqual(['alpha', 'beta', 'gamma']);
    expect(probe.open.selectedIndex).toBe(1);
    expect(probe.open.anchorRow).toBe(7);
    expect(probe.open.anchorCol).toBe(12);
    expect(probe.open.placeAbove).toBe(false);
    expect(probe.closed.open).toBe(false);
    expect(probe.closed.items).toEqual([]);
    expect(probe.closed.selectedIndex).toBe(-1);
  });
});
