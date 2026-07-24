import { describe, expect, it } from 'vitest';
import {
  decideHoverUnderline,
  decideLinkClick,
  parsePathWithLocation,
  resolveOpenTarget,
  resolvePathAgainstCwd,
  underlineRegionsFromSpan,
} from './linkAffordance';

describe('decideHoverUnderline', () => {
  it('shows underline only when modifier + hit', () => {
    expect(
      decideHoverUnderline({ hasLinkHit: true, modifierHeld: true, spanText: 'https://x' }),
    ).toEqual({
      showUnderline: true,
      cursor: 'pointer',
      spanText: 'https://x',
    });
    expect(decideHoverUnderline({ hasLinkHit: true, modifierHeld: false })).toEqual({
      showUnderline: false,
      cursor: '',
      spanText: null,
    });
    expect(decideHoverUnderline({ hasLinkHit: false, modifierHeld: true })).toEqual({
      showUnderline: false,
      cursor: '',
      spanText: null,
    });
  });
});

describe('decideLinkClick', () => {
  it('TUI mouse on: bare click forwards to program, no open', () => {
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
  });

  it('TUI mouse on: Ctrl+click opens link, no program', () => {
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

  it('TUI mouse off: Ctrl+click opens; bare click starts selection', () => {
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
