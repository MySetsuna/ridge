import { describe, expect, it } from 'vitest';
import { terminalVisualShiftPx } from './keyboardOffset';

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
});
