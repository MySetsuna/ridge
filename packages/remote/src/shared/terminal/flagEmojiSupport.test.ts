import { describe, it, expect } from 'vitest';
import {
  probeSystemFlagSupport,
  readFlagCache,
  writeFlagCache,
} from './flagEmojiSupport';

describe('probeSystemFlagSupport', () => {
  // measure() is the only browser dependency; here it's mocked. A single
  // Regional Indicator '🇯' has String#length 2; the pair '🇯🇵' has length 4.
  it('merged flag glyph (pair ≈ single width) → supported', () => {
    const measure = (t: string) => (t.length > 2 ? 11 : 10);
    expect(probeSystemFlagSupport(measure)).toBe(true);
  });

  it('two letter glyphs (pair ≈ 2× single width) → not supported', () => {
    const measure = (t: string) => (t.length > 2 ? 20 : 10);
    expect(probeSystemFlagSupport(measure)).toBe(false);
  });

  it('unmeasurable (0 width) → assume supported (inject nothing)', () => {
    expect(probeSystemFlagSupport(() => 0)).toBe(true);
  });

  it('pair width 0 but single valid → assume supported', () => {
    const measure = (t: string) => (t.includes('\u{1F1F5}') ? 0 : 10);
    expect(probeSystemFlagSupport(measure)).toBe(true);
  });
});

describe('flag-support cache', () => {
  it('round-trips a verdict for the same UA fingerprint', () => {
    const raw = writeFlagCache(false, 'UA-1');
    expect(readFlagCache(raw, 'UA-1')).toBe(false);
  });

  it('invalidates when the UA fingerprint changes', () => {
    const raw = writeFlagCache(true, 'UA-1');
    expect(readFlagCache(raw, 'UA-2')).toBeNull();
  });

  it('returns null on empty / corrupt input', () => {
    expect(readFlagCache(null, 'UA')).toBeNull();
    expect(readFlagCache('{not json', 'UA')).toBeNull();
  });

  it('partial JSON missing the supported field → null', () => {
    expect(readFlagCache('{"ua":"UA"}', 'UA')).toBeNull();
  });
});
