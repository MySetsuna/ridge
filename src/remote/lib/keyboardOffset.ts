// Pure geometry for the mobile soft-keyboard canvas shift.
//
// Extracted from TerminalCanvas.svelte so the math is unit-testable without a DOM
// and so the invariant that broke production is pinned by a regression test:
// **every input is transform-independent**. The earlier in-component version
// derived the shift from `getBoundingClientRect().top` and then "undid" the
// currently-applied `translateY(-keyboardOffset)` with the *target* offset. During
// the keyboard slide-in (which fires many `visualViewport` resize events while the
// `.container` transform is still animating over .2s), reading the in-flight
// transform but undoing the target one made the offset spiral — flinging the
// canvas off-screen (blank terminal) and thrashing the page (apparent freeze).
//
// Keeping the formula a function of intrinsic geometry only (keyboard height +
// canvas/cursor sizes + a gap measured while the keyboard is hidden) makes it
// idempotent: recomputing per resize converges to the same value regardless of
// how far the CSS transition has animated, so there is no feedback at all.

export interface KeyboardShiftInput {
  /** Soft-keyboard height in CSS px: `innerHeight - visualViewport.height`,
   *  clamped to >= 0. Zero (or less) means the keyboard is hidden. */
  keyboardHeightPx: number;
  /** Gap in CSS px between the canvas's BOTTOM edge and the layout-viewport
   *  bottom (the bottom tab bar + safe-area inset). Measured while the keyboard
   *  is hidden — i.e. with no transform applied — so it is a stable constant. */
  gapBelowCanvasPx: number;
  /** Distance in CSS px from the cursor cell's bottom to the canvas's bottom edge
   *  (`canvasHeight - (cursor.y + cursor.h)`). Intrinsic to the canvas grid, so a
   *  `translateY` on the canvas cannot change it. */
  cursorFromCanvasBottomPx: number;
  /** Breathing room in CSS px so the input row isn't flush with the keyboard. */
  gapPx: number;
}

/**
 * Vertical shift (CSS px, >= 0) to translate the terminal canvas UP so the cursor
 * row sits exactly `gapPx` above the keyboard's top edge.
 *
 * Derivation (all distances from the layout-viewport bottom, keyboard hidden):
 *   cursor bottom    = gapBelowCanvas + cursorFromCanvasBottom
 *   keyboard top     = keyboardHeight
 *   desired cursor   = keyboardHeight + gap
 *   shift            = desiredCursor - cursorBottom
 *                    = keyboardHeight + gap - gapBelowCanvas - cursorFromCanvasBottom
 *
 * Returns 0 when the keyboard is hidden or when the cursor already clears it.
 */
export function keyboardShiftPx({
  keyboardHeightPx,
  gapBelowCanvasPx,
  cursorFromCanvasBottomPx,
  gapPx,
}: KeyboardShiftInput): number {
  if (keyboardHeightPx <= 0) return 0; // keyboard hidden → no shift
  return Math.max(
    0,
    Math.round(keyboardHeightPx + gapPx - gapBelowCanvasPx - cursorFromCanvasBottomPx),
  );
}
