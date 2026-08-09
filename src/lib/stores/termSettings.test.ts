import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

describe('terminal font size', () => {
  let values = new Map<string, string>();
  let termFontSize: typeof import('./termSettings').termFontSize;
  let setTermFontSize: typeof import('./termSettings').setTermFontSize;

  beforeAll(async () => {
    const storage = {
      getItem: vi.fn((key: string) => values.get(key) ?? null),
      setItem: vi.fn((key: string, value: string) => values.set(key, value)),
    };
    Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: storage });
    ({ termFontSize, setTermFontSize } = await import('./termSettings'));
  });

  beforeEach(() => {
    values = new Map();
    termFontSize.reset();
  });

  it('clamps, rounds, and persists direct values', async () => {
    setTermFontSize(99.4);
    expect(await import('svelte/store').then(({ get }) => get(termFontSize))).toBe(32);
    expect(values.get('ridge-term-font-size')).toBe('32');
    setTermFontSize(Number.NaN);
    expect(await import('svelte/store').then(({ get }) => get(termFontSize))).toBe(15);
  });

  it('keeps increase and decrease inside bounds', async () => {
    setTermFontSize(8);
    termFontSize.decrease();
    expect(await import('svelte/store').then(({ get }) => get(termFontSize))).toBe(8);
    setTermFontSize(32);
    termFontSize.increase();
    expect(await import('svelte/store').then(({ get }) => get(termFontSize))).toBe(32);
  });
});
