import { retryableLazyImport } from './retryableLazyImport';
import type { TerminalKernel } from '@ridge/term-wasm';
const importFont = /* @__PURE__ */ retryableLazyImport(() => import('@ridge/term-font-wasm'));
const importAsset = /* @__PURE__ */ retryableLazyImport(() => import('@ridge/term-font-wasm/ridge_term_font_bg.wasm?url'));
/** Selected-font engine is optional; Remote browser fonts need no font WASM. */
let fontRuntime: Promise<typeof import('@ridge/term-font-wasm')> | null = null;

export function loadSelectedFontRuntime(): Promise<typeof import('@ridge/term-font-wasm')> {
  if (fontRuntime) return fontRuntime;
  const pending = Promise.all([
    importFont(),
    importAsset(),
  ]).then(async ([module, asset]) => {
    await module.default({ module_or_path: asset.default });
    return module;
  });
  fontRuntime = pending;
  void pending.catch(() => { if (fontRuntime === pending) fontRuntime = null; });
  return pending;
}

const importRenderer = /* @__PURE__ */ retryableLazyImport(() => import('@ridge/term-render-wasm'));
const importRendererAsset = /* @__PURE__ */ retryableLazyImport(() => import('@ridge/term-render-wasm/ridge_term_render_bg.wasm?url'));
let rendererRuntime: Promise<typeof import('@ridge/term-render-wasm')> | null = null;

export function loadRendererRuntime(): Promise<typeof import('@ridge/term-render-wasm')> {
  if (rendererRuntime) return rendererRuntime;
  const pending = Promise.all([importRenderer(), importRendererAsset()]).then(async ([module, asset]) => {
    await module.default({ module_or_path: asset.default });
    return module;
  });
  rendererRuntime = pending;
  void pending.catch(() => { if (rendererRuntime === pending) rendererRuntime = null; });
  return pending;
}

function delegate<T extends object>(wrapper: T, raw: object): T {
  return new Proxy(wrapper, { get(target, property) {
    if (property in target) return Reflect.get(target, property);
    const value = Reflect.get(raw, property);
    return typeof value === 'function' ? value.bind(raw) : value;
  } });
}

type RawHandle = import('@ridge/term-render-wasm').RenderHandle;
type RawHost = import('@ridge/term-render-wasm').SurfaceHostHandle;
export interface SurfaceHostHandle extends RawHost {}
export class SurfaceHostHandle {
  private constructor(readonly raw: RawHost) { return delegate(this, raw); }
  static async init(canvas: HTMLCanvasElement): Promise<SurfaceHostHandle> {
    const module = await loadRendererRuntime();
    return new SurfaceHostHandle(await module.SurfaceHostHandle.init(canvas));
  }
  clone(): SurfaceHostHandle { return new SurfaceHostHandle(this.raw.clone()); }
}

export interface RenderHandle extends Omit<RawHandle, 'render' | 'isDirty' | 'nextBlinkDeadlineMs'> {}
export class RenderHandle {
  private revision = 0;
  private boundKernel: TerminalKernel | null = null;
  private constructor(private raw: RawHandle) { return delegate(this, raw); }
  static async newWithWebgpuFirst(canvas: HTMLCanvasElement, host: SurfaceHostHandle): Promise<RenderHandle> {
    const module = await loadRendererRuntime();
    return new RenderHandle(await module.RenderHandle.newWithWebgpuFirst(canvas, host.raw));
  }
  private sync(kernel: TerminalKernel): void {
    if (this.boundKernel !== kernel) { this.boundKernel = kernel; this.revision = 0; this.raw.invalidateAll(); }
    const bytes = kernel.presentationFrame(this.revision);
    if (!bytes) return;
    try { this.revision = this.raw.applyPresentation(bytes); }
    catch (error) { this.revision = 0; throw error; }
  }
  render(kernel: TerminalKernel): boolean { this.sync(kernel); return this.raw.render(); }
  isDirty(kernel: TerminalKernel, now: number): boolean { this.sync(kernel); return this.raw.isDirty(now); }
  nextBlinkDeadlineMs(kernel: TerminalKernel, now: number): number { this.sync(kernel); return this.raw.nextBlinkDeadlineMs(now); }
}
