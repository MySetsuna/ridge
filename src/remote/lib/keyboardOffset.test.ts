import { describe, expect, it } from 'vitest';
import {
  resolveInputAnchor,
  stabilizeTerminalVisualShiftPx,
  terminalVisualShiftPx,
} from './keyboardOffset';

const phone = (overrides: Partial<Parameters<typeof terminalVisualShiftPx>[0]> = {}) => ({
  layoutHeightPx: 800,
  visualHeightPx: 500,
  visualOffsetTopPx: 0,
  stageTopPx: 100,
  cursorYPx: 500,
  cellHeightPx: 20,
  ...overrides,
});

describe('terminalVisualShiftPx', () => {
  it('returns zero with no keyboard or invalid geometry', () => {
    expect(terminalVisualShiftPx(phone({ visualHeightPx: 800 }))).toBe(0);
    expect(terminalVisualShiftPx(phone({ cellHeightPx: 0 }))).toBe(0);
  });

  it('does not move a cursor already above the keyboard', () => {
    expect(terminalVisualShiftPx(phone({ cursorYPx: 300 }))).toBe(0);
  });

  it('moves the cursor above the keyboard by one-cell breathing room', () => {
    // desired cursor bottom=480, actual=620 → -140.
    expect(terminalVisualShiftPx(phone())).toBe(-140);
  });

  it('bounds movement so context rows remain visible above the cursor', () => {
    expect(terminalVisualShiftPx(phone({
      visualHeightPx: 100,
      cursorYPx: 80,
      contextRows: 3,
    }))).toBe(-20);
  });

  it('includes visualViewport offsetTop and is deterministic', () => {
    const input = phone({ visualOffsetTopPx: 40 });
    expect(terminalVisualShiftPx(input)).toBe(-100);
    expect(terminalVisualShiftPx({ ...input })).toBe(terminalVisualShiftPx(input));
  });

  it('uses keyboard top and input rect with an explicit safe gap', () => {
    expect(terminalVisualShiftPx(phone({
      keyboardTopPx: 560,
      inputTopPx: 536,
      inputBottomPx: 560,
      safeGapPx: 12,
    }))).toBe(-12);
  });

  it('preserves prior shift through sub-cell viewport jitter', () => {
    expect(stabilizeTerminalVisualShiftPx(-101, -100, { hysteresisPx: 4 })).toBe(-100);
    expect(stabilizeTerminalVisualShiftPx(-106, -100, { hysteresisPx: 4 })).toBe(-106);
  });

  it('converges toward a large target by a finite step', () => {
    const options = { hysteresisPx: 0, maxStepPx: 40 };
    let shift = 0;
    shift = stabilizeTerminalVisualShiftPx(-130, shift, options);
    expect(shift).toBe(-40);
    shift = stabilizeTerminalVisualShiftPx(-130, shift, options);
    expect(shift).toBe(-80);
    shift = stabilizeTerminalVisualShiftPx(-130, shift, options);
    expect(shift).toBe(-120);
    shift = stabilizeTerminalVisualShiftPx(-130, shift, options);
    expect(shift).toBe(-130);
  });

  it('returns toward zero after keyboard closes instead of retaining stale offset', () => {
    expect(terminalVisualShiftPx(phone({
      visualHeightPx: 800,
      previousShiftPx: -100,
      maxStepPx: 30,
    }))).toBe(-70);
    expect(stabilizeTerminalVisualShiftPx(0, -4, { hysteresisPx: 12 })).toBe(0);
  });
});

describe('resolveInputAnchor', () => {
  const bounds = {
    containerLeft: 20,
    containerTop: 100,
    containerWidth: 300,
    containerHeight: 400,
    visualLeft: 0,
    visualTop: 180,
    visualWidth: 360,
    visualHeight: 500,
  };

  it('uses a valid terminal cursor exactly', () => {
    expect(resolveInputAnchor({ x: 24, y: 128, h: 18 }, bounds)).toEqual({
      x: 24,
      y: 128,
      h: 18,
    });
  });

  it('falls back to the visible terminal centre for missing or invalid cursors', () => {
    const expected = { x: 150, y: 240, h: 1 };
    expect(resolveInputAnchor(null, bounds)).toEqual(expected);
    expect(resolveInputAnchor({ x: -1, y: 10, h: 16 }, bounds)).toEqual(expected);
    expect(resolveInputAnchor({ x: 20, y: 999, h: 16 }, bounds)).toEqual(expected);
  });

  it('does not accept pointer coordinates as input', () => {
    const first = resolveInputAnchor(null, bounds);
    const second = resolveInputAnchor(null, { ...bounds });
    expect(second).toEqual(first);
  });
});
