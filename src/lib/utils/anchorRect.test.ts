import { describe, expect, it, beforeEach } from 'vitest';
import { popupStyleFor } from './anchorRect';

describe('popupStyleFor', () => {
  beforeEach(() => {
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: { innerWidth: 800, innerHeight: 600 },
    });
  });

  const anchor = (rect: Partial<DOMRect>) =>
    ({ getBoundingClientRect: () => ({ left: 40, right: 240, top: 100, bottom: 140, ...rect }) } as HTMLElement);

  it.each([
    ['bottom-end', 'position:fixed;top:144px;right:560px'],
    ['bottom-start', 'position:fixed;top:144px;left:40px'],
    ['top-end', 'position:fixed;bottom:504px;right:560px'],
    ['top-start', 'position:fixed;bottom:504px;left:40px'],
  ] as const)('computes %s placement', (placement, expected) => {
    expect(popupStyleFor(anchor({}), placement)).toBe(expected);
  });

  it('keeps popup edges at least eight pixels in the viewport', () => {
    expect(popupStyleFor(anchor({ left: 2, right: 798, top: 598, bottom: 599 }), 'bottom-end', 0)).toBe(
      'position:fixed;top:599px;right:8px',
    );
  });
});
