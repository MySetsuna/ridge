import { afterEach, describe, expect, it, vi } from 'vitest';
import { secureRandomUnit } from './random';

describe('secureRandomUnit', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('uses Web Crypto when available', () => {
    vi.stubGlobal('crypto', {
      getRandomValues(values: Uint32Array) {
        values[0] = 0x80000000;
        return values;
      },
    });
    expect(secureRandomUnit()).toBe(0.5);
  });

  it('fails closed to a bounded midpoint when crypto is unavailable', () => {
    vi.stubGlobal('crypto', undefined);
    expect(secureRandomUnit()).toBe(0.5);
  });

  it('fails closed when Web Crypto throws', () => {
    vi.stubGlobal('crypto', {
      getRandomValues() {
        throw new Error('crypto unavailable');
      },
    });
    expect(secureRandomUnit()).toBe(0.5);
  });
});
