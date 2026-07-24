import { describe, expect, it } from 'vitest';
import {
  decideTouchMouseGesture,
  decideTouchScroll,
  MOUSE_ACTION_DRAG,
  MOUSE_ACTION_PRESS,
  MOUSE_ACTION_RELEASE,
  MOUSE_BTN_LEFT,
  MOUSE_BTN_RELEASE,
} from './mobileTouchScroll';

describe('decideTouchScroll', () => {
  it('routes to SGR wheel when mouse reporting is on', () => {
    expect(
      decideTouchScroll({ deltaY: -40, isMouseReporting: true, isAltScreen: false }),
    ).toEqual({ kind: 'mouse_wheel', btn: 64 });
    expect(
      decideTouchScroll({ deltaY: 40, isMouseReporting: true, isAltScreen: true }),
    ).toEqual({ kind: 'mouse_wheel', btn: 65 });
  });

  it('routes alt-screen without mouse to arrow presses (desktop wheelAltScroll parity)', () => {
    const up = decideTouchScroll({
      deltaY: -90,
      isMouseReporting: false,
      isAltScreen: true,
      pixelLike: true,
    });
    expect(up).toEqual({ kind: 'alt_arrows', key: 'ArrowUp', presses: 3 });

    const down = decideTouchScroll({
      deltaY: 30,
      isMouseReporting: false,
      isAltScreen: true,
      pixelLike: true,
    });
    expect(down).toEqual({ kind: 'alt_arrows', key: 'ArrowDown', presses: 1 });
  });

  it('routes plain shell to local scrollback lines', () => {
    expect(
      decideTouchScroll({ deltaY: 50, isMouseReporting: false, isAltScreen: false }),
    ).toEqual({ kind: 'local_scroll', lines: 3 });
    expect(
      decideTouchScroll({ deltaY: -50, isMouseReporting: false, isAltScreen: false }),
    ).toEqual({ kind: 'local_scroll', lines: -3 });
  });

  it('returns null for zero delta', () => {
    expect(
      decideTouchScroll({ deltaY: 0, isMouseReporting: false, isAltScreen: true }),
    ).toBeNull();
  });
});

describe('decideTouchMouseGesture', () => {
  it('matches desktop manager press/drag/release encoding', () => {
    expect(decideTouchMouseGesture('press')).toEqual({
      button: MOUSE_BTN_LEFT,
      action: MOUSE_ACTION_PRESS,
    });
    expect(decideTouchMouseGesture('drag')).toEqual({
      button: MOUSE_BTN_LEFT,
      action: MOUSE_ACTION_DRAG,
    });
    // Was previously left-btn release on mobile — TUIs expect btn=3 release.
    expect(decideTouchMouseGesture('release')).toEqual({
      button: MOUSE_BTN_RELEASE,
      action: MOUSE_ACTION_RELEASE,
    });
  });
});
