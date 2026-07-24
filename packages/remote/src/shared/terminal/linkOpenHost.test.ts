import { describe, expect, it } from 'vitest';
import {
  buildOpenPlanFromHit,
  decodeUnderlineDataset,
  encodeUnderlineDataset,
  isPathSpanKind,
  isSafeHttpUrl,
  looksOutsideWorkspace,
  planHostOpen,
  underlineCssTokens,
} from './linkOpenHost';

describe('linkOpenHost (C51)', () => {
  it('opens safe http urls', () => {
    const a = planHostOpen('https://example.com/x', 'url');
    expect(a).toEqual({ type: 'open_url', href: 'https://example.com/x' });
    expect(isSafeHttpUrl('javascript:alert(1)')).toBe(false);
  });

  it('resolves relative path against cwd (rel kind)', () => {
    const a = planHostOpen('src/main.rs:10:2', 'rel', {
      paneCwd: 'C:\\code\\wind',
      workspaceRoot: 'C:\\code\\wind',
    });
    expect(a.type).toBe('open_file');
    if (a.type === 'open_file') {
      expect(a.path).toMatch(/main\.rs/);
      expect(a.line).toBe(10);
      expect(a.col).toBe(2);
    }
  });

  it('resolves win-abs / posix-abs / home path kinds', () => {
    const win = planHostOpen('C:\\code\\wind\\src\\x.rs', 'win-abs');
    expect(win.type).toBe('open_file');
    const posix = planHostOpen('/home/u/proj/a.ts', 'posix-abs');
    expect(posix.type).toBe('open_file');
    const home = planHostOpen('~/proj/a.ts', 'home');
    expect(home.type).toBe('open_file');
  });

  it('rejects empty path', () => {
    const a = planHostOpen('', 'rel');
    expect(a.type).toBe('noop');
  });

  it('underline dataset roundtrip', () => {
    const enc = encodeUnderlineDataset(3, 1, 8);
    expect(decodeUnderlineDataset(enc)).toEqual({ row: 3, c0: 1, c1: 8 });
    const osc = encodeUnderlineDataset(2, 'osc8');
    expect(decodeUnderlineDataset(osc)).toEqual({ row: 2, osc8: true });
  });

  it('css tokens cover granular path kinds', () => {
    expect(underlineCssTokens({ show: false, kind: 'url' })).toEqual([]);
    expect(underlineCssTokens({ show: true, kind: 'rel' })).toContain('ridge-link-path');
    expect(underlineCssTokens({ show: true, kind: 'win-abs' })).toContain('ridge-link-path');
    expect(underlineCssTokens({ show: true, kind: 'posix-abs' })).toContain('ridge-link-path');
    expect(underlineCssTokens({ show: true, kind: 'home' })).toContain('ridge-link-path');
    expect(underlineCssTokens({ show: true, kind: 'file-url' })).toContain('ridge-link-file');
    expect(isPathSpanKind('rel')).toBe(true);
    expect(isPathSpanKind('url')).toBe(false);
  });

  it('workspace outside check', () => {
    expect(looksOutsideWorkspace('C:/other/x', 'C:/code/wind')).toBe(true);
    expect(looksOutsideWorkspace('C:/code/wind/src', 'C:/code/wind')).toBe(false);
  });

  it('build from hit', () => {
    const p = buildOpenPlanFromHit({
      text: 'https://x.ai',
      kind: 'osc8',
    });
    expect(p.type).toBe('open_url');
  });
});
