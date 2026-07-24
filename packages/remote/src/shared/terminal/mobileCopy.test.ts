import { describe, it, expect, vi } from 'vitest';
import { copySelectionOnly } from './mobileCopy';

describe('V-MOB-CP copySelectionOnly', () => {
  it('writes clipboard and clears selection without focus or paste', () => {
    const writeText = vi.fn();
    const clearSelection = vi.fn();
    const focusInput = vi.fn();
    const paste = vi.fn();
    const ok = copySelectionOnly('hello', {
      writeText,
      clearSelection,
      focusInput,
      paste,
    });
    expect(ok).toBe(true);
    expect(writeText).toHaveBeenCalledWith('hello');
    expect(clearSelection).toHaveBeenCalledOnce();
    expect(focusInput).not.toHaveBeenCalled();
    expect(paste).not.toHaveBeenCalled();
  });

  it('no-ops on empty selection', () => {
    const writeText = vi.fn();
    const clearSelection = vi.fn();
    expect(copySelectionOnly('', { writeText, clearSelection })).toBe(false);
    expect(writeText).not.toHaveBeenCalled();
  });
});
