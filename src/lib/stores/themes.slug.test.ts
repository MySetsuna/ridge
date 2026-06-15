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
});
