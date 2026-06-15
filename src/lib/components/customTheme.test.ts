import { describe, it, expect } from 'vitest';
import { CORE_COLOR_KEYS, ANSI_COLOR_KEYS, ALPHA_COLOR_KEYS, previewStyle, buildThemeEntry } from './customTheme';

describe('customTheme constants', () => {
  it('has 18 core keys incl. accent and term-bg', () => {
    expect(CORE_COLOR_KEYS).toHaveLength(18);
    expect(CORE_COLOR_KEYS).toContain('accent');
    expect(CORE_COLOR_KEYS).toContain('term-bg');
  });
  it('has 16 ansi keys', () => {
    expect(ANSI_COLOR_KEYS).toHaveLength(16);
    expect(ANSI_COLOR_KEYS).toContain('ansi-brightWhite');
  });
  it('marks rgba-style keys as alpha-bearing', () => {
    expect(ALPHA_COLOR_KEYS).toContain('glass');
    expect(ALPHA_COLOR_KEYS).not.toContain('bg');
  });
});

describe('previewStyle', () => {
  it('emits scoped --rg- vars from colors map', () => {
    const s = previewStyle({ bg: '#000', accent: '#fff' });
    expect(s).toContain('--rg-bg: #000;');
    expect(s).toContain('--rg-accent: #fff;');
  });
});

describe('buildThemeEntry', () => {
  it('assembles a custom ThemeEntry from form state', () => {
    const e = buildThemeEntry({
      id: '', label: 'My', type: 'dark',
      colors: { bg: '#000' }, loaderPrimary: '#aaa', loaderSecondary: '#bbb',
      bgImage: 'x.png', bgImageOpacity: 0.5,
    });
    expect(e.label).toBe('My');
    expect(e.colors.bg).toBe('#000');
    expect(e.loader.primary).toBe('#aaa');
    expect(e.bgImage).toBe('x.png');
    expect(e.bgImageOpacity).toBe(0.5);
  });
  it('omits bgImage fields when no image', () => {
    const e = buildThemeEntry({
      id: 'custom-x', label: 'X', type: 'light',
      colors: {}, loaderPrimary: '#1', loaderSecondary: '#2',
      bgImage: undefined, bgImageOpacity: 0.3,
    });
    expect(e.bgImage).toBeUndefined();
    expect('bgImageOpacity' in e).toBe(false);
  });
});
