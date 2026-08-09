import { afterEach, describe, expect, it, vi } from 'vitest';
import { applyThemeVars, buildKernelTheme, themeChromeColor } from './theme';

class FakeStyle {
  private _cssText = '';
  private _backgroundColor = '';

  get cssText(): string { return this._cssText; }
  set cssText(value: string) {
    this._cssText = value;
    const match = value.match(/background-color:\s*([^;]+);?/);
    if (match) this._backgroundColor = match[1];
  }

  get backgroundColor(): string { return this._backgroundColor; }
  set backgroundColor(value: string) {
    this._backgroundColor = value;
    this._cssText += `background-color: ${value};`;
  }

  setProperty(name: string, value: string): void {
    this._cssText += `${name}: ${value};`;
  }
}

function installDocument() {
  const root = { style: new FakeStyle() };
  const body = { style: new FakeStyle() };
  const meta = { content: '#0d1117' };
  const doc = {
    documentElement: root,
    body,
    createElement: () => ({ style: new FakeStyle() }),
    querySelector: (selector: string) => selector === 'meta[name="theme-color"]' ? meta : null,
  };
  const previous = (globalThis as { document?: unknown }).document;
  (globalThis as { document?: unknown }).document = doc;
  return { root, body, meta, previous };
}

afterEach(() => {
  // Keep the node test environment isolated; the production helper is a no-op
  // when no document exists (SSR/worker paths).
  (globalThis as { document?: unknown }).document = undefined;
});

describe('themeChromeColor', () => {
  it('prefers chrome background and falls back to terminal background', () => {
    expect(themeChromeColor({ bg: '#15202b', 'term-bg': '#071009' })).toBe('#15202b');
    expect(themeChromeColor({ 'term-bg': '#071009' })).toBe('#071009');
    expect(themeChromeColor({ bg: '  ' })).toBeNull();
  });
});

describe('applyThemeVars', () => {
  it('atomically updates vars and keeps PWA edge colours in sync', () => {
    const { root, body, meta, previous } = installDocument();
    try {
      root.style.cssText = '--startup-accent: #fff;';
      applyThemeVars({ bg: '#15202b', accent: '#58a6ff' });

      expect(root.style.cssText).toContain('--startup-accent: #fff;');
      expect(root.style.cssText).toContain('--rg-bg: #15202b;');
      expect(root.style.cssText).toContain('--rg-accent: #58a6ff;');
      expect(root.style.backgroundColor).toBe('#15202b');
      expect(body.style.backgroundColor).toBe('#15202b');
      expect(meta.content).toBe('#15202b');
    } finally {
      (globalThis as { document?: unknown }).document = previous;
    }
  });

  it('is safe without a document and supports the minimal style fallback', () => {
    const previous = (globalThis as { document?: unknown }).document;
    try {
      (globalThis as { document?: unknown }).document = undefined;
      expect(() => applyThemeVars({ bg: '#000' })).not.toThrow();

      const root = { style: { setProperty: vi.fn(), backgroundColor: '' } };
      const body = { style: { backgroundColor: '' } };
      const meta = { content: '' };
      (globalThis as { document?: unknown }).document = {
        documentElement: root,
        body,
        createElement: () => { throw new Error('minimal shim'); },
        querySelector: () => meta,
      };
      applyThemeVars({ bg: '#101010', '': 'ignored', accent: 7 as unknown as string });
      expect(root.style.setProperty).toHaveBeenCalledWith('--rg-bg', '#101010');
      expect(root.style.setProperty).not.toHaveBeenCalledWith('--rg-', expect.anything());
      expect(root.style.backgroundColor).toBe('#101010');
      expect(body.style.backgroundColor).toBe('#101010');
      expect(meta.content).toBe('#101010');
    } finally {
      (globalThis as { document?: unknown }).document = previous;
    }
  });
});

describe('buildKernelTheme', () => {
  function installColorDocument() {
    const ctx = {
      value: '#000000',
      get fillStyle(): string { return this.value; },
      set fillStyle(value: string) {
        const hex = value.trim().toLowerCase();
        if (/^#[0-9a-f]{3}$/.test(hex)) {
          this.value = `#${hex[1]}${hex[1]}${hex[2]}${hex[2]}${hex[3]}${hex[3]}`;
        } else if (/^#[0-9a-f]{6}$/.test(hex)) {
          this.value = hex;
        } else if (/^#[0-9a-f]{8}$/.test(hex)) {
          const r = parseInt(hex.slice(1, 3), 16);
          const g = parseInt(hex.slice(3, 5), 16);
          const b = parseInt(hex.slice(5, 7), 16);
          const a = parseInt(hex.slice(7, 9), 16) / 255;
          this.value = `rgba(${r}, ${g}, ${b}, ${a})`;
        } else {
          this.value = value;
        }
      },
    };
    vi.stubGlobal('document', {
      createElement: () => ({ getContext: () => ctx }),
    });
  }

  it('normalizes terminal, cursor, selection, and ANSI palette colors', () => {
    installColorDocument();
    const result = buildKernelTheme({
      bg: '#101010',
      'term-bg': 'rgba(20, 30, 40, .5)',
      fg: '#f0f0f0',
      accent: '#58a6ff',
      'selection-bg': '#30363d',
      'ansi-red': '#ff0000',
      'ansi-brightWhite': '#fff',
    });

    expect(result).toMatchObject({
      background: '#141e2880',
      foreground: '#f0f0f0ff',
      cursor: '#58a6ffff',
      cursorAccent: '#141e2880',
      hyperlinkColor: '#58a6ffff',
      selectionBackground: '#30363dff',
      red: '#ff0000ff',
      brightWhite: '#ffffffff',
    });
  });

  it('uses the accent alpha fallback and omits incomplete fields', () => {
    installColorDocument();
    const result = buildKernelTheme({ accent: '#12345678', 'ansi-blue': '  ' });
    expect(result).toEqual({
      cursor: '#12345678',
      hyperlinkColor: '#12345678',
      selectionBackground: '#1234563d',
    });
  });
});
