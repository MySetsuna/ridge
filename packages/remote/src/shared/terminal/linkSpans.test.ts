import { describe, expect, it, vi } from 'vitest';
import { LinkSpanIndex, type LinkSpan } from './linkSpans';

const first: LinkSpan = { row: 0, c0: 5, c1: 24, kind: 'url', text: 'https://example.com/long/path', linkId: 'kernel-link' };
const second: LinkSpan = { ...first, row: 1, c0: 0, c1: 10 };

describe('ridge-term link adapter', () => {
  it('uses canonical cells and full targets without rescanning text in JavaScript', () => {
    const kernel = { linkSpans: vi.fn(() => [first, second]) };
    const index = new LinkSpanIndex();
    expect(index.hitTest(kernel, 0, 4)).toBeNull();
    expect(index.hitTest(kernel, 0, 5)).toBe(first);
    expect(index.hitTest(kernel, 1, 2)).toBe(second);
    expect(index.regionsForSpan(kernel, second)).toEqual([first, second]);
    expect(kernel.linkSpans).toHaveBeenCalledOnce();
  });

  it('invalidates cached spans on content, viewport or clear changes', () => {
    const kernel = { linkSpans: vi.fn(() => [first]) };
    const index = new LinkSpanIndex();
    expect(index.hitTest(kernel, 0, 5)).toBe(first);
    kernel.linkSpans.mockReturnValue([]);
    index.markDirty();
    expect(index.hitTest(kernel, 0, 5)).toBeNull();
    index.clear();
    expect(index.hitTest(kernel, 0, 5)).toBeNull();
    expect(kernel.linkSpans).toHaveBeenCalledTimes(3);
  });
});
