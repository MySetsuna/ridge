import { describe, expect, it } from 'vitest';
import { LinkSpanIndex } from './linkSpans';
import { decideHoverUnderline, decideLinkClick, underlineRegionsFromSpan } from './linkAffordance';
import { buildOpenPlanFromHit } from './linkOpenHost';

function fakeKernel(lines: string[], wrapped: Set<number> = new Set()) {
  return {
    dumpVisibleText: () => lines,
    rows: () => lines.length,
    cols: () => Math.max(...lines.map((line) => line.length), 1),
    rowWrapped: (row: number) => wrapped.has(row),
  };
}

describe('LinkSpanIndex', () => {
  it.each([
    ['https://example.com/a', 'https://example.com/a', 'url'],
    ['C:\\repo\\src\\main.ts:12:3', 'C:\\repo\\src\\main.ts:12:3', 'win-abs'],
    ['/home/u/repo/main.rs:9:2', '/home/u/repo/main.rs:9:2', 'posix-abs'],
    ['./src/main.ts:4', './src/main.ts:4', 'rel'],
    ['src/components', 'src/components', 'rel'],
    ['"C:\\My Project\\main.ts:7:2"', 'C:\\My Project\\main.ts:7:2', 'win-abs'],
    ["'../My Project/main.rs:8'", '../My Project/main.rs:8', 'rel'],
  ])('classifies %s without quote or location loss', (line, text, kind) => {
    const index = new LinkSpanIndex();
    const hit = index.hitTest(fakeKernel([line]), 0, Math.max(0, line.indexOf(text) + 1));
    expect(hit).toMatchObject({ text, kind });
  });

  it.each(['/word', '/help', 'value / count', '--output=/tmp'])('keeps ambiguous bare POSIX text inert: %s', (line) => {
    const index = new LinkSpanIndex();
    for (let col = 0; col < line.length; col += 1) {
      expect(index.hitTest(fakeKernel([line]), 0, col)).toBeNull();
    }
  });

  it('rebuilds viewport coordinates after content scrolls', () => {
    let lines = ['https://old.example/a', 'plain'];
    const kernel = {
      dumpVisibleText: () => lines,
      rows: () => lines.length,
      cols: () => 40,
      rowWrapped: () => false,
    };
    const index = new LinkSpanIndex();
    expect(index.hitTest(kernel, 0, 10)?.text).toContain('old.example');
    lines = ['plain', 'https://new.example/b'];
    index.markDirty();
    expect(index.hitTest(kernel, 0, 10)).toBeNull();
    expect(index.hitTest(kernel, 1, 10)?.text).toContain('new.example');
  });

  it('keeps the full URL on every visual segment after a soft wrap', () => {
    const lines = ['https://example.com/lo', 'ng/path'];
    const kernel = fakeKernel(lines, new Set([0]));
    const index = new LinkSpanIndex();

    const first = index.hitTest(kernel, 0, 10);
    const second = index.hitTest(kernel, 1, 2);

    expect(first?.text).toBe('https://example.com/long/path');
    expect(second?.text).toBe(first?.text);
    expect(second).toMatchObject({ row: 1, c0: 0, c1: 7 });
    expect(first && index.regionsForSpan(kernel, first)).toMatchObject([
      { row: 0 },
      { row: 1, c0: 0, c1: 7 },
    ]);
  });

  it('does not join a hard line break that merely looks continuous', () => {
    const lines = ['https://example.com/lo', 'ng/path'];
    const index = new LinkSpanIndex();
    const kernel = fakeKernel(lines);

    const first = index.hitTest(kernel, 0, 10);
    const second = index.hitTest(kernel, 1, 2);

    expect(first?.text).toBe('https://example.com/lo');
    expect(second?.text).toBe('ng/path');
    expect(second?.text).not.toContain(first?.text ?? '');
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

  it('joins when punctuation trimmed from the first row sits at the wrap edge', () => {
    const lines = ['https://example.com/foo,', 'bar'];
    const index = new LinkSpanIndex();
    const kernel = fakeKernel(lines, new Set([0]));

    const first = index.hitTest(kernel, 0, 10);
    const second = index.hitTest(kernel, 1, 1);

    expect(first?.text).toBe('https://example.com/foo,bar');
    expect(second?.text).toBe(first?.text);
    // The comma belongs to the URL target, but is intentionally excluded
    // from the first visual underline segment while the target is rebuilt.
    expect(first).toMatchObject({ row: 0, c1: lines[0]!.length - 1 });
    expect(second).toMatchObject({ row: 1, c0: 0, c1: 3 });
  });

  it('keeps the manager hover/click pipeline on the full wrapped target', () => {
    const lines = ['https://example.com/lo', 'ng/path'];
    const index = new LinkSpanIndex();
    const kernel = fakeKernel(lines, new Set([0]));
    const span = index.hitTest(kernel, 1, 2);
    expect(span).not.toBeNull();
    if (!span) return;

    expect(decideHoverUnderline({
      hasLinkHit: true,
      modifierHeld: true,
      spanText: span.text,
    })).toMatchObject({ showUnderline: true, cursor: 'pointer', spanText: span.text });
    expect(underlineRegionsFromSpan(span)).toEqual({ row: 1, c0: 0, c1: 7 });
    expect(decideLinkClick({
      mouseReportingOn: false,
      modifierHeld: true,
      hasLinkHit: true,
      primaryButton: true,
    }).openLink).toBe(true);
    expect(buildOpenPlanFromHit({ text: span.text, kind: span.kind })).toEqual({
      type: 'open_url',
      href: 'https://example.com/long/path',
    });
  });
});
