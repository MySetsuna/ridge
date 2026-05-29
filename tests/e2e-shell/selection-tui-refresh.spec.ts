/**
 * Regression guard — host selection MUST survive high-frequency TUI
 * redraws inside the wasm mirror.
 *
 * The bug this locks down: when the rust-parser backend is active
 * (Settings.parserBackend = 'rust', the default since P3.7), every PTY
 * output frame is shipped to the wasm consumer as a postcard-encoded
 * `DeltaFrame` and applied via `kernel.applyDeltaFrame(bytes)`. The
 * P3.6 implementation of that entry called `selection.clear()` on
 * EVERY applied frame — so Claude Code / htop / vim / less, which emit
 * ~30+ frames/s, erased the user's drag-select one frame after
 * pointerdown. Visible symptom: "selection flashes; can't copy text
 * from a refreshing TUI" — re-reported on 2026-05-21.
 *
 * The fix in `packages/ridge-term/src/lib.rs::apply_delta_frame` makes
 * the clear conditional on (a) scrollback eviction or (b) a hard
 * `Reset` delta — every other variant (Cells, Cursor, ModeChange,
 * non-evicting ScrollbackAppend, screen switch, semantic events) is now
 * selection-preserving, matching the §B.2 contract the `feed()` path
 * already follows.
 *
 * Spec strategy:
 *   1. Feed deterministic text via `feedPty` so the live grid has
 *      content we know the coordinates of.
 *   2. Set an absolute-row selection over "hello".
 *   3. Apply 30 synthesized no-op postcard delta frames through the
 *      REAL `manager.applyDeltaFrame` → `kernel.applyDeltaFrame` path
 *      (`encodeCursorDeltaFrame` hands out valid postcard bytes from
 *      wasm so the spec exercises the exact decode+apply pipeline the
 *      bug lives on).
 *   4. Assert `hasSelection()` and `getSelectionText()` are unchanged.
 *
 * Pre-fix: iteration 1 clears selection — every assertion below would
 * fail. Post-fix: 30 iterations leave selection byte-identical.
 */
// @ts-nocheck
import { browser, expect } from '@wdio/globals';
import { waitForAppReady, firstPaneId, clearVisibleGrid, waitForVisibleText } from './helpers';

describe('host selection survives delta-frame redraw storm (claude TUI regression)', () => {
  before(async () => {
    await waitForAppReady();
  });

  it('preserves a 5-char selection across 30 applyDeltaFrame calls', async () => {
    const paneId = await firstPaneId();

    // §1.35: wipe the visible grid to lock the `sbAbs = scrollbackLen()`
    // invariant. Without this the async PowerShell prompt can clobber
    // row 0 between feedPty and setSelectionAbs.
    await clearVisibleGrid(paneId);
    // Seed deterministic content into the live grid. The CR+LF puts
    // the cursor on a fresh row so anchor math below stays simple.
    await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (window as any).__windE2E.feedPty(id, 'hello world\n');
    }, paneId);
    await waitForVisibleText(paneId, 'hello world');

    // Compute the abs row of "hello" (live-grid row 0 is at
    // abs = scrollbackLen()). End-col is exclusive (selection range
    // convention), so cols [0, 5) covers 'h','e','l','l','o'.
    const setup = await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const e2e = (window as any).__windE2E;
      const sbAbs = e2e.scrollbackLen(id);
      e2e.setSelectionAbs(id, sbAbs, 0, sbAbs, 5);
      return {
        text: e2e.getSelectionText(id),
        has: e2e.hasSelection(id),
      };
    }, paneId);
    expect(setup.has).toBe(true);
    expect(setup.text).toContain('hello');

    // Slam 30 synthesized delta frames into the real apply pipeline.
    // Each frame is a single `Cursor` delta — Cells/Cursor/ModeChange
    // variants share the same invalidation gate, and Cursor is the
    // cheapest to encode without polluting the grid.
    const after = await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const e2e = (window as any).__windE2E;
      for (let i = 0; i < 30; i++) {
        // Alternate cursor target so any cursor-dedup short-circuit
        // doesn't bypass the apply path.
        const col = i % 2 === 0 ? 12 : 13;
        const bytes = e2e.encodeCursorDeltaFrame(id, i + 1, 0, col);
        if (!bytes) throw new Error('encodeCursorDeltaFrame returned null');
        e2e.applyDeltaFrameRaw(id, bytes);
      }
      return {
        text: e2e.getSelectionText(id),
        has: e2e.hasSelection(id),
      };
    }, paneId);

    expect(after.has).toBe(true);
    // Byte-identical selection content — proves abs-row anchors
    // weren't reset/clipped by the apply path.
    expect(after.text).toContain('hello');
    expect(after.text).toBe(setup.text);
  });

  it('feed()-path selection also survives a redraw storm (locks §B.2 too)', async () => {
    const paneId = await firstPaneId();

    // §1.35: wipe between specs so the prior `it`'s "hello world" +
    // cursor-home storm doesn't taint PROBE-XYZ's row indexing. After
    // the clear, PROBE-XYZ lands at visible row 0 — same convention
    // as it-1, so `sbAbs = scrollbackLen()` selects it directly.
    await clearVisibleGrid(paneId);
    await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (window as any).__windE2E.feedPty(id, 'PROBE-XYZ\n');
    }, paneId);
    await waitForVisibleText(paneId, 'PROBE-XYZ');

    // With the screen freshly cleared, PROBE-XYZ lands at visible row 0
    // — same anchoring convention as the first `it`. `sbAbs =
    // scrollbackLen()` selects the row containing PROBE-XYZ directly.
    const setup = await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const e2e = (window as any).__windE2E;
      const sbAbs = e2e.scrollbackLen(id);
      e2e.setSelectionAbs(id, sbAbs, 0, sbAbs, 9);
      return { text: e2e.getSelectionText(id) };
    }, paneId);
    expect(setup.text).toContain('PROBE-XYZ');

    // Repeated cursor-home escape sequence through the wasm feed()
    // path. §B.2 gates the selection-clear on scrollback eviction; a
    // pure cursor-motion redraw doesn't evict, so the anchor must
    // hold.
    const after = await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const e2e = (window as any).__windE2E;
      for (let i = 0; i < 30; i++) {
        e2e.feedPty(id, '\x1b[H');
      }
      return {
        text: e2e.getSelectionText(id),
        has: e2e.hasSelection(id),
      };
    }, paneId);

    expect(after.has).toBe(true);
    expect(after.text).toBe(setup.text);
  });
});
