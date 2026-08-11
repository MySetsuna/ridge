import { afterEach, describe, it, expect, vi } from 'vitest';
import {
  probeSystemFlagSupport,
  readFlagCache,
  writeFlagCache,
} from './flagEmojiSupport';

afterEach(() => {
  vi.unstubAllGlobals();
});

function installDom(options: {
  cached?: string | null;
  measure?: (text: string) => number;
  readStorage?: () => string | null;
  writeStorage?: (key: string, value: string) => void;
} = {}) {
  const styles: { id: string; textContent: string | null }[] = [];
  const canvas = {
    setAttribute: vi.fn(),
    getContext: vi.fn(() => ({
      font: '',
      measureText: (text: string) => ({ width: options.measure?.(text) ?? 10 }),
    })),
    remove: vi.fn(),
  };
  const documentStub = {
    createElement: vi.fn((tag: string) => tag === 'canvas' ? canvas : { id: '', textContent: '' }),
    getElementById: vi.fn((id: string) => styles.find((style) => style.id === id) ?? null),
    body: { appendChild: vi.fn() },
    head: {
      appendChild: vi.fn((style: { id: string; textContent: string | null }) => styles.push(style)),
    },
  };
  const storage = {
    getItem: vi.fn(options.readStorage ?? (() => options.cached ?? null)),
    setItem: vi.fn(options.writeStorage ?? (() => {})),
  };
  vi.stubGlobal('document', documentStub);
  vi.stubGlobal('navigator', { userAgent: 'UA-test' });
  vi.stubGlobal('localStorage', storage);
  return { canvas, documentStub, storage, styles };
}

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

describe('ensureFlagFont', () => {
  it('is a no-op outside a DOM context', async () => {
    expect((await import('./flagEmojiSupport')).ensureFlagFont()).toBe(false);
  });

  it('uses a cached native-support verdict without probing', async () => {
    const { documentStub } = installDom({ cached: writeFlagCache(true, 'UA-test') });
    const { ensureFlagFont } = await import('./flagEmojiSupport');

    expect(ensureFlagFont()).toBe(false);
    expect(documentStub.createElement).not.toHaveBeenCalled();
  });

  it('injects the fallback face once for a cached unsupported verdict', async () => {
    const { documentStub, styles } = installDom({ cached: writeFlagCache(false, 'UA-test') });
    const { ensureFlagFont } = await import('./flagEmojiSupport');

    expect(ensureFlagFont()).toBe(true);
    expect(ensureFlagFont()).toBe(true);
    expect(styles).toHaveLength(1);
    expect(styles[0].textContent).toContain("font-family:'Flag Emoji'");
    expect(documentStub.head.appendChild).toHaveBeenCalledTimes(1);
  });

  it('probes, caches, and injects when the verdict is absent', async () => {
    const { canvas, storage, styles } = installDom({
      measure: (text) => text.length > 2 ? 20 : 10,
    });
    const { ensureFlagFont } = await import('./flagEmojiSupport');

    expect(ensureFlagFont()).toBe(true);
    expect(canvas.getContext).toHaveBeenCalledWith('2d');
    expect(storage.setItem).toHaveBeenCalledWith(
      'ridge.flagEmojiSupport',
      writeFlagCache(false, 'UA-test'),
    );
    expect(styles).toHaveLength(1);
  });

  it('survives unavailable storage and canvas measurement', async () => {
    const { canvas, storage, styles } = installDom({
      measure: () => 0,
      readStorage: () => { throw new Error('private mode'); },
      writeStorage: () => { throw new Error('private mode'); },
    });
    canvas.getContext.mockReturnValueOnce(null);
    const { ensureFlagFont } = await import('./flagEmojiSupport');

    expect(ensureFlagFont()).toBe(false);
    expect(styles).toHaveLength(0);
    expect(storage.getItem).toHaveBeenCalled();
  });
});
