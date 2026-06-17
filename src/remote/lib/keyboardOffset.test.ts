import { describe, it, expect } from 'vitest';
import { keyboardShiftPx } from './keyboardOffset';

const GAP = 8;

describe('keyboardShiftPx (mobile soft-keyboard canvas shift)', () => {
  it('returns 0 when the keyboard is hidden (keyboardHeight = 0)', () => {
    // Arrange
    const input = {
      keyboardHeightPx: 0,
      gapBelowCanvasPx: 60,
      cursorFromCanvasBottomPx: 0,
      gapPx: GAP,
    };

    // Act
    const shift = keyboardShiftPx(input);

    // Assert
    expect(shift).toBe(0);
  });

  it('returns 0 for a negative keyboard height (degenerate viewport math)', () => {
    expect(
      keyboardShiftPx({
        keyboardHeightPx: -120,
        gapBelowCanvasPx: 0,
        cursorFromCanvasBottomPx: 0,
        gapPx: GAP,
      }),
    ).toBe(0);
  });

  it('lifts the cursor to gapPx above the keyboard when cursor sits at the canvas bottom', () => {
    // Arrange: keyboard 300px tall, canvas flush to the layout bottom (no bottom
    // bar), cursor on the last row.
    const input = {
      keyboardHeightPx: 300,
      gapBelowCanvasPx: 0,
      cursorFromCanvasBottomPx: 0,
      gapPx: GAP,
    };

    // Act
    const shift = keyboardShiftPx(input);

    // Assert: shift = kh + gap = 308.
    expect(shift).toBe(308);
  });

  it('subtracts the bottom-bar gap (fixes the old full-keyboard over-shift)', () => {
    // Arrange: a 56px bottom tab bar sits between the canvas and the keyboard.
    const withBar = keyboardShiftPx({
      keyboardHeightPx: 300,
      gapBelowCanvasPx: 56,
      cursorFromCanvasBottomPx: 0,
      gapPx: GAP,
    });
    const withoutBar = keyboardShiftPx({
      keyboardHeightPx: 300,
      gapBelowCanvasPx: 0,
      cursorFromCanvasBottomPx: 0,
      gapPx: GAP,
    });

    // Assert: the bar's height is removed from the shift, so the input row lands
    // just above the keyboard instead of being lifted a full bar-height too high.
    expect(withBar).toBe(withoutBar - 56);
    expect(withBar).toBe(252);
  });

  it('shifts less when the cursor is already above the canvas bottom', () => {
    // Arrange: cursor 120px up from the canvas bottom (e.g. a TUI status bar below).
    const shift = keyboardShiftPx({
      keyboardHeightPx: 300,
      gapBelowCanvasPx: 0,
      cursorFromCanvasBottomPx: 120,
      gapPx: GAP,
    });

    // Assert: kh + gap - 120 = 188.
    expect(shift).toBe(188);
  });

  it('clamps to 0 when the cursor already clears the keyboard (no negative shift)', () => {
    // Arrange: a tall bottom gap + high cursor already keep the input visible.
    const shift = keyboardShiftPx({
      keyboardHeightPx: 100,
      gapBelowCanvasPx: 200,
      cursorFromCanvasBottomPx: 50,
      gapPx: GAP,
    });

    // Assert: 100 + 8 - 200 - 50 < 0 → clamped.
    expect(shift).toBe(0);
  });

  it('is bounded by keyboardHeight + gap and never exceeds it', () => {
    // Regression guard for the "blank terminal" symptom: with non-negative gaps,
    // the shift can never exceed kh + gapPx, so the canvas can never be flung
    // entirely off-screen the way the old transform-coupled formula could.
    for (const kh of [120, 260, 360, 520]) {
      for (const gapBelow of [0, 40, 90]) {
        for (const cursorUp of [0, 30, 150]) {
          const shift = keyboardShiftPx({
            keyboardHeightPx: kh,
            gapBelowCanvasPx: gapBelow,
            cursorFromCanvasBottomPx: cursorUp,
            gapPx: GAP,
          });
          expect(shift).toBeGreaterThanOrEqual(0);
          expect(shift).toBeLessThanOrEqual(kh + GAP);
        }
      }
    }
  });

  it('is a pure function — identical inputs always yield identical output', () => {
    // The root-cause regression came from a hidden dependency on the live
    // (animating) transform. This pins that the result depends ONLY on its args.
    const input = {
      keyboardHeightPx: 333,
      gapBelowCanvasPx: 47,
      cursorFromCanvasBottomPx: 19,
      gapPx: GAP,
    };
    const first = keyboardShiftPx(input);
    const again = keyboardShiftPx({ ...input });
    expect(again).toBe(first);
  });
});
