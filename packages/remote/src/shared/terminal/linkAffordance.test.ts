import { describe, expect, it } from 'vitest';
import {
  decideHoverUnderline,
  decideLinkClick,
  osc8UnderlineRegions,
  parsePathWithLocation,
  resolveOpenTarget,
  resolvePathAgainstCwd,
  underlineRegionsFromSpan,
} from './linkAffordance';

describe('decideHoverUnderline', () => {
	it('shows platform modifier guidance and activates affordance only while held', () => {
    expect(
      decideHoverUnderline({ hasLinkHit: true, modifierHeld: true, spanText: 'https://x' }),
    ).toEqual({
		showUnderline: true,
		showHint: true,
		hintText: 'Ctrl+点击打开',
		cursor: 'pointer',
      spanText: 'https://x',
    });
    expect(decideHoverUnderline({ hasLinkHit: true, modifierHeld: false })).toEqual({
		showUnderline: false,
		showHint: true,
		hintText: 'Ctrl+点击打开',
		cursor: '',
      spanText: null,
    });
		expect(decideHoverUnderline({ hasLinkHit: true, modifierHeld: false, isMac: true }).hintText)
			.toBe('⌘+点击打开');
    expect(decideHoverUnderline({ hasLinkHit: false, modifierHeld: true })).toEqual({
		showUnderline: false,
		showHint: false,
		hintText: null,
		cursor: '',
      spanText: null,
    });
  });
});

describe('decideLinkClick', () => {
  it('TUI mouse on: plain click forwards and modifier click opens', () => {
    expect(
      decideLinkClick({
        mouseReportingOn: true,
        modifierHeld: false,
        hasLinkHit: true,
        primaryButton: true,
      }),
    ).toEqual({
			forwardToProgram: true,
			openLink: false,
			startHostSelection: false,
    });
    expect(
      decideLinkClick({
        mouseReportingOn: true,
        modifierHeld: true,
        hasLinkHit: true,
        primaryButton: true,
      }),
    ).toEqual({
      forwardToProgram: false,
      openLink: true,
      startHostSelection: false,
    });
  });

  it('TUI mouse off: only modifier link opens; plain link click selects', () => {
    expect(
      decideLinkClick({
        mouseReportingOn: false,
        modifierHeld: true,
        hasLinkHit: true,
        primaryButton: true,
      }),
    ).toEqual({
      forwardToProgram: false,
      openLink: true,
      startHostSelection: false,
    });
    expect(
      decideLinkClick({
        mouseReportingOn: false,
        modifierHeld: false,
        hasLinkHit: true,
        primaryButton: true,
      }),
    ).toEqual({
      forwardToProgram: false,
      openLink: false,
      startHostSelection: true,
    });
    expect(
      decideLinkClick({
        mouseReportingOn: false,
        modifierHeld: false,
        hasLinkHit: false,
        primaryButton: true,
      }),
    ).toEqual({
      forwardToProgram: false,
      openLink: false,
      startHostSelection: true,
    });
  });
});

describe('parsePathWithLocation / resolveOpenTarget', () => {
  it('parses file:line:col', () => {
    expect(parsePathWithLocation('src/main.rs:12:3')).toEqual({
      path: 'src/main.rs',
      line: 12,
      col: 3,
    });
    expect(parsePathWithLocation('C:\\code\\a.rs:9')).toEqual({
      path: 'C:\\code\\a.rs',
      line: 9,
      col: undefined,
    });
  });

  it('does not split http URLs', () => {
    expect(resolveOpenTarget('https://ex.com/a:1', 'url')).toEqual({
      kind: 'url',
      href: 'https://ex.com/a:1',
    });
  });

  it('path kinds become path targets', () => {
    expect(resolveOpenTarget('foo/bar.ts:2', 'rel')).toEqual({
      kind: 'path',
      path: 'foo/bar.ts',
      line: 2,
      col: undefined,
    });
  });
});

describe('resolvePathAgainstCwd', () => {
  it('joins relative to pane cwd', () => {
    expect(resolvePathAgainstCwd('./src/a.ts', '/home/u/proj', '/ws')).toBe(
      '/home/u/proj/src/a.ts',
    );
    expect(resolvePathAgainstCwd('src\\a.ts', 'C:\\ws\\app', null)).toBe(
      'C:\\ws\\app\\src\\a.ts',
    );
    expect(resolvePathAgainstCwd('../shared/a.ts', '/home/u/repo/docs', '/home/u/repo')).toBe(
      '/home/u/repo/shared/a.ts',
    );
  });

  it('keeps absolutes', () => {
    expect(resolvePathAgainstCwd('/abs/x', '/cwd', '/ws')).toBe('/abs/x');
    expect(resolvePathAgainstCwd('D:\\x\\y', 'C:\\c', null)).toBe('D:\\x\\y');
  });
});

describe('underlineRegionsFromSpan', () => {
  it('copies row range', () => {
    expect(underlineRegionsFromSpan({ row: 2, c0: 1, c1: 8 })).toEqual({
      row: 2,
      c0: 1,
      c1: 8,
    });
  });
});

describe('osc8UnderlineRegions', () => {
  function grid(lines: string[], wrapped: Set<number>) {
    const width = Math.max(...lines.map((line) => line.length), 1);
    return {
      rows: () => lines.length,
      cols: () => width,
      rowWrapped: (row: number) => wrapped.has(row),
      hyperlinkAt: (row: number, col: number) => {
        const ch = lines[row]?.[col];
        return ch && ch !== ' ' ? { uri: 'https://example.test/long' } : null;
      },
    };
  }

  it('joins OSC-8 segments across a verified soft wrap', () => {
    expect(osc8UnderlineRegions(grid(['https://example.test/', 'long'], new Set([0])), 1, 2, 'https://example.test/long')).toEqual([
      { row: 0, c0: 0, c1: 21 },
      { row: 1, c0: 0, c1: 4 },
    ]);
  });

  it('does not join repeated URI on a hard line break', () => {
    expect(osc8UnderlineRegions(grid(['same', 'same'], new Set()), 1, 1, 'https://example.test/long')).toEqual([
      { row: 1, c0: 0, c1: 4 },
    ]);
  });

  it('never opens a non-primary click', () => {
    expect(decideLinkClick({
      mouseReportingOn: false,
      modifierHeld: true,
      hasLinkHit: true,
      primaryButton: false,
    })).toEqual({
      forwardToProgram: false,
      openLink: false,
      startHostSelection: false,
    });
  });
});
