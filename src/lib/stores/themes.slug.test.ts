import { describe, it, expect } from 'vitest';
import { constrainWallpaperSize, slugifyThemeId, WALLPAPER_MAX_EDGE, WALLPAPER_MAX_PIXELS } from './themes';

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

describe('constrainWallpaperSize', () => {
  it('bounds native-size wallpaper decode by edge and pixel count', () => {
    const size = constrainWallpaperSize(8000, 6000);
    expect(size.width).toBe(WALLPAPER_MAX_EDGE);
    expect(size.height).toBe(3072);
    expect(size.width * size.height).toBeLessThanOrEqual(WALLPAPER_MAX_PIXELS);
  });

  it('keeps normal images unchanged and rejects invalid dimensions', () => {
    expect(constrainWallpaperSize(1200, 800)).toEqual({ width: 1200, height: 800 });
    expect(constrainWallpaperSize(0, 800)).toEqual({ width: 0, height: 0 });
  });
});
