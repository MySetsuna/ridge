/**
 * Regression guard for the boot-time theme-injection chain
 * (Rust `get_theme_data` → SvelteKit `initThemeSystem` →
 *  `applyTheme` CSS-var write → `setupTerminalThemeBridge` →
 *  `manager.setTheme` → wasm kernel `apply_theme`).
 *
 * The chain has bitten us twice:
 *   1. `bundle.resources` array form put `ridge.theme` under `_up_/`
 *      so `find_theme_path()` returned None → empty theme catalog →
 *      every downstream step silently no-op'd.
 *   2. The theme bridge's RAF fired after the first pane attached,
 *      AND attach() saw a null `opts.theme`, so the kernel kept
 *      its compile-time defaults even though the page chrome had
 *      the right CSS vars.
 *
 * This spec asserts both ends are consistent:
 *   - `--rg-term-bg` is non-empty on `documentElement` (CSS side).
 *   - `__windE2E.themeSnapshot().background` is set AND parses to the
 *     same RGB triple as `--rg-term-bg` (kernel side).
 *
 * If either fails, the bug is back. The diff between the two pinpoints
 * which half of the chain regressed.
 */
// @ts-nocheck
import { browser, expect } from '@wdio/globals';
import { waitForAppReady } from './helpers';

/** Parse `#RRGGBB` / `#RRGGBBAA` / `rgb(r, g, b)` / `rgba(...)` into
 *  `{r,g,b}`. Returns null on unparseable input. Mirrors the canvas-2d
 *  normalisation logic in `src/lib/utils/cssColor.ts` minus the alpha
 *  channel — alpha differs between CSS-var form (no alpha) and the
 *  kernel's bridge-pushed form (`hex8` always appends `ff`), so we
 *  only require RGB to match. */
function parseRgb(input: string): { r: number; g: number; b: number } | null {
  const s = input.trim();
  if (!s) return null;
  if (s.startsWith('#')) {
    if (s.length !== 7 && s.length !== 9) return null;
    const r = parseInt(s.slice(1, 3), 16);
    const g = parseInt(s.slice(3, 5), 16);
    const b = parseInt(s.slice(5, 7), 16);
    if ([r, g, b].some((n) => Number.isNaN(n))) return null;
    return { r, g, b };
  }
  const m = s.match(/^rgba?\((\d+),\s*(\d+),\s*(\d+)/);
  if (m) {
    return { r: +m[1], g: +m[2], b: +m[3] };
  }
  return null;
}

describe('theme injection — Rust ridge.theme reaches the wasm kernel', () => {
  before(async () => {
    await waitForAppReady();
  });

  it('CSS side: --rg-term-bg is set on documentElement', async () => {
    const value = await browser.execute(() => {
      return getComputedStyle(document.documentElement)
        .getPropertyValue('--rg-term-bg')
        .trim();
    });
    expect(value).not.toBe('');
    const parsed = parseRgb(value);
    expect(parsed).not.toBeNull();
  });

  it('kernel side: __windE2E.themeSnapshot() reports a non-empty background', async () => {
    const snapshot = await browser.execute(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const w = window as any;
      return w.__windE2E?.themeSnapshot?.() ?? null;
    });
    expect(snapshot).not.toBeNull();
    expect(snapshot!.background).toBeTruthy();
    // The bridge normalises to `#RRGGBBAA` (8-digit hex). Anything else
    // points at a regression in `cssColor.ts::hex8` or its callers.
    expect(snapshot!.background).toMatch(/^#[0-9a-f]{8}$/i);
  });

  it('CSS and kernel agree on the terminal background RGB triple', async () => {
    const result = await browser.execute(() => {
      const css = getComputedStyle(document.documentElement)
        .getPropertyValue('--rg-term-bg')
        .trim();
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const w = window as any;
      const snap = w.__windE2E?.themeSnapshot?.() ?? null;
      return { css, kernelBg: snap?.background ?? null };
    });
    const cssRgb = parseRgb(result.css);
    const kernelRgb = result.kernelBg ? parseRgb(result.kernelBg) : null;
    expect(cssRgb).not.toBeNull();
    expect(kernelRgb).not.toBeNull();
    // R/G/B must match — alpha drift is acceptable (CSS form has no
    // alpha; the kernel always carries `ff`).
    expect(kernelRgb!.r).toBe(cssRgb!.r);
    expect(kernelRgb!.g).toBe(cssRgb!.g);
    expect(kernelRgb!.b).toBe(cssRgb!.b);
  });
});
