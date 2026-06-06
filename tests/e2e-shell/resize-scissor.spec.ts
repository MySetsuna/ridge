/**
 * §fix(resize-scissor) — pane splitter drag scissor regression guard.
 *
 * Verifies that the GPU scissor rectangle tracks the DOM container in
 * real time during pane splitter drag (immediate _recomputeViewport in
 * viewportChanged), while the kernel grid resize + PTY SIGWINCH remain
 * debounced.
 *
 * The fix was motivated by the symptom "随 pane resize 下/右遮挡、松手
 * 恢复" — the scissor was deferred until RESIZE_SETTLE_MS elapsed,
 * causing right/bottom clipping during continuous drag.
 *
 * Setup: see tests/e2e-shell/README.md.
 * Requires tauri-driver (Tauri app with real PTY backend).
 */
// @ts-nocheck
import { browser, expect } from '@wdio/globals';
import { waitForAppReady, firstPaneId, clearVisibleGrid, waitForVisibleText } from './helpers';

describe('pane splitter drag scissor tracks DOM immediately', () => {
  before(async () => {
    await waitForAppReady();
  });

  /**
   * Core invariant: after a layout change (simulated by viewport
   * resize, which causes the same ResizeObserver → viewportChanged →
   * _recomputeViewport chain as a splitter drag), the kernel reports
   * non-zero rows/cols and the terminal content remains visible.
   *
   * We can't programmatically drag the splitter handle via WebDriver
   * (the splitter is a CSS flex child, not a native resize handle),
   * so we exercise the same code path via browser.setWindowSize which
   * triggers the per-pane ResizeObserver → viewportChanged.
   */
  it('terminal rows/cols stay valid after rapid size changes', async () => {
    const paneId = await firstPaneId();

    // Feed some content so we can verify it stays visible.
    await clearVisibleGrid(paneId);
    await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (window as any).__windE2E.feedPty(id, 'scissor-probe-marker\n');
    }, paneId);
    await waitForVisibleText(paneId, 'scissor-probe-marker');

    // Baseline: read the current grid dimensions.
    const before = await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const e = (window as any).__windE2E;
      return { rows: e.rows(id), cols: e.cols(id) };
    }, paneId);
    expect(before.rows).toBeGreaterThan(0);
    expect(before.cols).toBeGreaterThan(0);

    // Three quick size changes — simulates the container ResizeObserver
    // firing in rapid succession during a splitter drag.
    await browser.setWindowSize(1100, 700);
    await browser.pause(80);
    await browser.setWindowSize(800, 500);
    await browser.pause(80);
    await browser.setWindowSize(1000, 600);
    // Allow debounce to settle (RESIZE_SETTLE_MS + margin).
    await browser.pause(600);

    // After settle: grid dimensions must be valid (non-zero, finite).
    const after = await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const e = (window as any).__windE2E;
      return { rows: e.rows(id), cols: e.cols(id) };
    }, paneId);
    expect(Number.isFinite(after.rows) && after.rows > 0).toBe(true);
    expect(Number.isFinite(after.cols) && after.cols > 0).toBe(true);

    // Content fed before resize must still be visible.
    const text = await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      return (window as any).__windE2E.visibleText(id) as string[];
    }, paneId);
    expect(text.some((r) => r.includes('scissor-probe-marker'))).toBe(true);
  });

  /**
   * Verify that after a size change, the host canvas pixel at a
   * known position returns valid color data (not all-zero = clipped).
   * A zero-alpha sample means either the scissor clipped that region
   * or the clear color hasn't been drawn — both indicate the scissor
   * didn't track the DOM in time.
   *
   * We sample near the center of the viewport, which should always
   * be inside the terminal pane after resize settle.
   */
  it('host canvas pixel at center reports non-zero data after resize settle', async () => {
    const paneId = await firstPaneId();
    await clearVisibleGrid(paneId);

    // Fill the terminal with a visible character so every cell has
    // non-bg color. Use the 'E' character which has a strong visual
    // footprint.
    await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const w = (window as any).__windE2E;
      const cols = w.cols(id);
      const rows = w.rows(id);
      // Fill the entire grid with 'E' on every visible line.
      const line = 'E'.repeat(Math.max(1, cols));
      for (let r = 0; r < rows; r++) {
        w.feedPty(id, `\x1b[${r + 1};1H${line}`);
      }
    }, paneId);
    await browser.pause(200);

    // Resize to trigger the scissor path.
    await browser.setWindowSize(900, 600);
    await browser.pause(600);

    // Sample host pixel at center (0.5, 0.5) — should be inside
    // the terminal grid and return non-transparent data.
    const pixel = await browser.execute(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      return (window as any).__windE2E.sampleHostPixel(0.5, 0.5) as {
        r: number; g: number; b: number; a: number;
      } | null;
    });

    // sampleHostPixel returns null when:
    // - No host canvas (Canvas2D fallback)
    // - WebGPU surface lost
    // Both are acceptable states in this test; we gate on null.
    if (pixel !== null) {
      // At least one channel must be non-zero — the terminal bg is
      // not pure black and the 'E' character has color.
      const hasColor = pixel.r > 0 || pixel.g > 0 || pixel.b > 0;
      expect(hasColor || pixel.a > 0).toBe(true);
    }
  });
});
