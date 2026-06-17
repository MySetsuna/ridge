import { describe, it, expect } from 'vitest';
import { slugifyThemeId } from './themes';

describe('slugifyThemeId', () => {
  it('lowercases and dashes non-alnum, adds custom- prefix', () => {
    expect(slugifyThemeId('My Theme!!')).toBe('custom-my-theme');
  });
  it('keeps CJK', () => {
    expect(slugifyThemeId('全新主题')).toBe('custom-全新主题');
  });
  it('falls back to theme on empty', () => {
    expect(slugifyThemeId('   ')).toBe('custom-theme');
  });
  it('collapses consecutive separators', () => {
    expect(slugifyThemeId('My  Theme')).toBe('custom-my-theme');
  });
  it('strips leading/trailing separators', () => {
    expect(slugifyThemeId('!!Fire!!')).toBe('custom-fire');
  });
  it('handles CJK + ASCII mix', () => {
    expect(slugifyThemeId('Dark 暗黑')).toBe('custom-dark-暗黑');
  });
});
