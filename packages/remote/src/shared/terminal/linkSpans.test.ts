import { describe, expect, it } from 'vitest';
import { LinkSpanIndex } from './linkSpans';

function fakeKernel(lines: string[], wrapped: Set<number> = new Set()) {
  return {
    dumpVisibleText: () => lines,
    rows: () => lines.length,
    cols: () => Math.max(...lines.map((line) => line.length), 1),
    rowWrapped: (row: number) => wrapped.has(row),
  };
}

describe('LinkSpanIndex', () => {
  it('keeps the full URL on every visual segment after a soft wrap', () => {
    const lines = ['https://example.com/lo', 'ng/path'];
    const kernel = fakeKernel(lines, new Set([0]));
    const index = new LinkSpanIndex();

    const first = index.hitTest(kernel, 0, 10);
    const second = index.hitTest(kernel, 1, 2);

    expect(first?.text).toBe('https://example.com/long/path');
    expect(second?.text).toBe(first?.text);
    expect(second).toMatchObject({ row: 1, c0: 0, c1: 7 });
  });

  it('does not join a hard line break that merely looks continuous', () => {
    const lines = ['https://example.com/lo', 'ng/path'];
    const index = new LinkSpanIndex();
    const kernel = fakeKernel(lines);

    const first = index.hitTest(kernel, 0, 10);
    const second = index.hitTest(kernel, 1, 2);

    expect(first?.text).toBe('https://example.com/lo');
    expect(second).toBeNull();
  });

  it('uses the full-row fallback for older kernels without rowWrapped()', () => {
    const lines = ['https://example.com/lo', 'ng/path'];
    const index = new LinkSpanIndex();
    const kernel = {
      dumpVisibleText: () => lines,
      rows: () => lines.length,
      cols: () => lines[0]!.length,
    };

    expect(index.hitTest(kernel, 1, 2)?.text).toBe('https://example.com/long/path');
  });
});
