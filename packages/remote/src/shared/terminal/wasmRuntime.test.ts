import { existsSync, readFileSync } from 'node:fs';
import { beforeEach, describe, expect, it, vi } from 'vitest';
const mocks = vi.hoisted(() => ({ initialize: vi.fn() }));
const systemFont = ['C:/Windows/Fonts/consola.ttf', '/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf', '/System/Library/Fonts/Menlo.ttc'].find(existsSync);
vi.mock('@ridge/term-font-wasm', () => ({ default: mocks.initialize, FontRasterizer: class {}, installFontData: vi.fn() }));
vi.mock('@ridge/term-font-wasm/ridge_term_font_bg.wasm?url', () => ({ default: '/font.wasm' }));

describe('selected-font loading', () => {
  beforeEach(() => { vi.resetModules(); mocks.initialize.mockReset().mockResolvedValue(undefined); });
  it('shares concurrent initialization', async () => {
    const { loadSelectedFontRuntime } = await import('./wasmRuntime');
    const first = loadSelectedFontRuntime();
    expect(loadSelectedFontRuntime()).toBe(first);
    const module = await first;
    expect(mocks.initialize).toHaveBeenCalledExactlyOnceWith({ module_or_path: '/font.wasm' });
    expect(await loadSelectedFontRuntime()).toBe(module);
  });
  it('retries failed WASM initialization without poisoning the loaded JS module', async () => {
    mocks.initialize.mockRejectedValueOnce(new Error('network unavailable'));
    const { loadSelectedFontRuntime } = await import('./wasmRuntime');
    await expect(loadSelectedFontRuntime()).rejects.toThrow('network unavailable');
    await expect(loadSelectedFontRuntime()).resolves.toHaveProperty('FontRasterizer');
    expect(mocks.initialize).toHaveBeenCalledTimes(2);
  });
  it.skipIf(!systemFont)('runs real font WASM bindings, validates bytes and produces metrics and bitmaps', async () => {
    const font = await vi.importActual<typeof import('@ridge/term-font-wasm')>('@ridge/term-font-wasm');
    font.initSync({ module: readFileSync('packages/ridge-term-font/pkg/ridge_term_font_bg.wasm') });
    expect(() => font.installFontData(new Uint8Array([1, 2, 3, 4]))).toThrow();
    const bytes = readFileSync(systemFont!);
    expect(font.installFontData(bytes)).toBe(true);
    expect(font.installFontData(bytes)).toBe(false);
    const rasterizer = new font.FontRasterizer(256, 256);
    try {
      const metrics = rasterizer.measure('monospace', 18);
      expect(metrics).toBeInstanceOf(Float32Array);
      expect(Array.from(metrics).every(value => Number.isFinite(value) && value > 0)).toBe(true);
      const bitmap = rasterizer.rasterize('monospace', 18, 2, 0, '─');
      expect(bitmap).toBeInstanceOf(Uint8Array);
      expect(bitmap.length).toBeGreaterThan(32);
    } finally { rasterizer.free(); }
  });
  it('ships independent kernel and renderer exports and copies no idle presentation', async () => {
    const core = await vi.importActual<typeof import('@ridge/term-wasm')>('@ridge/term-wasm');
    const renderer = await vi.importActual<typeof import('@ridge/term-render-wasm')>('@ridge/term-render-wasm');
    expect(core).not.toHaveProperty('RenderHandle');
    expect(renderer).not.toHaveProperty('TerminalKernel');
    core.initSync({ module: readFileSync('packages/ridge-term/pkg/ridge_term_bg.wasm') });
    const kernel = new core.TerminalKernel(3, 24, 20);
    try {
      kernel.feed(new TextEncoder().encode('中文👩‍🔧'));
      const first = kernel.presentationFrame(0);
      expect(first).toBeInstanceOf(Uint8Array);
      expect(kernel.presentationFrame(1)).toBeUndefined();
      kernel.feed(new TextEncoder().encode('\x1b[2;4H'));
      const cursor = kernel.presentationFrame(1)!;
      expect(cursor.byteLength).toBeLessThan(first!.byteLength);
    } finally { kernel.free(); }
  });
});
