import { afterEach, describe, expect, it } from 'vitest';
import { applyThemeVars, themeChromeColor } from './theme';

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
});
