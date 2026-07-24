import { describe, it, expect } from 'vitest';
import { nextImageVersion, imageUrlWithVersion } from './imagePreviewVersion';

describe('V-B3 image preview version', () => {
  it('bumps from undefined to 1', () => {
    expect(nextImageVersion(undefined)).toBe(1);
  });

  it('increments existing version', () => {
    expect(nextImageVersion(3)).toBe(4);
  });

  it('appends cache buster', () => {
    expect(imageUrlWithVersion('asset://x.png', 2)).toBe('asset://x.png?v=2');
    expect(imageUrlWithVersion('asset://x.png?q=1', 2)).toBe('asset://x.png?q=1&v=2');
    expect(imageUrlWithVersion('asset://x.png', 0)).toBe('asset://x.png');
  });
});
