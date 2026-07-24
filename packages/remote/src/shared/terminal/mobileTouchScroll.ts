/**
 * Pure decision helpers for mobile remote touch → mouse / TUI / scrollback.
 *
 * Desktop `TerminalManager.handleWheel` + `wheelAltScroll` already cover:
 *   1) DEC mouse reporting → SGR wheel (btn 64/65)
 *   2) alt-screen without mouse → arrow keys (pager / claude menus)
 *   3) else host scrollback
 *
 * Mobile `TerminalCanvas.touchWheel` historically only did (1) and (3), so
 * full-screen TUIs without mouse reporting could not scroll via swipe.
 * Keep the branch tree here so both paths and unit tests share one SSOT.
 */

export type TouchScrollDecision =
  | { kind: 'mouse_wheel'; btn: 64 | 65 }
  | { kind: 'alt_arrows'; key: 'ArrowUp' | 'ArrowDown'; presses: number }
  | { kind: 'local_scroll'; lines: number };

/** SGR release button used by desktop manager (btn=3, action=1). */
export const MOUSE_BTN_RELEASE = 3;
export const MOUSE_BTN_LEFT = 0;
export const MOUSE_ACTION_PRESS = 0;
export const MOUSE_ACTION_RELEASE = 1;
export const MOUSE_ACTION_DRAG = 2;

const WHEEL_LINES_DIVISOR = 30;
const MAX_PRESSES_PER_EVENT = 5;
/** Fixed line steps for touch pixel accumulation (~one finger flick chunk). */
const TOUCH_LOCAL_LINES = 3;

/**
 * Decide how a vertical swipe / wheel delta should be applied.
 * `deltaY > 0` = finger/content moving up (scroll down / next page).
 */
export function decideTouchScroll(input: {
  deltaY: number;
  isMouseReporting: boolean;
  isAltScreen: boolean;
  /** When true, treat deltaY as pixel-like (touch accum); false = already coarse. */
  pixelLike?: boolean;
}): TouchScrollDecision | null {
  const { deltaY, isMouseReporting, isAltScreen } = input;
  if (deltaY === 0 || !Number.isFinite(deltaY)) return null;

  if (isMouseReporting) {
    return { kind: 'mouse_wheel', btn: deltaY < 0 ? 64 : 65 };
  }

  if (isAltScreen) {
    const magnitude = input.pixelLike
      ? Math.abs(deltaY) / WHEEL_LINES_DIVISOR
      : Math.abs(deltaY);
    const presses = Math.max(1, Math.min(MAX_PRESSES_PER_EVENT, Math.round(magnitude) || 1));
    return {
      kind: 'alt_arrows',
      key: deltaY < 0 ? 'ArrowUp' : 'ArrowDown',
      presses,
    };
  }

  // Local scrollback: positive deltaY → scrollDown
  const lines = deltaY > 0 ? TOUCH_LOCAL_LINES : -TOUCH_LOCAL_LINES;
  return { kind: 'local_scroll', lines };
}

/** Encode-ready mouse press/drag/release for selection-as-mouse mode. */
export function decideTouchMouseGesture(
  phase: 'press' | 'drag' | 'release',
): { button: number; action: number } {
  switch (phase) {
    case 'press':
      return { button: MOUSE_BTN_LEFT, action: MOUSE_ACTION_PRESS };
    case 'drag':
      return { button: MOUSE_BTN_LEFT, action: MOUSE_ACTION_DRAG };
    case 'release':
      // Parity with desktop manager pointerup: btn=3 release, not left-btn release.
      return { button: MOUSE_BTN_RELEASE, action: MOUSE_ACTION_RELEASE };
  }
}
