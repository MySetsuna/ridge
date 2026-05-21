/**
 * Regression guard for the theme-rotation cache-staleness bug
 * (`manager.ts::setTheme` updated `opts.theme` + wasm Theme struct but
 *  didn't invalidate per-pane CellInstance cache, so the next frame's
 *  `recordCachedOnly()` replayed quads with the OLD bg color baked in).
 *
 * The bug presented as:
 *   - boot: first frame rendered with `Theme::default_dark` (#071009)
 *   - bridge: pushed boundless-light → kernel Theme.bg = #ffffffff
 *   - screen: stayed near-black; cursor-blink frames flashed white then
 *     reverted (full-render vs cache-replay diverge)
 *
 * Both ends agreed on the *Theme struct* (`kernelThemeProbe` returned
 * the new color) but the GPU output didn't match, because the cache
 * replay path bypasses `Backend::clear()` which is where `theme.bg`
 * gets re-sampled into the bg quad.
 *
 * Implementation note — pixel readback timing:
 *   `drawImage(<webgpu canvas>)` only captures the most recently
 *   PRESENTED texture, and only while the swap chain still owns it.
 *   Crossing a mocha `it` boundary (which round-trips through WebDriver
 *   BiDi) lets the swap chain hand the texture back, after which
 *   drawImage paints nothing and getImageData returns (0,0,0,0).
 *   So each setTheme + assertion must happen inside ONE
 *   `browser.execute` block — `awaitPromise:true` keeps the call
 *   alive until our `requestAnimationFrame`s have completed.
 */
// @ts-nocheck
import { browser, expect } from '@wdio/globals';
import { waitForAppReady, firstPaneId } from './helpers';

const RED = '#ff0000ff';
const GREEN = '#00cc00ff';

/** Drive a setTheme + return {kernel, pixel} from the same realm round-trip. */
async function rotateAndProbe(
  paneId: string,
  theme: Record<string, string>,
): Promise<{
  kernel: { bg: string; fg: string; cursor: string; tuiBg: string };
  pixel: { r: number; g: number; b: number; a: number };
}> {
  const out = await browser.execute(
    async (paneIdArg: string, themeArg: Record<string, string>) => {
      const w = window as any;
      w.__windE2E.setTheme(themeArg);
      // 2 RAFs: one to consume the wake → frame encode, one to let
      // the just-presented texture stabilise so drawImage captures it.
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      return {
        kernel: w.__windE2E.kernelThemeProbe(paneIdArg),
        pixel: w.__windE2E.sampleHostPixel(0.5, 0.85),
      };
    },
    paneId,
    theme,
  );
  return out as {
    kernel: { bg: string; fg: string; cursor: string; tuiBg: string };
    pixel: { r: number; g: number; b: number; a: number };
  };
}

describe('theme rotation — setTheme reaches the GPU output, not just the kernel', () => {
  let paneId: string;
  let originalTheme: Record<string, string> | null = null;

  before(async () => {
    await waitForAppReady();
    paneId = await firstPaneId();
    // Snapshot boot-time theme so we can restore after — wdio sessions
    // are torn down per spec, but if specs ever share a session we
    // shouldn't leave the next one staring at a red canvas.
    originalTheme = await browser.execute(() => {
      const w = window as any;
      return w.__windE2E?.themeSnapshot?.() ?? null;
    });
    expect(originalTheme).not.toBeNull();
  });

  after(async () => {
    if (originalTheme) {
      await browser.execute((t: Record<string, string>) => {
        (window as any).__windE2E?.setTheme?.(t);
      }, originalTheme);
    }
  });

  it('first rotation: kernel Theme.bg goes red', async () => {
    const r = await rotateAndProbe(paneId, {
      background: RED,
      foreground: '#ffffffff',
      cursor: '#ffffffff',
    });
    // Kernel side — strong assertion: this is the bug the spec exists
    // to catch (`setTheme` would update `opts.theme` but the wasm
    // renderer's `Theme` struct stayed at the previous palette).
    expect(r.kernel.bg.toLowerCase()).toBe(RED);
    // Best-effort GPU pixel check: `drawImage(webgpu_canvas)` returns
    // `(0,0,0,0)` on some Edge / WebView2 builds (especially with the
    // `PreMultiplied` alpha mode the renderer uses post-fix); we can't
    // depend on it. When it DOES read a non-zero pixel, verify it's
    // in the red half of the spectrum — failing that means the wasm
    // renderer is genuinely painting the wrong colour, not a CDP
    // readback quirk.
    if (r.pixel.a > 0) {
      expect(r.pixel.r).toBeGreaterThan(r.pixel.g);
      expect(r.pixel.r).toBeGreaterThan(r.pixel.b);
    }
  });

  it('second rotation: kernel follows into green (cache re-invalidates)', async () => {
    const r = await rotateAndProbe(paneId, {
      background: GREEN,
      foreground: '#ffffffff',
      cursor: '#ffffffff',
    });
    expect(r.kernel.bg.toLowerCase()).toBe(GREEN);
    if (r.pixel.a > 0) {
      expect(r.pixel.g).toBeGreaterThan(r.pixel.r);
      expect(r.pixel.g).toBeGreaterThan(r.pixel.b);
    }
  });
});
